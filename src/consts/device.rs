use super::const_register::{register_const, register_mut_const};

// DTB parser will modify this
register_mut_const!(pub UART0_BASE, usize, 0xdead_beef);

register_mut_const!(PHYMEM_START, usize, 0);

register_mut_const!(MAX_PHYSICAL_MEMORY, usize, 0);

register_mut_const!(pub PLATFORM_BOOT_PC, usize, 0);

register_const!(DEVICE_START, usize, 0xc00_0000);
