use buddy_system_allocator::LockedHeap;

// 16 MiB kernel init heap
// The frame reference count is 4 byte per frame.
// Given an 8 GiB physis memory & 4k page size,
// the frame reference count will cost 8 MiB memory.
// So we need larger than 8 MiB heap.
const KERNEL_HEAP_SIZE: usize = 16 * 1024 * 1024;

#[global_allocator]
static HEAP_ALLOCATOR: LockedHeap<32> = LockedHeap::<32>::empty();

static mut HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

pub fn init() {
    unsafe {
        HEAP_ALLOCATOR.lock().init(HEAP_SPACE.as_ptr() as usize, KERNEL_HEAP_SIZE);
    }
}

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error, layout = {:?}", layout);
}
