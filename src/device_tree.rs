use alloc::boxed::Box;
use fdt::Fdt;

use crate::{
    boot,
    consts::{
        self,
        address_space::{self, K_SEG_DTB},
        platform,
    },
    driver,
    memory::{self, kernel_phys_dev_to_virt, pagetable::pte::PTEFlags},
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
    unsafe {
        consts::device::PHYMEM_START = phy_mem.starting_address as usize;
        consts::device::MAX_PHYSICAL_MEMORY = phy_mem.size.unwrap() as usize;
    }
    device_tree
}

pub fn device_init() {
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

    // Init serial console
    unsafe {
        consts::device::UART0_BASE =
            stdout.reg().unwrap().into_iter().next().unwrap().starting_address as usize
    };

    // Map devices
    let mut kernel_page_table = memory::pagetable::pagetable::PageTable::new_with_paddr(
        (boot::boot_pagetable_paddr()).into(),
    );
    kernel_page_table.map_page(
        (kernel_phys_dev_to_virt(unsafe { consts::device::UART0_BASE })).into(),
        unsafe { consts::device::UART0_BASE.into() },
        PTEFlags::R | PTEFlags::W | PTEFlags::A | PTEFlags::D,
    );

    for reg in platform::VIRTIO_MMIO_REGIONS {
        kernel_page_table.map_region(
            kernel_phys_dev_to_virt(reg.0).into(),
            reg.0.into(),
            reg.1,
            PTEFlags::R | PTEFlags::W | PTEFlags::A | PTEFlags::D,
        );
    }
    // Avoid drop
    core::mem::forget(kernel_page_table);

    // Init device
    init_serial_console(&stdout);
}

fn init_serial_console(stdout: &fdt::node::FdtNode) {
    match stdout.compatible().unwrap().first() {
        "ns16550a" | "snps,dw-apb-uart" => {
            todo!()
        }
        "sifive,uart0" => {
            // sifive_u QEMU (FU540)
            // VisionFive 2 (FU740)

            let paddr = stdout.reg().unwrap().into_iter().next().unwrap().starting_address as usize;
            let vaddr = kernel_phys_dev_to_virt(paddr);

            let uart = driver::SifiveUart::new(
                vaddr,
                500 * 1000 * 1000, // 500 MHz hard coded for now
            );
            unsafe { *crate::UART0.lock(here!()) = Some(Box::new(uart)) }
        }
        _ => panic!("Unsupported serial console"),
    }
}
