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
        let slots = SlotArr::new(capacity);
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
        if tail != head {
            match self.slots.unset(tail){
                Ok(data)=>{
                    let is_less =
                        unsafe { transmute::<isize, usize>(-((tail < self.slots.capacity) as isize)) };
                    let next_tail_bounded = tail & is_less;
                    self.tail.store(next_tail_bounded, Release);
                    return Some(data);
                }
                Err(_)=>return None
            }
        }
        None
    }
}
