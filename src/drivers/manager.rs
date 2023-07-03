//! Device manager
//!

use core::{any::Any, cell::RefCell};

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

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
            .map(|d| {
                if let Some(d) = d.clone().as_blk() {
                    Some(d.clone())
                } else {
                    None
                }
            })
            .filter(|d| d.is_some())
            .map(|d| d.unwrap())
            .collect::<Vec<_>>()
    }

    pub fn probe(&mut self) {
        if let Some(dev) = super::blk::probe() {
            self.devices.push(Arc::new(dev));
        }
    }
}
