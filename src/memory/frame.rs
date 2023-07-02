//! Physical frame allocator
//!
//! Max physical frame amount is currently hard coded
//!
//!
//!
use crate::consts::device::MAX_PHYSICAL_MEMORY;
use crate::here;

use bitmap_allocator::BitAlloc;

use crate::consts::memlayout;
use crate::consts::{device::PHYMEM_START, PAGE_SIZE};
use crate::sync::SpinNoIrqLock;
use log::*;

use super::address::{kernel_virt_text_to_phys, PhysAddr4K};

// Support 64GiB (?)
pub type FrameAllocator = bitmap_allocator::BitAlloc16M;

pub static FRAME_ALLOCATOR: SpinNoIrqLock<FrameAllocator> =
    SpinNoIrqLock::new(FrameAllocator::DEFAULT);

#[derive(Debug, Clone, Copy)]
pub struct GlobalFrameAlloc;

impl GlobalFrameAlloc {
    fn alloc(&self) -> Option<PhysAddr4K> {
        // get the real address of the alloc frame
        let ret = FRAME_ALLOCATOR
            .lock(here!())
            .alloc()
            .map(|id| id * PAGE_SIZE + unsafe { PHYMEM_START })
            .map(PhysAddr4K::from);
        trace!("Allocate frame: {:x?}", ret);
        ret
    }
    fn alloc_contiguous(&self, size: usize, align_log2: usize) -> Option<PhysAddr4K> {
        // get the real address of the alloc frame
        let ret = FRAME_ALLOCATOR
            .lock(here!())
            .alloc_contiguous(size, align_log2)
            .map(|id| id * PAGE_SIZE + unsafe { PHYMEM_START })
            .map(PhysAddr4K::from);
        trace!("Allocate frame: {:x?}", ret);
        ret
    }
    fn dealloc(&self, target: PhysAddr4K) {
        trace!("Deallocate frame: {:x}", target);
        let target: usize = target.bits();
        FRAME_ALLOCATOR
            .lock(here!())
            .dealloc((target - unsafe { PHYMEM_START }) / PAGE_SIZE);
    }
}

pub fn init() {
    // Insert frames into allocator
    FRAME_ALLOCATOR
        .lock(here!())
        .insert(0..(unsafe { MAX_PHYSICAL_MEMORY } / PAGE_SIZE));
    // Remove kernel occupied
    let kernel_end = memlayout::kernel_end as usize;
    let kernel_end = kernel_virt_text_to_phys(kernel_end);
    let kernel_end = (kernel_end - unsafe { PHYMEM_START }) / PAGE_SIZE;
    FRAME_ALLOCATOR.lock(here!()).remove(0..kernel_end);
}

/// Allocate a frame
/// returns the physical address of the frame, usually 0x80xxxxxx
pub fn alloc_frame() -> Option<PhysAddr4K> {
    GlobalFrameAlloc.alloc()
}
pub fn dealloc_frame(target: PhysAddr4K) {
    GlobalFrameAlloc.dealloc(target);
}
pub fn alloc_frame_contiguous(size: usize, align_log2: usize) -> Option<PhysAddr4K> {
    GlobalFrameAlloc.alloc_contiguous(size, align_log2)
}
pub fn dealloc_frames(target: usize, pages: usize) {
    for i in 0..pages {
        GlobalFrameAlloc.dealloc(PhysAddr4K::from(target + i * PAGE_SIZE));
    }
}

pub fn test_first_frame() {
    let first_frame = alloc_frame().unwrap();
    let kernel_end = memlayout::kernel_end as usize;
    let kernel_end = kernel_virt_text_to_phys(kernel_end);
    assert!(
        first_frame.bits() == kernel_end,
        "first_frame: 0x{:x}, kernel_end: 0x{:x}",
        first_frame,
        kernel_end
    );
    info!("Frame allocator self test passed.");
    info!("First available frame: 0x{:x}", first_frame);
    dealloc_frame(first_frame);
}
