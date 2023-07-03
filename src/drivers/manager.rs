//! Device manager
//!

use core::{any::Any, cell::RefCell};

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

use super::{BlockDevice, Device};

pub struct DeviceManager {
    devices: Vec<RefCell<Arc<Box<dyn Any>>>>,
    interrupt_map: BTreeMap<usize, Box<dyn Device>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            devices: Vec::new(),
            interrupt_map: BTreeMap::new(),
        }
    }
    pub fn disks(&self) -> Vec<Arc<Box<dyn BlockDevice>>> {
        self.devices
            .iter()
            .map(|d| {
                if let Some(d) = d.borrow().downcast_ref::<Arc<Box<dyn BlockDevice>>>() {
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
            self.devices.push(RefCell::new(Arc::new(Box::new(dev))));
        }
    }
}
