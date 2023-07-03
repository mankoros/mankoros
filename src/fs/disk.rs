use core::cell::Cell;
use core::fmt::Debug;

use alloc::boxed::Box;
use alloc::sync::Arc;

use super::vfs::filesystem::VfsNode;
use super::vfs::node::VfsNodeAttr;
use super::vfs::node::VfsNodePermission;
use super::vfs::node::VfsNodeType;
use super::vfs::AVfsResult;
use super::vfs::VfsResult;
/// Disk related
///
/// Copyright (C) 2023 by ArceOS
/// Copyright (C) 2023 by MankorOS
///
/// Adapted from ArceOS
use crate::axerrno::AxError;
use crate::drivers::BlockDevice;
use crate::drivers::DevResult;
use crate::impl_vfs_non_dir_default;

pub const BLOCK_SIZE: usize = 512;

/// A disk holds a block device, and can be used to read and write data to that block device.
pub struct Disk {
    block_id: Cell<u64>,
    offset: Cell<usize>,
    dev: Arc<dyn BlockDevice>,
}

unsafe impl Send for Disk {}
unsafe impl Sync for Disk {}

impl Debug for Disk {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Disk")
            .field("block_id", &self.block_id.get())
            .field("offset", &self.offset.get())
            .field("dev", &self.dev.name())
            .finish()
            .fmt(f)
    }
}

impl Disk {
    /// Create a new disk.
    pub fn new(dev: Arc<dyn BlockDevice>) -> Self {
        assert_eq!(BLOCK_SIZE, dev.block_size());
        Self {
            block_id: Cell::new(0),
            offset: Cell::new(0),
            dev: dev,
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

/// A disk can be a dev file
impl VfsNode for Disk {
    impl_vfs_non_dir_default! {}

    fn sync_write_at(&self, offset: u64, mut buf: &[u8]) -> VfsResult<usize> {
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

    fn fsync(&self) -> VfsResult {
        // No cache is used here
        Ok(())
    }

    fn truncate(&self, _size: u64) -> VfsResult {
        crate::ax_err!(Unsupported)
    }
    fn sync_read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        // Offset is ignored

        if buf.len() == 0 {
            return Ok(0);
        }
        // TODO: implement read
        Ok(1)
    }
    fn read_at<'b>(&'b self, offset: u64, buf: &'b mut [u8]) -> AVfsResult<usize> {
        Box::pin(async move { self.sync_read_at(offset, buf) })
    }

    fn write_at<'b>(&'b self, offset: u64, buf: &'b [u8]) -> AVfsResult<usize> {
        Box::pin(async move { self.sync_write_at(offset, buf) })
    }
    /// 文件属性
    fn stat(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new(
            VfsNodePermission::all(),
            VfsNodeType::CharDevice,
            0,
            0,
        ))
    }
}
