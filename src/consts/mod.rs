pub mod memlayout;

pub const PAGE_SIZE: usize = 1usize << 12;

pub const MAX_PHYSICAL_MEMORY: usize = 1024 * 1024 * 1024; // use 1G for now

pub const MAX_PHYSICAL_FRAMES: usize = MAX_PHYSICAL_MEMORY / PAGE_SIZE;
