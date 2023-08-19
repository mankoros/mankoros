use core::cell::Cell;
use core::fmt::Debug;

use alloc::boxed::Box;
use alloc::sync::Arc;

/// Disk related
///
/// Copyright (C) 2023 by ArceOS
/// Copyright (C) 2023 by MankorOS
///
/// Adapted from ArceOS
use super::BlockDevice;

use crate::drivers::DevResult;

use super::new_vfs::underlying::ConcreteFile;
use super::new_vfs::DeviceIDCollection;
use super::new_vfs::VfsFileAttr;
use crate::tools::errors::dyn_future;
use crate::tools::errors::ASysResult;
use crate::tools::errors::SysError;
use crate::tools::errors::SysResult;

use super::new_vfs::page_cache::PageCacheFile;
use super::new_vfs::sync_attr_file::SyncAttrFile;
use super::new_vfs::top::VfsFileRef;
use alloc::string::String;
use alloc::vec::Vec;

pub const BLOCK_SIZE: usize = 512;
pub const LOG2_BLOCK_SIZE: usize = 9;

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
            dev,
        }
    }

    pub fn to_vfs_file(self) -> VfsFileRef {
        let block_count = self.dev.num_blocks() as usize;
        let byte_size = block_count * self.dev.block_size();
        let _attr = VfsFileAttr {
            kind: super::new_vfs::VfsFileKind::BlockDevice,
            device_id: 0, // TODO: Device id
            self_device_id: 0,
            byte_size,
            block_count,
            access_time: 0,
            modify_time: 0,
            create_time: 0,
        };
        let file = PageCacheFile::new(SyncAttrFile::new(self));
        VfsFileRef::new(file)
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
    fn sync_read_at(&self, _offset: u64, buf: &mut [u8]) -> SysResult<usize> {
        // Offset is ignored

        if buf.is_empty() {
            return Ok(0);
        }
        // TODO: implement read
        Ok(1)
    }
    fn read_at<'a>(&'a self, offset: u64, buf: &'a mut [u8]) -> ASysResult<usize> {
        Box::pin(async move { self.sync_read_at(offset, buf) })
    }

    fn write_at<'a>(&'a self, offset: u64, buf: &'a [u8]) -> ASysResult<usize> {
        Box::pin(async move { self.sync_write_at(offset, buf) })
    }
    /// 文件属性
    fn stat(&self) -> SysResult<VfsFileAttr> {
        let block_count = self.dev.num_blocks() as usize;
        let byte_size = self.dev.block_size() * block_count;

        Ok(VfsFileAttr {
            kind: super::new_vfs::VfsFileKind::CharDevice,
            device_id: DeviceIDCollection::DEV_FS_ID,
            self_device_id: 0, // TODO: 让每一个 BlockDevice 有一个 id
            byte_size,
            block_count,
            access_time: 0,
            modify_time: 0,
            create_time: 0, // TODO: create time
        })
    }

    fn sync_write_at(&self, offset: u64, mut buf: &[u8]) -> SysResult<usize> {
        let mut write_len = 0;
        self.set_position(offset);
        while !buf.is_empty() {
            match self.write_one(buf) {
                Ok(0) => break,
                Ok(n) => {
                    buf = &buf[n..];
                    write_len += n;
                }
                Err(_) => return Err(SysError::EIO),
            }
        }
        Ok(write_len)
    }
}

/// A disk can be a dev file
impl ConcreteFile for Disk {
    // TODO: async dir read/write
    fn read_page_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move { self.sync_read_at(offset as u64, buf) })
    }
    fn write_page_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move { self.sync_write_at(offset as u64, buf) })
    }

    fn attr_kind(&self) -> super::new_vfs::VfsFileKind {
        super::new_vfs::VfsFileKind::BlockDevice
    }
    fn attr_device(&self) -> super::new_vfs::top::DeviceInfo {
        super::new_vfs::top::DeviceInfo {
            device_id: DeviceIDCollection::DEV_FS_ID,
            self_device_id: 0,
        }
    }
    fn attr_size(&self) -> ASysResult<super::new_vfs::top::SizeInfo> {
        dyn_future(async {
            Ok(super::new_vfs::top::SizeInfo {
                bytes: self.size() as _,
                blocks: self.dev.block_size(),
            })
        })
    }
    fn attr_time(&self) -> ASysResult<super::new_vfs::top::TimeInfo> {
        dyn_future(async {
            Ok(super::new_vfs::top::TimeInfo {
                access: 0,
                modify: 0,
                change: 0,
            })
        })
    }
    fn update_time(&self, _info: super::new_vfs::top::TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn delete(&self) -> ASysResult {
        todo!()
    }

    fn truncate(&self, _new_size: usize) -> ASysResult {
        todo!()
    }

    fn lookup<'a>(&'a self, _name: &'a str) -> ASysResult<Self> {
        todo!()
    }

    fn list(&self) -> ASysResult<Vec<(String, Self)>> {
        todo!()
    }

    fn create<'a>(
        &'a self,
        _name: &'a str,
        _kind: super::new_vfs::VfsFileKind,
    ) -> ASysResult<Self> {
        todo!()
    }

    fn rename(&self, _file: &Self, _new_name: &str) -> ASysResult {
        todo!()
    }

    fn detach(&self, _file: &Self) -> ASysResult {
        todo!()
    }

    fn attach<'a>(&'a self, _file: &'a Self, _name: &'a str) -> ASysResult {
        todo!()
    }
}
