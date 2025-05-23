use std::sync::atomic::{Ordering::{Acquire,Release,AcqRel}, AtomicU8};

#[repr(u8)]
#[derive(Clone, Copy)]
pub enum SlotStatus {
    /// The slot is empty.
    Empty = 0,
    /// The slot is full.
    Full = 1,
    /// The slot is being processed.
    Processing = 2,
}

impl Into<SlotStatus> for u8 {
    fn into(self) -> SlotStatus {
        SLOT_STATUS_CONVERTER[self as usize]
    }
}

const SLOT_STATUS_CONVERTER: [SlotStatus; 3] =
    [SlotStatus::Empty, SlotStatus::Full, SlotStatus::Processing];

impl Into<u8> for SlotStatus {
    fn into(self) -> u8 {
        match self {
            SlotStatus::Empty => 0,
            SlotStatus::Full => 1,
            SlotStatus::Processing => 2,
        }
    }
}

#[repr(Rust)]
pub struct Slot<T> {
    /// The value stored in the slot.
    pub(crate) value: T,
    /// The status of the slot, indicating whether it is occupied or not.
    pub(crate) status: AtomicU8,
}

impl <T> Slot<T> {
    /// Creates a new slot with the given value and status.
    pub(crate) fn new(value: T, status: SlotStatus) -> Self {
        Slot {
            value,
            status: AtomicU8::new(status.into()),
        }
    }
    /// Returns the status of the slot.
    pub(crate) fn get_status(&self) -> SlotStatus {
        self.status.load(Acquire).into()
    }
    /// Sets the status of the slot.
    pub(crate) unsafe fn set_status(&self, status: SlotStatus) {
        self.status.store(status.into(), Release);
    }

    pub(crate) unsafe fn cas_status(&self, old: SlotStatus, new: SlotStatus) -> Result<u8, u8> {
        self.status
            .compare_exchange(old.into(), new.into(), AcqRel, Acquire)
    }

}