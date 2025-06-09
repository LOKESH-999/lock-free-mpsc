use std::{
    alloc::{Layout, alloc, dealloc},
    ptr::NonNull,
    sync::atomic::Ordering::Release,
};

use crate::mpsc::slot::READY;

use super::super::slot::Slot;

pub struct SlotArr<T> {
    pub(super) ptr: NonNull<Slot<T>>,
    pub(super) capacity: usize,
}

impl<T> SlotArr<T> {
    pub fn new(capacity: usize) -> Self {
        let layout = Layout::array::<Slot<T>>(capacity).unwrap();
        let ptr = unsafe { NonNull::new(alloc(layout) as _).unwrap() };
        Self::init_slots(ptr, capacity);
        Self { ptr, capacity }
    }

    fn init_slots(ptr: NonNull<Slot<T>>, capacity: usize) {
        for idx in 0..capacity {
            unsafe {
                (&*ptr.as_ptr().add(idx)).state.store(READY, Release);
            }
        }
    }

    pub fn set(&self, index: usize, data: T) -> Result<(), T> {
        unsafe { (&*self.ptr.as_ptr().add(index)).set(data) }
    }

    pub fn unset(&self, index: usize) -> Result<T, ()> {
        unsafe { (&*self.ptr.as_ptr().add(index)).unset() }
    }
}

impl<T> Drop for SlotArr<T> {
    fn drop(&mut self) {
        let layout = Layout::array::<Slot<T>>(self.capacity).unwrap();
        unsafe {
            dealloc(self.ptr.as_ptr() as _, layout);
        }
    }
}
