//! Device manager
//!

use core::cell::RefCell;

use alloc::{boxed::Box, collections::BTreeMap, sync::Arc, vec::Vec};

use super::{BlockDevice, Device};

pub struct DeviceManager {
    block_devices: Vec<RefCell<Arc<Box<dyn BlockDevice>>>>,
    interrupt_map: BTreeMap<usize, Box<dyn Device>>,
}

impl DeviceManager {
    pub fn new() -> Self {
        Self {
            block_devices: Vec::new(),
            interrupt_map: BTreeMap::new(),
        }
    }
    pub fn disks(&self) -> Vec<Arc<Box<dyn BlockDevice>>> {
        self.block_devices.iter().map(|d| d.borrow().clone()).collect::<Vec<_>>()
    }

    pub fn probe(&mut self) {
        if let Some(dev) = super::blk::probe() {
            self.block_devices.push(RefCell::new(Arc::new(Box::new(dev))));
        }
    }
}
