use std::sync::atomic::AtomicUsize;

use crate::cache_padded::CachePadded;

use super::slot_arr::SlotArr;
use std::sync::atomic::Ordering::{Release,Acquire,Relaxed};



pub struct RawMpsc<T>{
    next_head:CachePadded<AtomicUsize>,
    tail:CachePadded<AtomicUsize>,
    head_advance:CachePadded<AtomicUsize>,
    slots:SlotArr<T>
}

impl<T> RawMpsc<T>{
    
    pub fn new(capacity:usize)->Self{
        let slots = SlotArr::new(capacity);
        let next_head = CachePadded::new(0.into());
        let tail = CachePadded::new(0.into());
        let head_advance = CachePadded::new(0.into());
        Self{
            next_head,
            tail,
            head_advance,
            slots
        }
    }

    #[inline(always)]
    fn advance_head(&self)->Result<usize,()>{
        let mut head_advance = self.head_advance.load(Acquire);
        loop {
            let tail = self.tail.load(Acquire);
            let idx = (head_advance + 1)%self.slots.capacity; // TODO MOD(%) optimization 
            if idx != tail {
                match self.head_advance.compare_exchange(head_advance, head_advance + 1, Release, Acquire){
                    Ok(head) => return Ok(head),
                    Err(head) => {
                        head_advance = head;
                        continue;        
                    }
                }
            }else {
                break;
            }
        }
        todo!()
    }

    pub fn push(&self,data:T)->Result<(),T>{
        todo!()
    }

}