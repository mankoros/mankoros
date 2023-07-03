//! Device manager
//!

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use log::warn;

use crate::memory::kernel_phys_dev_to_virt;

use super::{BlockDevice, Device};

pub struct DeviceManager {
    devices: Vec<Arc<dyn Device>>,
    interrupt_map: BTreeMap<usize, Arc<dyn Device>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            interrupt_map: BTreeMap::new(),
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

    pub fn probe(&mut self) {
        if let Some(dev) = super::blk::probe() {
            self.devices.push(Arc::new(dev));
        }

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
        1 // TODO: impl a dev manager to manage harts and generate PLIC context map
    }
}
