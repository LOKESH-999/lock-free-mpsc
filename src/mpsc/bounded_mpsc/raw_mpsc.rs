use std::{fmt::Debug, hint::spin_loop, mem::transmute, sync::atomic::{AtomicU32, AtomicUsize}};

use crate::cache_padded::CachePadded;

use super::slot_arr::SlotArr;
use std::sync::atomic::Ordering::{Release,Acquire,AcqRel};


pub struct RawMpsc<T>{
    next_head:CachePadded<AtomicUsize>,
    tail:CachePadded<AtomicUsize>,
    slots:SlotArr<T>
}

impl<T:Debug> RawMpsc<T>{
    
    pub fn new(capacity:usize)->Self{
        let slots = SlotArr::new(capacity);
        let next_head = CachePadded::new(0.into());
        let tail = CachePadded::new(0.into());
        Self{
            next_head,
            tail,
            slots
        }
    }
    pub fn push(&self,data:T)->Result<(),T>{
        let mut curr_head = self.next_head.load(Acquire);
        loop {
            let next_head = curr_head + 1 ;
            let is_less = unsafe { transmute::<isize,usize>(-((next_head < self.slots.capacity) as isize)) };
            let next_head_bounded = next_head & is_less;
            if next_head_bounded != self.tail.load(Acquire){
                match self.next_head.compare_exchange(curr_head, next_head_bounded, AcqRel, Acquire){
                    Err(h) =>curr_head = h,
                    Ok(_)=>break,
                }
            }else {
                return Err(data);
            }
        }
        // it shoudnt pannic
        self.slots.set(curr_head, data).unwrap();
        Ok(())
    }

}