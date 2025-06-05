//! A low-level bounded Multi-Producer Single-Consumer (MPSC) queue.
//!
//! This implementation provides a non-blocking, lock-free bounded queue where
//! multiple producer threads can push data, but only a single consumer is allowed
//! to pop data.
//!
//! Internally, it uses an array of slots with atomic head and tail indices, along
//! with an exponential backoff strategy to handle contention efficiently.

use std::{fmt::Debug, mem::transmute, sync::atomic::AtomicUsize};
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};

use crate::{backoff::GlobalBackoff, cache_padded::CachePadded};
use super::slot_arr::SlotArr;

/// A bounded lock-free multi-producer single-consumer (MPSC) queue.
///
/// `RawMpsc<T>` supports multiple threads concurrently pushing elements
/// while allowing only one thread to pop elements. It uses a ring buffer internally,
/// and contention is reduced using a global exponential backoff strategy.
///
/// This is a low-level primitive used by higher-level channel abstractions.
pub struct RawMpsc<T> {
    /// The next index to be pushed to by producers.
    next_head: CachePadded<AtomicUsize>,
    /// The next index to be popped by the single consumer.
    tail: CachePadded<AtomicUsize>,
    /// Global exponential backoff to reduce contention during CAS failure.
    global_wait: CachePadded<GlobalBackoff>,
    /// Internal storage array for queue slots.
    slots: SlotArr<T>,
}

impl<T: Debug> RawMpsc<T> {
    /// Creates a new bounded MPSC queue with the given capacity.
    ///
    /// Internally allocates `capacity + 1` slots to avoid ambiguity between full and empty.
    pub fn new(capacity: usize) -> Self {
        let slots = SlotArr::new(capacity + 1);
        let next_head = CachePadded::new(AtomicUsize::new(0));
        let tail = CachePadded::new(AtomicUsize::new(0));
        let global_wait = CachePadded::new(GlobalBackoff::new());

        Self {
            next_head,
            tail,
            global_wait,
            slots,
        }
    }

    /// Attempts to push data into the queue.
    ///
    /// Returns `Ok(())` if the push succeeded, or returns the original `data` back
    /// in `Err(data)` if the queue is full.
    pub fn push(&self, data: T) -> Result<(), T> {
        unsafe { self.global_wait.reg_wait() };
        let curr_head = loop {
            let curr_head = self.next_head.load(Acquire);
            let next_head = curr_head + 1;

            // Bounds the index to wrap around at capacity
            let is_less =
                unsafe { transmute::<isize, usize>(-((next_head < self.slots.capacity) as isize)) };
            let next_head_bounded = next_head & is_less;

            if next_head_bounded != self.tail.load(Acquire) {
                match self
                    .next_head
                    .compare_exchange(curr_head, next_head_bounded, AcqRel, Acquire)
                {
                    Ok(_) => {
                        unsafe { self.global_wait.de_reg() };
                        break curr_head;
                    }
                    Err(_) => self.global_wait.wait(),
                }
            } else {
                unsafe { self.global_wait.de_reg() };
                return Err(data);
            }
        };

        self.slots.set(curr_head, data).unwrap(); // infallible under valid usage
        Ok(())
    }

    /// Attempts to pop a value from the queue.
    ///
    /// Returns `Some(T)` if a value was available, or `None` if the queue is empty.
    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Acquire);
        let head = self.next_head.load(Acquire);

        if tail != head {
            match self.slots.unset(tail) {
                Ok(data) => {
                    let next_tail = tail + 1;
                    let is_less =
                        unsafe { transmute::<isize, usize>(-((next_tail < self.slots.capacity) as isize)) };
                    let next_tail_bounded = next_tail & is_less;
                    self.tail.store(next_tail_bounded, Release);
                    Some(data)
                }
                Err(_) => {
                    // Corruption or double-pop should not happen in valid single-consumer usage
                    None
                }
            }
        } else {
            None
        }
    }
}

