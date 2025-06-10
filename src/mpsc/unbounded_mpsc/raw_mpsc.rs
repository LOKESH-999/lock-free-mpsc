use std::{
    fmt::Debug, hint::spin_loop, mem::transmute, sync::atomic::{
        fence, AtomicBool, AtomicPtr, Ordering::{AcqRel, Acquire, Relaxed, Release}
    }
};

use crate::{
    backoff::LocalBackoff,
    mpsc::unbounded_mpsc::segment_arr::{SEGMENT_SIZE, Segment},
};

pub struct RawMpsc<T> {
    head: AtomicPtr<Segment<T>>,
    tail: AtomicPtr<Segment<T>>,
    segment_allocation_pending: AtomicBool,
}

impl<T: Debug> RawMpsc<T> {
    pub fn new() -> Self {
        let segment_ptr = Box::into_raw(Box::new(Segment::new()));
        let head = AtomicPtr::new(segment_ptr);
        let tail = AtomicPtr::new(segment_ptr);
        let segment_allocation_pending = AtomicBool::new(false);
        Self {
            head,
            tail,
            segment_allocation_pending,
        }
    }

    #[inline]
    fn wait_for_seg_alloc(&self){
        while self.segment_allocation_pending.load(Acquire){
            spin_loop();
        }
    }

    pub fn push(&self, mut data: T) {
        loop{
            self.wait_for_seg_alloc();
            let tail = self.tail.load(Acquire);
            let segment = unsafe{&*tail};
            match Self::segment_push(segment, data){
                Ok(_)=>{return;}
                Err(d)=>{
                    data = d;
                    match self.segment_allocation_pending.compare_exchange(false, true, AcqRel, Relaxed){
                        Ok(_)=>{
                            let new_block = Box::into_raw(Box::new(Segment::new()));
                            segment.next.set(new_block);
                            fence(Release);
                            self.tail.store(new_block, Release);
                            self.segment_allocation_pending.store(false, Release);
                        },
                        Err(_)=>continue
                    }
                }
            }
        }
    }

    pub fn pop(&self) -> Option<T> {
        loop {
            let head = self.head.load(Acquire);
            println!("{:p}",head);
            let segment = unsafe { &*head };
            match Self::segment_pop(segment){
                Some(data) => return Some(data),
                None=>{
                    if self.tail.load(Acquire) != head {
                        fence(Acquire);
                        let next = segment.next.get();
                        self.head.store(next, Release);
                        let _dropping = unsafe { Box::from_raw(head) };
                        continue;
                    }
                    break None;
                }
            }
        }
    }

    fn segment_push(segment: &Segment<T>, data: T) -> Result<(), T> {
        let backoff = LocalBackoff::new();
        loop {
            let curr_head = segment.next_head.load(Acquire);
            let next_unbound = curr_head + 1;
            // bounding within range without mod for performance
            let is_bound =
                unsafe { transmute::<isize, usize>(-((next_unbound < SEGMENT_SIZE) as isize)) };
            let next_head = next_unbound & is_bound;
            if segment.tail.load(Acquire) != next_head {
                match segment
                    .next_head
                    .compare_exchange(curr_head, next_head, AcqRel, Relaxed)
                {
                    Ok(_) => {
                        // shodnt pannic if so then there is error in logic
                        println!("push:{},{}",curr_head,next_head);
                        segment.set(curr_head, data).unwrap();
                        return Ok(());
                    }
                    Err(_) => backoff.wait(),
                }
            } else {
                return Err(data);
            }
        }
    }
    fn segment_pop(segment: &Segment<T>) -> Option<T> {
        let head = segment.next_head.load(Acquire);
        let tail = segment.tail.load(Acquire);
        if head != tail {
            let ptr = segment.buff.as_ptr();
            let slot = unsafe { &*ptr.add(tail) };
            // bounding within range without mod for performance
            let is_bound = unsafe { transmute::<isize, usize>(-((tail + 1 < SEGMENT_SIZE) as isize)) };
            let next_tail = (tail + 1) & is_bound;
            segment.tail.store(next_tail, Release);
            println!("{},{}",head,tail);
            // shodnt pannic if so then there is error in logic
            return Some(slot.unset().unwrap());
        }
        return None;
    }
}



#[cfg(test)]
mod tests {
    use super::RawMpsc;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::collections::BTreeSet;
    use std::time::Duration;

    // Basic test: push then pop single element in one thread
    #[test]
    fn test_push_pop_single_thread() {
        let q = RawMpsc::new();

        q.push(42);
        let val = q.pop();

        assert_eq!(val, Some(42));
        assert_eq!(q.pop(), None);
    }

