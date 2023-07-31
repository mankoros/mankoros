use buddy_system_allocator::{Heap, LockedHeapWithRescue};
use log::warn;

use crate::{
    boot,
    consts::{self, address_space::K_SEG_VIRT_MEM_BEG, PAGE_MASK},
};

use super::{
    frame::alloc_frame_contiguous,
    pagetable::{self, pte::PTEFlags},
};

/// 2 MiB kernel init heap
/// Auto expand when needed
const KERNEL_HEAP_SIZE: usize = 2 * 1024 * 1024;

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeapWithRescue<32> =
    LockedHeapWithRescue::<32>::new(heap_allocate_rescue);

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

static mut KERNEL_HEAP_TOP: usize = K_SEG_VIRT_MEM_BEG;

pub fn init() {
    unsafe {
        HEAP_ALLOCATOR.lock().init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
    }
}

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:x?}", layout);
}

fn heap_allocate_rescue(heap: &mut Heap<32>, layout: &core::alloc::Layout) {
    warn!("Heap expanding, layout = {:x?}", layout);
    let mut root_pagetable = pagetable::pagetable::PageTable::new_with_paddr_no_heap_alloc(
        boot::boot_pagetable_paddr().into(),
    );

    let allocate_size = layout.size() + 2 * 1024 * 1024; // Speculatively allocate 2 MiB more.

    let page_cnt = (allocate_size + consts::PAGE_SIZE - 1) / consts::PAGE_SIZE;
    let paddr = alloc_frame_contiguous(
        page_cnt,
        layout.align().checked_ilog2().expect("alignment is not power of 2") as _,
    )
    .expect("Heap expansion failed, cannot allocate frame from physical frame allocator");
    let aligned_heap_top =
        (unsafe { KERNEL_HEAP_TOP } + layout.align() - 1) & !(layout.align() - 1) & !PAGE_MASK;

    root_pagetable.map_region(
        aligned_heap_top.into(),
        paddr,
        allocate_size,
        PTEFlags::rw(),
    );
    unsafe {
        // Add the newly allocated frame to the heap
        heap.add_to_heap(aligned_heap_top, aligned_heap_top + allocate_size);
        KERNEL_HEAP_TOP = aligned_heap_top + allocate_size;
    }
    core::mem::forget(root_pagetable);
    warn!(
        "Heap expansion success, aligned_heap_top = {:#x}, paddr = {:#x}, allocate_size = 0x{:x}",
        unsafe { KERNEL_HEAP_TOP },
        paddr,
        allocate_size
    );
    warn!("Current heap: {:x?}", heap);
}
