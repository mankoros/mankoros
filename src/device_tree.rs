use fdt::Fdt;

use crate::consts::{self, address_space::K_SEG_DTB};

pub fn parse_device_tree() -> Fdt<'static> {
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    // Uart
    let uart = device_tree
        .find_compatible(&[
            "ns16550a",
            "snps,dw-apb-uart", // C910
            "sifive,uart0",     // sifive_u QEMU (FU540)
        ])
        .expect("No compatible serial console"); // Must be one
    unsafe {
        consts::device::UART0_BASE =
            uart.reg().unwrap().into_iter().next().unwrap().starting_address as usize
    };

    // Memory
    let phy_mem = device_tree.memory().regions().next().expect("No memory region found");
    unsafe {
        consts::device::PHYMEM_START = phy_mem.starting_address as usize;
        consts::device::MAX_PHYSICAL_MEMORY = phy_mem.size.unwrap() as usize;
    }
    device_tree
}