    // Test pop on empty queue returns None immediately
    #[test]
    fn test_pop_empty_queue() {
        let q: RawMpsc<u32> = RawMpsc::new();
        assert_eq!(q.pop(), None);
    }

    // Test pushing and popping multiple elements sequentially
    #[test]
    fn test_multiple_push_pop_single_thread() {
        let q = RawMpsc::new();

        for i in 0..1000 {
            q.push(i);
        }

        for i in 0..1000 {
            assert_eq!(q.pop(), Some(i));
        }

        assert_eq!(q.pop(), None);
    }

    // Concurrent: multiple producers, single consumer, basic check for all messages
    // #[test]
    fn test_multi_producer_single_consumer_basic() {
        const PRODUCERS: usize = 4;
        const MSGS_PER_PRODUCER: usize = 1000;
        const TOTAL_MSGS: usize = PRODUCERS * MSGS_PER_PRODUCER;

        let q = Arc::new(RawMpsc::new());
        let barrier = Arc::new(Barrier::new(PRODUCERS + 1));

        for pid in 0..PRODUCERS {
            let q_cloned = Arc::clone(&q);
            let b = Arc::clone(&barrier);

            thread::spawn(move || {
                b.wait();
                for i in 0..MSGS_PER_PRODUCER {
                    q_cloned.push((pid, i));
                }
            });
        }

        barrier.wait();

        let mut seen = BTreeSet::new();
        while seen.len() < TOTAL_MSGS {
            if let Some(msg) = q.pop() {
                seen.insert(msg);
            } else {
                thread::yield_now();
            }
        }

        assert_eq!(seen.len(), TOTAL_MSGS);

        // Check all expected messages are received
        for pid in 0..PRODUCERS {
            for val in 0..MSGS_PER_PRODUCER {
                assert!(seen.contains(&(pid, val)), "Missing message ({}, {})", pid, val);
            }
        }
    }

    // Stress test: larger number of producers and messages
    #[test]
    fn test_stress_multi_producer_single_consumer() {
        const PRODUCERS: usize = 8;
        const MSGS_PER_PRODUCER: usize = 10_000;
        const TOTAL_MSGS: usize = PRODUCERS * MSGS_PER_PRODUCER;

        let q = Arc::new(RawMpsc::new());
        let barrier = Arc::new(Barrier::new(PRODUCERS + 1));

        for pid in 0..PRODUCERS {
            let q_cloned = Arc::clone(&q);
            let b = Arc::clone(&barrier);

            thread::spawn(move || {
                b.wait();
                for i in 0..MSGS_PER_PRODUCER {
                    q_cloned.push((pid, i));
                }
            });
        }

        barrier.wait();

        let mut seen = BTreeSet::new();
        while seen.len() < TOTAL_MSGS {
            if let Some(msg) = q.pop() {
                seen.insert(msg);
            } else {
                thread::yield_now();
            }
        }

        assert_eq!(seen.len(), TOTAL_MSGS);
    }

    // Test that popping from empty after exhaustion returns None
    #[test]
    fn test_pop_after_exhaustion() {
        let q = RawMpsc::new();
        q.push(1);
        assert_eq!(q.pop(), Some(1));
        assert_eq!(q.pop(), None);
        assert_eq!(q.pop(), None);
    }

    // Test concurrent push with slight delays to simulate contention
    #[test]
    fn test_multi_producer_with_delay() {
        const PRODUCERS: usize = 4;
        const MSGS_PER_PRODUCER: usize = 1000;
        const TOTAL_MSGS: usize = PRODUCERS * MSGS_PER_PRODUCER;

        let q = Arc::new(RawMpsc::new());
        let barrier = Arc::new(Barrier::new(PRODUCERS + 1));

        for pid in 0..PRODUCERS {
            let q_cloned = Arc::clone(&q);
            let b = Arc::clone(&barrier);

            thread::spawn(move || {
                b.wait();
                for i in 0..MSGS_PER_PRODUCER {
                    q_cloned.push((pid, i));
                    if i % 100 == 0 {
                        std::thread::sleep(Duration::from_micros(10));
                    }
                }
            });
        }

        barrier.wait();

        let mut seen = BTreeSet::new();
        while seen.len() < TOTAL_MSGS {
            if let Some(msg) = q.pop() {
                seen.insert(msg);
            } else {
                thread::yield_now();
            }
        }

        assert_eq!(seen.len(), TOTAL_MSGS);
    }

    // Optional: test with custom struct instead of tuple
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct Message {
        producer_id: usize,
        value: usize,
    }

    #[test]
    fn test_custom_struct_message() {
        let q = RawMpsc::new();

        let msg = Message { producer_id: 1, value: 42 };
        q.push(msg);

        let popped = q.pop().unwrap();
        assert_eq!(popped, Message { producer_id: 1, value: 42 });
    }
}
