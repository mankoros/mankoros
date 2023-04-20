use alloc::sync::Arc;

use crate::{here, sync::SpinNoIrqLock};

use super::disk::Disk;
use crate::driver::DevResult;
use log::info;

#[derive(Debug, Clone)]
pub struct Partition {
    offset: u64,
    size: u64,
    pos: u64,
    disk: Arc<SpinNoIrqLock<Disk>>,
}

impl Partition {
    pub fn new(offset: u64, size: u64, disk: Arc<SpinNoIrqLock<Disk>>) -> Self {
        let humain_size = humansize::SizeFormatter::new(size, humansize::BINARY);
        info!("Partition: offset: 0x{:x}, size: {}", offset, humain_size);
        Self {
            offset,
            size,
            pos: 0,
            disk,
        }
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn read_one(&mut self, buf: &mut [u8]) -> DevResult<usize> {
        let mut disk = self.disk.lock(here!());
        disk.set_position(self.pos + self.offset);
        let result = disk.read_one(buf);
        self.pos = disk.position() - self.offset;
        result
    }
    pub fn write_one(&mut self, buf: &[u8]) -> DevResult<usize> {
        let mut disk = self.disk.lock(here!());
        disk.set_position(self.pos + self.offset);
        let result = disk.write_one(buf);
        self.pos = disk.position() - self.offset;
        result
    }
    pub fn position(&self) -> u64 {
        self.pos
    }
    pub fn set_position(&mut self, pos: u64) {
        self.pos = pos;
    }
}
