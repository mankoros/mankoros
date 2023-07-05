//! CPU is also a device
//!

use alloc::vec::Vec;
use log::info;

use crate::consts::address_space::K_SEG_DTB;

pub struct CPU {
    pub id: usize,
    pub usable: bool, // is the CPU usable? we need MMU
    pub clock_freq: usize,
    pub timebase_freq: usize,
}

pub fn probe() -> Vec<CPU> {
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    let dtb_cpus = device_tree.cpus();
    for prop in device_tree.find_node("/cpus").unwrap().properties() {
        info!("{:?}", prop);
    }
    let mut cpus = Vec::new();
    for dtb_cpu in dtb_cpus {
        let mut cpu = CPU {
            id: dtb_cpu.ids().first(),
            usable: true,
            clock_freq: dtb_cpu
                .properties()
                .find(|p| p.name == "clock-frequency")
                .map(|p| {
                    let mut a32: [u8; 4] = [0; 4];
                    let mut a64: [u8; 8] = [0; 8];
                    a32.copy_from_slice(p.value);
                    a64.copy_from_slice(p.value);
                    match p.value.len() {
                        4 => u32::from_be_bytes(a32) as usize,
                        8 => u64::from_be_bytes(a64) as usize,
                        _ => unreachable!(),
                    }
                })
                .or_else(|| Some(0))
                .unwrap(),
            timebase_freq: dtb_cpu.timebase_frequency(),
        };

        // Mask CPU without MMU
        // Get RISC-V ISA string
        let isa = dtb_cpu.property("riscv,isa").expect("RISC-V ISA not found");
        if isa.as_str().unwrap().contains("u") {
            // Privleged mode is in ISA string
            if !isa.as_str().unwrap().contains("s") {
                cpu.usable = false;
            }
        }
        // Check mmu type
        let mmu_type = dtb_cpu.property("mmu-type");
        if mmu_type.is_none() {
            cpu.usable = false;
        }
        // Add to list
        cpus.push(cpu);
    }

    cpus
}
