pub mod memlayout;

pub const PAGE_SIZE_BITS: usize = 12;

pub const PAGE_SIZE: usize = 1usize << PAGE_SIZE_BITS;

pub const PAGE_MASK: usize = PAGE_SIZE - 1;

pub const MAX_PHYSICAL_MEMORY: usize = 1024 * 1024 * 1024; // use 1G for now

pub const MAX_PHYSICAL_FRAMES: usize = MAX_PHYSICAL_MEMORY / PAGE_SIZE;

pub const PA_WIDTH_SV39: usize = 56;

pub const PPN_WIDTH_SV39: usize = PA_WIDTH_SV39 - PAGE_SIZE_BITS;

pub const PPN_MASK_SV39: usize = ((1usize << 54) - 1) & !PAGE_MASK;
