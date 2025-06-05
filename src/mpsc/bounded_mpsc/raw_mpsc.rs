use std::{fmt::Debug, mem::transmute, sync::atomic::AtomicUsize};

use crate::{backoff::GlobalBackoff, cache_padded::CachePadded};

use super::slot_arr::SlotArr;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};

pub struct RawMpsc<T> {
    next_head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    global_wait: CachePadded<GlobalBackoff>,
    slots: SlotArr<T>,
}

impl<T: Debug> RawMpsc<T> {
    pub fn new(capacity: usize) -> Self {
        let slots = SlotArr::new(capacity+1);
        let next_head = CachePadded::new(0.into());
        let tail = CachePadded::new(0.into());
        let global_wait = CachePadded::new(GlobalBackoff::new());
        Self {
            next_head,
            tail,
            slots,
            global_wait,
        }
    }
    pub fn push(&self, data: T) -> Result<(), T> {
        unsafe { self.global_wait.reg_wait() };
        let curr_head = loop {
            let curr_head = self.next_head.load(Acquire);
            let next_head = curr_head + 1;
            let is_less =
                unsafe { transmute::<isize, usize>(-((next_head < self.slots.capacity) as isize)) };
            let next_head_bounded = next_head & is_less;
            println!("curr: {curr_head}, next: {next_head_bounded}");
            if next_head_bounded != self.tail.load(Acquire) {
                match self
                    .next_head
                    .compare_exchange(curr_head, next_head_bounded, AcqRel, Acquire)
                {
                    Err(_) => self.global_wait.wait(),
                    Ok(_) => {
                        unsafe { self.global_wait.de_reg() };
                        break curr_head;
                    }
                }
            } else {
                unsafe { self.global_wait.de_reg() };
                return Err(data);
            }
        };
        // it shoudnt pannic
        self.slots.set(curr_head, data).unwrap();
        Ok(())
    }

    pub fn pop(&self) -> Option<T> {
        let tail = self.tail.load(Acquire);
        let head = self.next_head.load(Acquire);
        println!("tail : {tail},head : {head}");
        if tail != head {
            match self.slots.unset(tail){
                Ok(data)=>{
                    let next_tail = tail + 1;
                    let is_less =
                        unsafe { transmute::<isize, usize>(-((next_tail < self.slots.capacity) as isize)) };
                    let next_tail_bounded = next_tail & is_less;
                    println!("tail:{tail},new_bounded:{next_tail_bounded}");
                    self.tail.store(next_tail_bounded, Release);
                    return Some(data);
                }
                Err(_)=>{println!("unset err");return None}
            }
        }
        None
    }
}

unsafe impl<T> Send for RawMpsc<T>{}
unsafe impl<T> Sync for RawMpsc<T>{}


#[cfg(test)]
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
}
