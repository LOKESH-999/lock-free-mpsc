use std::hint::spin_loop;
use std::sync::atomic::{
    AtomicUsize,
    Ordering::{AcqRel, Acquire},
};

const MAX_WAIT_SPIN: u32 = 1 << 18;
const MIN_WAIT_SPIN: u32 = 32;

#[repr(transparent)]
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
