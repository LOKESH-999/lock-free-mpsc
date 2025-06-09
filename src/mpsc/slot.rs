use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
use std::sync::atomic::fence;
use std::sync::atomic::{
    AtomicU8,
    Ordering::{AcqRel, Acquire, Relaxed, Release},
};

/// A slot in a concurrent queue or stack, representing a cell that can store a value of type `T`.
///
/// The `Slot` manages the lifecycle of a single value with a state flag indicating whether
/// the slot is ready, reserved, or registered (occupied). It is designed for use in lock-free
/// multiple-producer, single-consumer (MPSC) and similar concurrent data structures.
///
/// # Atomic state Flags
///
/// - `READY` (0): The slot is empty and ready to be written.
/// - `RESERVED` (1): The slot is reserved for writing.
/// - `REGISTERED` (2): The slot contains a value and is occupied.
#[repr(Rust)]
pub struct Slot<T> {
    /// The storage for the value in the slot. Access is controlled via `UnsafeCell` and `MaybeUninit`.
    pub(crate) value: UnsafeCell<MaybeUninit<T>>,
    /// The atomic state of the slot.
    pub(crate) state: AtomicU8,
}

impl<T> Slot<T> {
    /// Attempts to write a value into the slot.
    ///
    /// Transitions the slot from `READY` to `REGISTERED`. If successful, writes `data` into the slot
    /// and returns `Ok(())`. If the slot was not `READY`, returns `Err(data)`.
    ///
    /// # Arguments
    ///
    /// * `data` - The value to write into the slot.
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the value was successfully written.
    /// * `Err(data)` if the slot was reserved/registered.
    ///
    pub fn set(&self, data: T) -> Result<(), T> {
        if self
            .state
            .compare_exchange(READY, RESERVED, AcqRel, Relaxed)
            .is_ok()
        {
            unsafe { self.unchecked_set(data) };
            self.state.store(REGISTERED, Release);
            Ok(())
        } else {
            Err(data)
        }
    }

    /// Attempts to remove and return the value from the slot.
    ///
    /// Transitions the slot from `REGISTERED` to `READY`. If successful, returns `Ok(value)`.
    /// If the slot was not `REGISTERED`, returns `Err(())`.
    ///
    /// # Returns
    ///
    /// * `Ok(value)` if the value was successfully removed.
    /// * `Err(())` if the slot was not registered.
    ///
    pub fn unset(&self) -> Result<T, ()> {
        if self
            .state
            .compare_exchange(REGISTERED, READY, AcqRel, Relaxed)
            .is_ok()
        {
            Ok(unsafe { self.unchecked_unset() })
        } else {
            Err(())
        }
    }

    /// Writes a value into the slot without checking or updating the state.
    ///
    /// # Safety
    ///
    /// This function bypasses synchronization and state checks. It must only be used
    /// when the caller has exclusive access to the slot and the slot's state is known.
    #[inline(always)]
    pub unsafe fn unchecked_set(&self, data: T) {
        unsafe { (&mut *self.value.get()).write(data) };
        fence(Release);
    }

    /// Reads and removes the value from the slot without checking or updating the state.
    ///
    /// # Safety
    ///
    /// This function bypasses synchronization and state checks. The caller must ensure that
    /// the slot actually contains a value and that the state is valid.
    #[inline(always)]
    pub unsafe fn unchecked_unset(&self) -> T {
        fence(Acquire);
        unsafe { (&*self.value.get()).assume_init_read() }
    }
}

// Atomic state constants
pub(super) const READY: u8 = 0; // Slot is empty
const RESERVED: u8 = 1; // Slot is reserved for writing
const REGISTERED: u8 = 2; // Slot contains data

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_set_and_unset_success() {
        let slot = Slot {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(READY),
        };

        // Attempt to set a value
        assert!(slot.set(42).is_ok());

        // Attempt to unset and get the value
        let val = slot.unset().expect("Expected to unset a value");
        assert_eq!(val, 42);

        // After unset, slot should be READY
        assert_eq!(slot.state.load(Relaxed), READY);
    }

    #[test]
    fn test_set_failure_when_not_ready() {
        let slot = Slot {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(RESERVED),
        };

        // Attempt to set should fail because slot is RESERVED
        let result = slot.set(100);
        assert!(result.is_err());
        assert_eq!(result.err(), Some(100));
    }

    #[test]
    fn test_unset_failure_when_not_registered() {
        let slot = Slot {
            value: UnsafeCell::new(MaybeUninit::<i32>::uninit()),
            state: AtomicU8::new(RESERVED), // Not REGISTERED yet
        };

        // Attempt to unset should fail
        let result = slot.unset();
        assert!(result.is_err());
    }

    #[test]
    fn test_unchecked_set_and_unset() {
        let slot = Slot {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(RESERVED), // Skip READY check
        };

        unsafe {
            slot.unchecked_set(77);
            let val = slot.unchecked_unset();
            assert_eq!(val, 77);
        }
    }
}
