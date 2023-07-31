use fdt::Fdt;

use crate::{
    consts::{self, address_space::K_SEG_DTB},
    println,
};

/// early_parse_device_tree
/// No heap allocator
/// Parse memory information from device tree
pub fn early_parse_device_tree() -> Fdt<'static> {
    println!("Early parsing device tree");
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    // Memory
    let phy_mem = device_tree.memory().regions().next().expect("No memory region found");
    consts::platform::set_phymem_start(phy_mem.starting_address as usize);
    consts::platform::set_max_physical_memory(phy_mem.size.unwrap());
    device_tree
}

pub fn device_init() {
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    // Init timer frequency
    consts::time::set_clock_freq(device_tree.cpus().next().unwrap().timebase_frequency());
}
