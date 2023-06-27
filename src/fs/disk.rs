use core::cell::Cell;

/// Disk related
///
/// Copyright (C) 2023 by ArceOS
/// Copyright (C) 2023 by MankorOS
///
/// Adapted from ArceOS
use super::BlockDevice;
use crate::axerrno::AxError;
use crate::driver::BlockDriverOps;
use crate::driver::DevResult;
use super::new_vfs::underlying::FsNode;
use crate::tools::errors::SysResult;
use super::new_vfs::info::NodeStat;
use crate::tools::errors::ASysResult;
use super::new_vfs::info::NodeType;
use crate::tools::errors::dyn_future;
use crate::impl_default_non_dir;

pub const BLOCK_SIZE: usize = 512;

/// A disk holds a block device, and can be used to read and write data to that block device.
#[derive(Debug)]
pub struct Disk {
    block_id: Cell<u64>,
    offset: Cell<usize>,
    dev: BlockDevice,
}

unsafe impl Send for Disk {}
unsafe impl Sync for Disk {}

impl Disk {
    /// Create a new disk.
    pub fn new(dev: BlockDevice) -> Self {
        assert_eq!(BLOCK_SIZE, dev.block_size());
        Self {
            block_id: Cell::new(0),
            offset: Cell::new(0),
            dev,
        }
    }

    /// Get the size of the disk.
    pub fn size(&self) -> u64 {
        self.dev.num_blocks() * BLOCK_SIZE as u64
    }

    /// Get the position of the cursor.
    pub fn position(&self) -> u64 {
        self.block_id.get() * BLOCK_SIZE as u64 + self.offset.get() as u64
    }

    /// Set the position of the cursor.
    pub fn set_position(&self, pos: u64) {
        self.block_id.set(pos / BLOCK_SIZE as u64);
        self.offset.set(pos as usize % BLOCK_SIZE);
    }

    /// Read within one block, returns the number of bytes read.
    pub fn read_one(&self, buf: &mut [u8]) -> DevResult<usize> {
        let read_size = if self.offset.get() == 0 && buf.len() >= BLOCK_SIZE {
            // whole block
            self.dev.read_block(self.block_id.get(), &mut buf[0..BLOCK_SIZE])?;
            self.block_id.set(self.block_id.get() + 1);
            BLOCK_SIZE
        } else {
            // partial block
            let mut data = [0u8; BLOCK_SIZE];
            let start = self.offset.get();
            let count = buf.len().min(BLOCK_SIZE - self.offset.get());

            self.dev.read_block(self.block_id.get(), &mut data)?;
            buf[..count].copy_from_slice(&data[start..start + count]);

            self.offset.set(self.offset.get() + count);
            if self.offset.get() >= BLOCK_SIZE {
                self.block_id.set(self.block_id.get() + 1);
                self.offset.set(self.offset.get() - BLOCK_SIZE);
            }
            count
        };
        Ok(read_size)
    }

    /// Write within one block, returns the number of bytes written.
    pub fn write_one(&self, buf: &[u8]) -> DevResult<usize> {
        let write_size = if self.offset.get() == 0 && buf.len() >= BLOCK_SIZE {
            // whole block
            self.dev.write_block(self.block_id.get(), &buf[0..BLOCK_SIZE])?;
            self.block_id.set(self.block_id.get() + 1);
            BLOCK_SIZE
        } else {
            // partial block
            let mut data = [0u8; BLOCK_SIZE];
            let start = self.offset.get();
            let count = buf.len().min(BLOCK_SIZE - self.offset.get());

            self.dev.read_block(self.block_id.get(), &mut data)?;
            data[start..start + count].copy_from_slice(&buf[..count]);
            self.dev.write_block(self.block_id.get(), &data)?;

            self.offset.set(self.offset.get() + count);
            if self.offset.get() >= BLOCK_SIZE {
                self.block_id.set(self.block_id.get() + 1);
                self.offset.set(self.offset.get() - BLOCK_SIZE);
            }
            count
        };
        Ok(write_size)
    }

    /// Read the master boot record.
    pub fn mbr(&mut self) -> mbr_nostd::MasterBootRecord {
        static mut MBR: [u8; 512] = [0u8; 512];
        self.set_position(0);
        unsafe {
            self.read_one(MBR.as_mut_slice()).expect("Read Master Boot Record failed");
            mbr_nostd::MasterBootRecord::from_bytes(&MBR).expect("Parse Master Boot Record failed")
        }
    }
}

impl Disk {
    pub fn sync_write_at(&self, offset: u64, mut buf: &[u8]) -> SysResult<usize> {
        let mut write_len = 0;
        self.set_position(offset);
        while !buf.is_empty() {
            match self.write_one(buf) {
                Ok(0) => break,
                Ok(n) => {
                    buf = &buf[n..];
                    write_len += n;
                }
                Err(_) => return Err(AxError::Io),
            }
        }
        Ok(write_len)
    }

    pub fn sync_read_at(&self, _offset: u64, buf: &mut [u8]) -> SysResult<usize> {
        // Offset is ignored
    
        if buf.len() == 0 {
            return Ok(0);
        }
        // TODO: implement read
        Ok(1)
    }
}

/// A disk can be a dev file
impl FsNode for Disk {
    fn stat(&self) -> ASysResult<NodeStat> {
        NodeStat::default(NodeType::BlockDevice)
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> ASysResult<usize> {
        dyn_future(async move { self.sync_read_at(offset as u64, buf) })
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> ASysResult<usize> {
        dyn_future(async move { self.sync_write_at(offset as u64, buf) })
    }

    impl_default_non_dir!(Disk);
}
