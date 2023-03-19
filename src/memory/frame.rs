//! Physical frame allocator
//!
//! Max physical frame amount is currently hard coded
//!
//!
//!
use bitmap_allocator::BitAlloc;

use crate::consts::memlayout;
use crate::consts::{MAX_PHYSICAL_FRAMES, PAGE_SIZE};
use crate::sync::SpinLock;
use log::*;

// Support 64GiB (?)
pub type FrameAllocator = bitmap_allocator::BitAlloc16M;

pub static FRAME_ALLOCATOR: SpinLock<FrameAllocator> = SpinLock::new(FrameAllocator::DEFAULT);

#[derive(Debug, Clone, Copy)]
pub struct GlobalFrameAlloc;

impl GlobalFrameAlloc {
    fn alloc(&self) -> Option<usize> {
        // get the real address of the alloc frame
        let ret = FRAME_ALLOCATOR
            .lock()
            .alloc()
            .map(|id| id * PAGE_SIZE + memlayout::PHYMEM_START);
        trace!("Allocate frame: {:x?}", ret);
        ret
    }
    fn alloc_contiguous(&self, size: usize, align_log2: usize) -> Option<usize> {
        // get the real address of the alloc frame
        let ret = FRAME_ALLOCATOR
            .lock()
            .alloc_contiguous(size, align_log2)
            .map(|id| id * PAGE_SIZE + memlayout::PHYMEM_START);
        trace!("Allocate frame: {:x?}", ret);
        ret
    }
    fn dealloc(&self, target: usize) {
        trace!("Deallocate frame: {:x}", target);
        FRAME_ALLOCATOR.lock().dealloc((target - memlayout::PHYMEM_START) / PAGE_SIZE);
    }
}

pub fn init() {
    // Insert frames into allocator
    FRAME_ALLOCATOR.lock().insert(0..MAX_PHYSICAL_FRAMES);
    // Remove kernel occupied
    let kernel_end = unsafe { memlayout::kernel_end as usize };
    let kernel_end = (kernel_end - memlayout::PHYMEM_START) / PAGE_SIZE;
    FRAME_ALLOCATOR.lock().remove(0..kernel_end);
}

pub fn alloc_frame() -> Option<usize> {
    GlobalFrameAlloc.alloc()
}
pub fn dealloc_frame(target: usize) {
    GlobalFrameAlloc.dealloc(target);
}
pub fn alloc_frame_contiguous(size: usize, align_log2: usize) -> Option<usize> {
    GlobalFrameAlloc.alloc_contiguous(size, align_log2)
}
