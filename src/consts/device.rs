pub static mut UART0_BASE: usize = 0xdead_beef; // DTB parser will modify this

pub static mut PHYMEM_START: usize = 0;

pub static mut MAX_PHYSICAL_MEMORY: usize = 0;

pub static mut PLATFORM_BOOT_PC: usize = 0;

pub const DEVICE_START: usize = 0xc00_0000;
