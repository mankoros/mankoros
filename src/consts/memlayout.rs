//! Memory layout for the system.
//!
//!

use log::info;

extern "C" {
    pub fn kernel_start();
    pub fn text_start();
    pub fn rodata_start();
    pub fn data_start();
    pub fn init_kstack_start();
    pub fn bss_start();
    pub fn kernel_end();
}

pub const PHYMEM_START: usize = 0x8000_0000;

pub fn print_memlayout() {
    let kernel_start = kernel_start as usize;
    let text_start = text_start as usize;
    let rodata_start = rodata_start as usize;
    let data_start = data_start as usize;
    let init_kstack_start = init_kstack_start as usize;
    let bss_start = bss_start as usize;
    let kernel_end = kernel_end as usize;

    let kernel_size = humansize::SizeFormatter::new(kernel_end - kernel_start, humansize::BINARY);
    // Print them
    info!("Physical memory layout:");
    info!("");
    info!("{:20} 0x{:x}", "kernel_start:", kernel_start);
    info!("{:20} 0x{:x}", "text_start:", text_start);
    info!("{:20} 0x{:x}", "rodata_start:", rodata_start);
    info!("{:20} 0x{:x}", "data_start:", data_start);
    info!("{:20} 0x{:x}", "init_kstack_start:", init_kstack_start);
    info!("{:20} 0x{:x}", "bss_start:", bss_start);
    info!("{:20} 0x{:x}", "kernel_end:", kernel_end);
    info!("");
    info!("{:20} {}", "Total kernel size:", kernel_size);
    info!("");
}
