use super::slot::{Slot, SlotStatus};
use crate::cache_padded::CachePadded;
use std::alloc::{Layout, alloc, dealloc};
use std::mem::MaybeUninit;
use std::{
    ptr::NonNull,
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct Array<T> {
    pub(crate) slots: NonNull<MaybeUninit<Slot<T>>>,
    pub(crate) capacity: usize,
}
impl<T> Array<T> {
    pub fn new(capacity: usize) -> Self {
        let layout = Layout::array::<MaybeUninit<Slot<T>>>(capacity).unwrap();
        let slots = NonNull::new(unsafe { alloc(layout) as *mut MaybeUninit<Slot<T>> })
            .expect("Failed to allocate memory for slots");
        Array { slots, capacity }
    }

    pub unsafe fn unsafe_get(&self, idx:usize) -> Slot<T>{
        unsafe {
            let slot_ptr = self.slots.as_ptr().add(idx);
            let ret = (*slot_ptr).assume_init_read();
            (*slot_ptr).assume_init_ref().set_status(SlotStatus::Empty);
            ret
        }
    }

    pub unsafe fn unsafe_set(&self, idx: usize, slot: Slot<T>) {
        unsafe {
            let slot_ptr = self.slots.as_ptr().add(idx);
            (*slot_ptr).assume_init_ref().set_status(SlotStatus::Full);
            (*slot_ptr).write(slot);
        }
    }

    pub fn set(&self, idx: usize, slot: Slot<T>)->Result<(), Slot<T>> {
        unsafe {
            let slot_ptr = self.slots.as_ptr().add(idx);
            match (*slot_ptr).assume_init_ref().cas_status(SlotStatus::Empty,SlotStatus::Full){
                Ok(_) => {
                    (*slot_ptr).assume_init_ref().set_status(SlotStatus::Full);
                    (*slot_ptr).write(slot);
                    Ok(())
                }
                Err(_) => {
                    Err(slot)
                }
            }
        }
    }
}

impl<T> Drop for Array<T> {
    fn drop(&mut self) {
        let layout = Layout::array::<MaybeUninit<Slot<T>>>(self.capacity).unwrap();
        unsafe { dealloc(self.slots.as_ptr() as *mut u8, layout) };
    }
}

pub struct RawMpsc<T> {
    pub(crate) head: CachePadded<AtomicUsize>,
    pub(crate) tail: CachePadded<AtomicUsize>,
    pub(crate) head_advance: CachePadded<AtomicUsize>,
    pub(crate) array: Array<T>,
}
