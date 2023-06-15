use fdt::Fdt;

use crate::{
    consts::{self, address_space::K_SEG_DTB},
    println,
};

pub fn parse_device_tree() -> Fdt<'static> {
    println!("Start parsing device tree");
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    let chosen = device_tree.chosen();
    if let Some(bootargs) = chosen.bootargs() {
        println!("Bootargs: {:?}", bootargs);
    }

    println!("Device: {}", device_tree.root().model());

    // Serial
    let mut stdout = chosen.stdout();
    if stdout.is_none() {
        println!("Non-standard stdout device, trying to workaround");
        let chosen = device_tree.find_node("/chosen").expect("No chosen node");
        let stdout_path = chosen
            .properties()
            .find(|n| n.name == "stdout-path")
            .and_then(|n| {
                let bytes = unsafe {
                    core::slice::from_raw_parts_mut((n.value.as_ptr()) as *mut u8, n.value.len())
                };
                let mut len = 0;
                for byte in bytes.iter() {
                    if *byte == b':' {
                        return core::str::from_utf8(&n.value[..len]).ok();
                    }
                    len += 1;
                }
                core::str::from_utf8(&n.value[..n.value.len() - 1]).ok()
            })
            .unwrap();
        println!("Searching stdout: {}", stdout_path);
        stdout = device_tree.find_node(stdout_path);
    }
    if stdout.is_none() {
        println!("Unable to parse /chosen, choosing first serial device");
        stdout = device_tree.find_compatible(&[
            "ns16550a",
            "snps,dw-apb-uart", // C910, VF2
            "sifive,uart0",     // sifive_u QEMU (FU540)
        ])
    }
    let stdout = stdout.expect("Still unable to get stdout device");
    println!("Stdout: {}", stdout.name);
    unsafe {
        consts::device::UART0_BASE =
            stdout.reg().unwrap().into_iter().next().unwrap().starting_address as usize
    };

    // Memory
    let phy_mem = device_tree.memory().regions().next().expect("No memory region found");
    unsafe {
        consts::device::PHYMEM_START = phy_mem.starting_address as usize;
        consts::device::MAX_PHYSICAL_MEMORY = phy_mem.size.unwrap() as usize;
    }
    device_tree
}
