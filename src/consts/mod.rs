pub mod address_space;
pub mod device;
pub mod memlayout;
pub mod time;

mod const_register;

use const_register::register_const;

register_const!(KERNEL_LINK_ADDR, usize, address_space::K_SEG_DATA_BEG);

register_const!(PAGE_SIZE_BITS, usize, 12);

register_const!(PAGE_SIZE, usize, 1usize << PAGE_SIZE_BITS);

register_const!(HUGE_PAGE_SIZE, usize, 1usize << 30); // 1GiB huge page, hard coded, TODO

register_const!(PAGE_MASK, usize, PAGE_SIZE - 1);

register_const!(VA_WIDTH_SV39, usize, 39);

register_const!(PA_WIDTH_SV39, usize, 56);

register_const!(PPN_WIDTH_SV39, usize, PA_WIDTH_SV39 - PAGE_SIZE_BITS);

register_const!(
    PADDR_PPN_MASK_SV39,
    usize,
    ((1usize << 56) - 1) & !PAGE_MASK
);

register_const!(PTE_FLAGS_BITS, usize, 10);

register_const!(PTE_FLAGS_MASK, usize, (1usize << PTE_FLAGS_BITS) - 1);

register_const!(MAX_OPEN_FILES, usize, 512);

register_const!(
    PTE_PPN_MASK_SV39,
    usize,
    ((1usize << 54) - 1) & !PTE_FLAGS_MASK
);

register_const!(MAX_SUPPORTED_CPUS, usize, 32);

register_const!(MAX_PIPE_SIZE, usize, 4 * 1024); // use 4k for now
