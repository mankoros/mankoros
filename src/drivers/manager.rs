//! Device manager
//!

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use log::warn;

use crate::{
    boot,
    memory::{self, address::VirtAddr, kernel_phys_dev_to_virt, pagetable::pte::PTEFlags},
};

use super::{cpu, BlockDevice, CharDevice, Device};

pub struct DeviceManager {
    cpus: Vec<cpu::CPU>,
    devices: Vec<Arc<dyn Device>>,
    interrupt_map: BTreeMap<usize, Arc<dyn Device>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            interrupt_map: BTreeMap::new(),
            cpus: Vec::new(),
        }
    }

    pub fn disks(&self) -> Vec<Arc<dyn BlockDevice>> {
        self.devices
            .iter()
            .map(|d| d.clone().as_blk())
            .filter(|d| d.is_some())
            .map(|d| d.unwrap())
            .collect::<Vec<_>>()
    }
    pub fn serials(&self) -> Vec<Arc<dyn CharDevice>> {
        self.devices
            .iter()
            .map(|d| d.clone().as_char())
            .filter(|d| d.is_some())
            .map(|d| d.unwrap())
            .collect::<Vec<_>>()
    }

    pub fn probe(&mut self) {
        // Probe CPU
        self.cpus = cpu::probe();
        // Probe Devices
        if let Some(dev) = super::blk::probe() {
            self.devices.push(Arc::new(dev));
        }
        if let Some(dev) = super::serial::probe() {
            self.devices.push(Arc::new(dev));
        }

        // Map PLIC first
        // TODO: use a plic driver instead
        let mut kernel_page_table = memory::pagetable::pagetable::PageTable::new_with_paddr(
            (boot::boot_pagetable_paddr()).into(),
        );
        kernel_page_table.map_region(
            (kernel_phys_dev_to_virt(0xc00_0000)).into(),
            0xc00_0000.into(),
            0x600000,
            PTEFlags::R | PTEFlags::W | PTEFlags::A | PTEFlags::D,
        );
        // Avoid drop
        core::mem::forget(kernel_page_table);

        // Register interrupt
        let plic = unsafe { kernel_phys_dev_to_virt(0xc000000) as *mut plic::Plic };
        for dev in self.devices.iter() {
            if let Some(irq) = dev.interrupt_number() {
                self.interrupt_map.insert(irq, dev.clone());

                // Setup PLIC
                let plicwrapper = PLICWrapper::new(irq);

                unsafe { (*plic).set_threshold(plicwrapper, 0) };
                unsafe { (*plic).enable(plicwrapper, plicwrapper) };
                unsafe { (*plic).set_priority(plicwrapper, 6) };
            }
        }
        // Enable external interrupts
        unsafe { riscv::register::sie::set_sext() };
    }

    pub fn interrupt_handler(&mut self, irq: usize) {
        if let Some(dev) = self.interrupt_map.get(&irq) {
            dev.interrupt_handler();
        } else {
            warn!("Unknown interrupt: {}", irq);
        }
    }

    pub fn map_devices(&self) {
        let mut kernel_page_table = memory::pagetable::pagetable::PageTable::new_with_paddr(
            (boot::boot_pagetable_paddr()).into(),
        );

        for dev in self.devices.iter() {
            let size = VirtAddr::from(dev.mmio_size());
            kernel_page_table.map_region(
                kernel_phys_dev_to_virt(dev.mmio_base()).into(),
                dev.mmio_base().into(),
                size.round_up().bits(),
                PTEFlags::rw(),
            )
        }

        // Avoid drop
        core::mem::forget(kernel_page_table);
    }

    pub fn devices_init(&mut self) {
        for dev in self.devices.iter() {
            dev.init();
        }
    }

    // Return the hart id of usable CPU
    pub fn bootable_cpus(&self) -> Vec<usize> {
        self.cpus.iter().filter(|c| c.usable).map(|c| c.id).collect()
    }

    pub fn cpu_freqs(&self) -> Vec<usize> {
        self.cpus.iter().map(|c| c.clock_freq).collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct PLICWrapper {
    irq: usize,
}
impl PLICWrapper {
    fn new(irq: usize) -> Self {
        Self { irq }
    }
}
impl plic::InterruptSource for PLICWrapper {
    fn id(self) -> core::num::NonZeroU32 {
        core::num::NonZeroU32::try_from(self.irq as u32).unwrap()
    }
}
impl plic::HartContext for PLICWrapper {
    fn index(self) -> usize {
        // hart 0 s mode
        2 // TODO: impl a dev manager to manage harts and generate PLIC context map
    }
}
