pub mod address_space;
pub mod memlayout;
pub mod platform;

pub const PAGE_SIZE_BITS: usize = 12;

pub const PAGE_SIZE: usize = 1usize << PAGE_SIZE_BITS;

pub const HUGE_PAGE_SIZE: usize = 1usize << 30; // 1GiB huge page, hard coded, TODO

pub const PAGE_MASK: usize = PAGE_SIZE - 1;

pub const PHYMEM_START: usize = 0x8000_0000;

pub const MAX_PHYSICAL_MEMORY: usize = 4 * 1024 * 1024 * 1024; // use 4G for now

pub const MAX_PHYSICAL_FRAMES: usize = MAX_PHYSICAL_MEMORY / PAGE_SIZE;

pub const VA_WIDTH_SV39: usize = 39;

pub const PA_WIDTH_SV39: usize = 56;

pub const PPN_WIDTH_SV39: usize = PA_WIDTH_SV39 - PAGE_SIZE_BITS;

pub const PADDR_PPN_MASK_SV39: usize = ((1usize << 56) - 1) & !PAGE_MASK;

pub const PTE_FLAGS_BITS: usize = 10;

pub const PTE_FLAGS_MASK: usize = (1usize << PTE_FLAGS_BITS) - 1;

pub const PTE_PPN_MASK_SV39: usize = ((1usize << 54) - 1) & !PTE_FLAGS_MASK;

pub const MAX_SUPPORTED_CPUS: usize = 32;