impl<T> Drop for RawMpsc<T> {
    /// Drops the queue and all remaining values in it.
    ///
    /// Any items that have not been consumed are dropped here.
    fn drop(&mut self) {
        let head = self.next_head.load(Acquire);
        let tail = self.tail.load(Acquire);
        let mut curr = head;

        while curr != tail {
            let next_curr = {
                let next = curr + 1;
                let is_less =
                    unsafe { transmute::<isize, usize>(-((next < self.slots.capacity) as isize)) };
                next & is_less
            };
            let _ = self.slots.unset(curr);
            curr = next_curr;
        }
    }
}

// SAFETY: `RawMpsc` is `Send` and `Sync` as long as `T` is properly handled within the SlotArr.
unsafe impl<T> Send for RawMpsc<T> {}
unsafe impl<T> Sync for RawMpsc<T> {}



#[cfg(all(test, not(no_std)))]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::collections::HashSet;

    #[test]
    fn test_basic_push_pop() {
        let q = RawMpsc::new(8);

        assert!(q.push(10).is_ok());
        assert!(q.push(20).is_ok());

        assert_eq!(q.pop(), Some(10));
        assert_eq!(q.pop(), Some(20));
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn test_push_until_full() {
        let q = RawMpsc::new(4);

        for i in 0..4 {
            assert!(q.push(i).is_ok());
        }

        // Now full, next push should fail
        assert_eq!(q.push(99), Err(99));
    }

    #[test]
    fn test_pop_from_empty() {
        let q = RawMpsc::<u64>::new(4);

        assert_eq!(q.pop(), None);
    }

    #[test]
    fn test_multi_producer_single_consumer() {
        const THREADS: usize = 8;
        const ITEMS_PER_THREAD: usize = 32;
        const CAPACITY: usize = THREADS * ITEMS_PER_THREAD;

        let q = Arc::new(RawMpsc::new(CAPACITY));
        let start_barrier = Arc::new(Barrier::new(THREADS + 1));

        let mut handles = Vec::with_capacity(THREADS);
        for t in 0..THREADS {
            let q = Arc::clone(&q);
            let barrier = Arc::clone(&start_barrier);
            handles.push(thread::spawn(move || {
                barrier.wait();
                for i in 0..ITEMS_PER_THREAD {
                    let value = t * 1000 + i;
                    loop {
                        if q.push(value).is_ok() {
                            break;
                        }
                        std::thread::yield_now(); // give other threads a chance
                    }
                }
            }));
        }

        // Synchronize start
        start_barrier.wait();

        // Pop all items
        let mut results = HashSet::new();
        while results.len() < THREADS * ITEMS_PER_THREAD {
            if let Some(val) = q.pop() {
                results.insert(val);
            } else {
                std::thread::yield_now();
            }
        }

        for h in handles {
            h.join().unwrap();
        }

        // Check all expected values were received
        for t in 0..THREADS {
            for i in 0..ITEMS_PER_THREAD {
                let expected = t * 1000 + i;
                assert!(results.contains(&expected), "missing {expected}");
            }
        }
    }

    #[test]
    fn test_order_preserved_single_thread() {
        let q = RawMpsc::new(8);

        for i in 0..8 {
            assert!(q.push(i).is_ok());
        }

        for i in 0..8 {
            assert_eq!(q.pop(), Some(i));
        }

        assert_eq!(q.pop(), None);
    }

    #[test]
    fn test_degenerate_capacity_zero() {
        let q = RawMpsc::<i32>::new(0);
        assert!(q.push(1).is_err());
        assert_eq!(q.pop(), None);
    }

    #[test]
    fn test_capacity_one_behavior() {
        let q = RawMpsc::new(1);
        assert!(q.push(1).is_ok());
        assert!(q.push(2).is_err());
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), None);
        assert!(q.push(3).is_ok()); // no wraparound
    }


    #[test]
    fn free_drop_test() {
        let q = RawMpsc::new(10);
        for i in 0..10{
            assert!(q.push(i).is_ok())
        }
    }
}
