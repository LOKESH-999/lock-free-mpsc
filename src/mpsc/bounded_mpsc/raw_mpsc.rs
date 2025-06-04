use std::{
    fmt::Debug,
    hint::spin_loop,
    mem::transmute,
    sync::atomic::{AtomicU32, AtomicUsize},
};

use crate::cache_padded::CachePadded;

use super::slot_arr::SlotArr;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release};

const MAX_WAIT_SPIN: u32 = 1 << 18;
const MIN_WAIT_SPIN: u32 = 32;
struct GlobalBackoff {
    active_threads: AtomicUsize,
}

impl GlobalBackoff {
    pub(crate) fn new() -> Self {
        GlobalBackoff {
            active_threads: 0.into(),
        }
    }

    #[inline(always)]
    pub(crate) fn reg_wait(&self) {
        let n_iters = self.active_threads.fetch_add(1, AcqRel);
        self.wait_with_spin(n_iters as u32);
    }

    #[inline(always)]
    pub(crate) fn wait(&self) {
        let n_iters = (MIN_WAIT_SPIN << self.active_threads.load(Acquire)).min(MAX_WAIT_SPIN);
        self.wait_with_spin(n_iters);
    }

    #[inline(always)]
    pub(crate) fn wait_with_spin(&self, iters: u32) {
        for _ in 0..iters {
            spin_loop();
        }
    }
    #[inline(always)]
    pub(crate) fn de_reg(&self) {
        self.active_threads.fetch_sub(1, AcqRel);
    }
}

pub struct RawMpsc<T> {
    next_head: CachePadded<AtomicUsize>,
    tail: CachePadded<AtomicUsize>,
    slots: SlotArr<T>,
}

impl<T: Debug> RawMpsc<T> {
    pub fn new(capacity: usize) -> Self {
        let slots = SlotArr::new(capacity);
        let next_head = CachePadded::new(0.into());
        let tail = CachePadded::new(0.into());
        Self {
            next_head,
            tail,
            slots,
        }
    }
    pub fn push(&self, data: T) -> Result<(), T> {
        let mut curr_head = self.next_head.load(Acquire);
        loop {
            let next_head = curr_head + 1;
            let is_less =
                unsafe { transmute::<isize, usize>(-((next_head < self.slots.capacity) as isize)) };
            let next_head_bounded = next_head & is_less;
            if next_head_bounded != self.tail.load(Acquire) {
                match self
                    .next_head
                    .compare_exchange(curr_head, next_head_bounded, AcqRel, Acquire)
                {
                    Err(h) => curr_head = h,
                    Ok(_) => break,
                }
            } else {
                return Err(data);
            }
        }
        // it shoudnt pannic
        self.slots.set(curr_head, data).unwrap();
        Ok(())
    }
}
