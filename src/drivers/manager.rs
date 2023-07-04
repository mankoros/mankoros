//! Device manager
//!

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use log::warn;

use crate::{
    arch, boot,
    memory::{self, address::VirtAddr, kernel_phys_dev_to_virt, pagetable::pte::PTEFlags},
};

use super::{cpu, plic, BlockDevice, CharDevice, Device};

pub struct DeviceManager {
    cpus: Vec<cpu::CPU>,
    plic: plic::PLIC,
    devices: Vec<Arc<dyn Device>>,
    interrupt_map: BTreeMap<usize, Arc<dyn Device>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            interrupt_map: BTreeMap::new(),
            cpus: Vec::new(),
            plic: plic::PLIC::new(0, 0),
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

        // Probe PLIC
        self.plic = plic::probe();

        // Probe Devices
        if let Some(dev) = super::blk::probe() {
            self.devices.push(Arc::new(dev));
        }
        if let Some(dev) = super::serial::probe() {
            self.devices.push(Arc::new(dev));
        }

        // Add to interrupt map if have interrupts
        for dev in self.devices.iter() {
            if let Some(irq) = dev.interrupt_number() {
                self.interrupt_map.insert(irq, dev.clone());
            }
        }
    }

    pub fn interrupt_handler(&mut self) {
        // First clain interrupt from PLIC
        if let Some(irq_number) = self.plic.claim_irq(self.irq_context()) {
            if let Some(dev) = self.interrupt_map.get(&irq_number) {
                dev.interrupt_handler();
                // Complete interrupt when done
                self.plic.complete_irq(irq_number, self.irq_context());
                return;
            }
            warn!("Unknown interrupt: {}", irq_number);
            return;
        }
        warn!("No interrupt available");
    }

    pub fn map_devices(&self) {
        let mut kernel_page_table = memory::pagetable::pagetable::PageTable::new_with_paddr(
            (boot::boot_pagetable_paddr()).into(),
        );

        // Map probed devices
        for dev in self.devices.iter() {
            let size = VirtAddr::from(dev.mmio_size());
            kernel_page_table.map_region(
                kernel_phys_dev_to_virt(dev.mmio_base()).into(),
                dev.mmio_base().into(),
                size.round_up().bits(),
                PTEFlags::rw(),
            );
        }

        // Map PLIC
        kernel_page_table.map_region(
            kernel_phys_dev_to_virt(self.plic.base_address).into(),
            self.plic.base_address.into(),
            self.plic.size,
            PTEFlags::rw(),
        );

        // Avoid drop
        core::mem::forget(kernel_page_table);
    }

    pub fn devices_init(&mut self) {
        for dev in self.devices.iter() {
            dev.init();
        }
    }

    pub fn enable_external_interrupts(&mut self) {
        for dev in self.devices.iter() {
            if let Some(irq) = dev.interrupt_number() {
                self.plic.enable_irq(irq, self.irq_context());
            }
        }
        unsafe { riscv::register::sie::set_sext() };
    }

    // Return the hart id of usable CPU
    pub fn bootable_cpus(&self) -> Vec<usize> {
        self.cpus.iter().filter(|c| c.usable).map(|c| c.id).collect()
    }

    pub fn cpu_freqs(&self) -> Vec<usize> {
        self.cpus.iter().map(|c| c.clock_freq).collect()
    }

    fn min_hart_id(&self) -> usize {
        self.bootable_cpus().iter().min().unwrap().clone()
    }

    // Calculate the interrupt context from current hart id
    fn irq_context(&self) -> usize {
        2 * arch::get_hart_id() - self.min_hart_id() + 1
    }
}
