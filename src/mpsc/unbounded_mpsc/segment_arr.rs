use crate::{
    cache_padded::CachePadded,
    mpsc::slot::{READY, Slot},
};
use std::{alloc::dealloc, cell::Cell, ptr::null_mut, sync::atomic::Ordering::Release};
use std::{
    alloc::{Layout, alloc},
    ptr::NonNull,
    sync::atomic::AtomicUsize,
};

pub(crate) const SEGMENT_SIZE: usize = 128;

pub struct Segment<T> {
    pub(crate) next_head: CachePadded<AtomicUsize>,
    pub(crate) tail: CachePadded<AtomicUsize>,
    pub(crate) buff: NonNull<Slot<T>>,
    pub(crate) next: Cell<*mut Segment<T>>
}

impl<T> Segment<T> {
    pub fn new() -> Self {
        let layout = Self::layout();
        let buff = NonNull::new(unsafe { alloc(layout) } as *mut _).unwrap();
        let ptr: *mut Slot<T> = buff.as_ptr();
        for idx in 0..SEGMENT_SIZE {
            let slot = unsafe { &*ptr.add(idx) };
            slot.state.store(READY, Release);
        }
        let next_head = CachePadded::new(AtomicUsize::new(0));
        let tail = CachePadded::new(AtomicUsize::new(0));
        let next = Cell::new(null_mut());
        Self {
            next_head,
            tail,
            buff,
            next
        }
    }

    const fn layout() -> Layout {
        if let Ok(layout) = Layout::array::<Slot<T>>(SEGMENT_SIZE) {
            layout
        } else {
            panic!("Invalid layout for Segment")
        }
    }

    #[inline]
    pub fn set(&self, index: usize, data: T) -> Result<(), T> {
        let ptr = self.buff.as_ptr();
        let slot = unsafe { &*ptr.add(index) };
        slot.set(data)
    }

    #[inline]
    pub fn unset(&self, index: usize) -> Option<T> {
        let ptr = self.buff.as_ptr();
        let slot = unsafe { &*ptr.add(index) };
        slot.unset().ok()
    }

    #[inline]
    pub unsafe fn set_unchecked(&self, index: usize, data: T) {
        let ptr = self.buff.as_ptr();
        let slot = unsafe { &*ptr.add(index) };
        unsafe { slot.unchecked_set(data) };
    }

    #[inline]
    pub unsafe fn unset_unchecked(&self, index: usize) -> T {
        let ptr = self.buff.as_ptr();
        let slot = unsafe { &*ptr.add(index) };
        let data = unsafe { slot.unchecked_unset() };
        data
    }
}


impl<T> Drop for Segment<T>{
    fn drop(&mut self) {
        let layout = Self::layout();
        let ptr = self.buff.as_ptr();
        unsafe { dealloc(ptr as _, layout) };
    }
}