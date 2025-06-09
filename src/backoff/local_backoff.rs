use std::{cell::Cell, hint::spin_loop};

/// A thread-local exponential backoff strategy for reducing contention.
///
/// `LocalBackoff` is useful in scenarios involving tight spin loops, such as in lock-free
/// data structures, where threads compete for the same resource. It exponentially increases
/// the number of CPU spin iterations each time `wait` is called, which helps reduce
/// contention and CPU usage under heavy load.
///
/// # Example
/// ```
/// let backoff = LocalBackoff::new();
/// loop {
///     if try_acquire_lock() {
///         break;
///     }
///     backoff.wait();
/// }
/// ```
pub struct LocalBackoff {
    /// Tracks the current number of spin iterations for this thread.
    spins: Cell<u32>,
}

/// Maximum number of spin iterations allowed during backoff.
///
/// This is used to cap the exponential growth in spin iterations.
const MAX_SPIN: u32 = 1 << 18;

impl LocalBackoff {
    /// Creates a new `LocalBackoff` instance with an initial spin count of 0.
    ///
    /// Typically used in a local scope for retry-based synchronization primitives.
    pub fn new() -> Self {
        Self {
            spins: Cell::new(0),
        }
    }

    /// Performs a backoff by spinning the CPU in a tight loop.
    ///
    /// The number of iterations doubles with each call (up to `MAX_SPIN`), allowing
    /// contention to decrease before the next retry attempt. Internally, it uses
    /// [`std::hint::spin_loop`] to inform the CPU that it is in a spin-wait loop.
    pub fn wait(&self) {
        let curr_spin = self.spins.get();
        // Exponential backoff: increase spin count for next wait
        {
            let new_spin = (curr_spin << 1).min(MAX_SPIN);
            self.spins.set(new_spin);
        }

        // Perform the actual spin
        for _ in 0..curr_spin {
            spin_loop();
        }
    }
}
