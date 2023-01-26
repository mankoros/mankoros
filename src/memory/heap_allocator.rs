use buddy_system_allocator::LockedHeap;

// 128 KiB heap, with max ORDER as 32, meaning block size is 128KiB / 2^32 == 0
// TODO: find out why it will happened
const KERNEL_HEAP_SIZE: usize = 128 * 1024;
const KERNEL_HEAP_ORDER: usize = 32;

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<KERNEL_HEAP_ORDER> = LockedHeap::<KERNEL_HEAP_ORDER>::empty();

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init_heap() {
    unsafe {
        HEAP_ALLOCATOR.lock().init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
    }
}

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}
