//! A global exponential backoff strategy for managing contention in lock-free data structures.
//!
//! The [`GlobalBackoff`] struct helps reduce contention between threads attempting to access
//! shared resources in concurrent environments, such as lock-free MPSC queues. It introduces
//! configurable delays (`spin_loop`s) when contention is detected, allowing threads to back off
//! and reduce CPU cache thrashing or spinning overhead.

use std::hint::spin_loop;
use std::sync::atomic::{
    AtomicUsize,
    Ordering::{AcqRel, Acquire},
};

const MAX_WAIT_SPIN: u32 = 1 << 18;
const MIN_WAIT_SPIN: u32 = 32;

/// A lock-free backoff mechanism used globally across threads to coordinate retries.
///
/// `GlobalBackoff` provides cooperative spinning by maintaining a shared counter of active
/// participants. When a thread fails to make progress (e.g., a CAS failure), it calls
/// [`wait`](GlobalBackoff::wait) to yield some CPU time via `spin_loop()`, scaled based on how many
/// threads are currently contending. This reduces unnecessary contention and improves throughput
/// under load.

#[repr(transparent)]
pub struct GlobalBackoff {
    /// Number of threads currently contending.
    active_threads: AtomicUsize,
}

impl GlobalBackoff {
    /// Creates a new `GlobalBackoff` instance with no registered threads.
    ///
    /// # Examples
    ///
    /// ```
    /// use lock_free_mpsc::backoff::GlobalBackoff;
    ///
    /// let backoff = GlobalBackoff::new();
    /// ```
    #[inline]
    pub const fn new() -> Self {
        Self {
            active_threads: AtomicUsize::new(0),
        }
    }

    /// Registers the calling thread for backoff and immediately performs a backoff wait.
    ///
    /// Increments the count of contending threads and waits using `spin_loop` proportional
    /// to the number of currently active threads.
    ///
    /// Should typically be called once before entering a contention-sensitive region.
    
    #[inline(always)]
    pub fn reg_wait(&self) {
        let n_iters = self.active_threads.fetch_add(1, AcqRel);
        self.spin_for(n_iters as u32);
    }

    /// Deregisters the calling thread from contention tracking.
    ///
    /// Decrements the count of active contending threads. Should be called once a thread
    /// exits a contention-sensitive operation.
    #[inline(always)]
    pub fn de_reg(&self) {
        self.active_threads.fetch_sub(1, AcqRel);
    }

    /// Performs a backoff wait proportional to the current contention level.
    ///
    /// This method reads the number of active contending threads and computes an
    /// exponential spin count bounded by `MIN_WAIT_SPIN` and `MAX_WAIT_SPIN`.
    ///
    /// Can be called within retry loops to yield control between CAS failures or
    /// other contention failures.
    #[inline(always)]
    pub fn wait(&self) {
        let n_iters = (MIN_WAIT_SPIN << self.active_threads.load(Acquire)).min(MAX_WAIT_SPIN);
        self.spin_for(n_iters);
    }

    /// Performs an exact number of spin iterations using `std::hint::spin_loop()`.
    ///
    /// Used internally by [`reg_wait`](Self::reg_wait) and [`wait`](Self::wait).
    ///
    /// # Safety
    /// This method does not yield to the OS scheduler; it performs pure CPU busy-waiting.
    #[inline(always)]
    pub fn spin_for(&self, iters: u32) {
        for _ in 0..iters {
            spin_loop();
        }
    }
}
