//! Device manager
//!

use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use log::warn;

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
        for dev in self.devices.iter() {
            if let Some(irq) = dev.interrupt_number() {
                self.interrupt_map.insert(irq, dev.clone());
            }
        }
    }

    pub fn interrupt_handler(&mut self, irq: usize) {
        if let Some(dev) = self.interrupt_map.get(&irq) {
            dev.interrupt_handler();
        } else {
            warn!("Unknown interrupt: {}", irq);
        }
    }
}
