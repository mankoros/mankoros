//! 标准输入输出流的 File 封装
//!
//! Adapted from MaturinOS
//! Copyright 2022 (C) MaturinOS
//! Copyright 2023 (C) MankorOS

use core::pin::Pin;

use super::new_vfs::{top::VfsFile, DeviceIDCollection};
use crate::{
    drivers, impl_vfs_default_non_dir,
    tools::errors::{dyn_future, ASysResult, LinuxError, SysError},
};

use log::warn;

/// 标准输入流
pub struct Stdin;
/// 标准输出流
pub struct Stdout;
/// 错误输出流。目前会和 Stdout 一样直接打印出来，
/// TODO: 当stdout卡死的时候照常输出
pub struct Stderr;

impl VfsFile for Stdin {
    impl_vfs_default_non_dir!(Stdin);

    fn attr_kind(&self) -> super::new_vfs::VfsFileKind {
        super::new_vfs::VfsFileKind::CharDevice
    }
    fn attr_device(&self) -> super::new_vfs::top::DeviceInfo {
        super::new_vfs::top::DeviceInfo {
            device_id: DeviceIDCollection::DEV_FS_ID,
            self_device_id: DeviceIDCollection::STDIN_FS_ID,
        }
    }
    fn attr_size(&self) -> ASysResult<super::new_vfs::top::SizeInfo> {
        dyn_future(async { Ok(super::new_vfs::top::SizeInfo::new_zero()) })
    }
    fn attr_time(&self) -> ASysResult<super::new_vfs::top::TimeInfo> {
        dyn_future(async { Ok(super::new_vfs::top::TimeInfo::new_zero()) })
    }
    fn update_time(&self, _info: super::new_vfs::top::TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async { Err(SysError::EPERM) })
    }

    fn read_at<'a>(&'a self, _offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        // Offset is ignored
        // ensure_offset_is_tail!(offset);
        let buf = Pin::new(buf);
        dyn_future(async {
            if buf.is_empty() {
                return Ok(0);
            }
            if let Some(serial) = drivers::get_device_manager().serials().get(0) {
                return (serial).read(buf).await.map_err(|_| LinuxError::EIO);
            }
            Ok(0)
        })
    }

    fn get_page(
        &self,
        _offset: usize,
        _kind: super::new_vfs::top::MmapKind,
    ) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("Stdin::get_page")
    }
    fn truncate(&self, _length: usize) -> ASysResult {
        unimplemented!("Stdin::truncate")
    }

    fn poll_ready(
        &self,
        _offset: usize,
        _len: usize,
        kind: super::new_vfs::top::PollKind,
    ) -> ASysResult<usize> {
        dyn_future(async move {
            if kind != super::new_vfs::top::PollKind::Read {
                Err(SysError::EPERM)
            } else {
                // TODO: implement read
                Ok(1)
            }
        })
    }
    fn poll_read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
        // TODO: implement read
        1
    }
    fn poll_write(&self, _offset: usize, _buf: &[u8]) -> usize {
        panic!("Stdin::poll_write")
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}

impl VfsFile for Stdout {
    impl_vfs_default_non_dir!(Stdout);

    fn attr_kind(&self) -> super::new_vfs::VfsFileKind {
        super::new_vfs::VfsFileKind::CharDevice
    }
    fn attr_device(&self) -> super::new_vfs::top::DeviceInfo {
        super::new_vfs::top::DeviceInfo {
            device_id: DeviceIDCollection::DEV_FS_ID,
            self_device_id: DeviceIDCollection::STDOUT_FS_ID,
        }
    }
    fn attr_size(&self) -> ASysResult<super::new_vfs::top::SizeInfo> {
        dyn_future(async { Ok(super::new_vfs::top::SizeInfo::new_zero()) })
    }
    fn attr_time(&self) -> ASysResult<super::new_vfs::top::TimeInfo> {
        dyn_future(async { Ok(super::new_vfs::top::TimeInfo::new_zero()) })
    }
    fn update_time(&self, _info: super::new_vfs::top::TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move { Ok(self.poll_write(offset, buf)) })
    }

    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move { Err(SysError::EPERM) })
    }

    fn get_page(
        &self,
        _offset: usize,
        _kind: super::new_vfs::top::MmapKind,
    ) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("Stdout::get_page")
    }
    fn truncate(&self, _length: usize) -> ASysResult {
        unimplemented!("Stdout::truncate")
    }

    fn poll_ready(
        &self,
        _offset: usize,
        len: usize,
        kind: super::new_vfs::top::PollKind,
    ) -> ASysResult<usize> {
        // ensure_offset_is_tail!(offset);
        dyn_future(async move {
            if kind != super::new_vfs::top::PollKind::Write {
                Err(SysError::EPERM)
            } else {
                Ok(len)
            }
        })
    }
    fn poll_read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
        panic!("Stdout::poll_read")
    }
    fn poll_write(&self, _offset: usize, buf: &[u8]) -> usize {
        // ensure_offset_is_tail!(offset);
        if let Ok(data) = core::str::from_utf8(buf) {
            cfg_if::cfg_if! {
                if #[cfg(debug_assertions)] {
                    warn!("User stdout: {}", data);
                } else {
                    use crate::print;
                    print!("{}", data);
                }
            }
        } else {
            for i in 0..buf.len() {
                warn!("User stdout (non-utf8): {} ", buf[i]);
            }
        }
        buf.len()
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}

impl VfsFile for Stderr {
    impl_vfs_default_non_dir!(Stdout);

    fn attr_kind(&self) -> super::new_vfs::VfsFileKind {
        super::new_vfs::VfsFileKind::CharDevice
    }
    fn attr_device(&self) -> super::new_vfs::top::DeviceInfo {
        super::new_vfs::top::DeviceInfo {
            device_id: DeviceIDCollection::DEV_FS_ID,
            self_device_id: DeviceIDCollection::STDERR_FS_ID,
        }
    }
    fn attr_size(&self) -> ASysResult<super::new_vfs::top::SizeInfo> {
        dyn_future(async { Ok(super::new_vfs::top::SizeInfo::new_zero()) })
    }
    fn attr_time(&self) -> ASysResult<super::new_vfs::top::TimeInfo> {
        dyn_future(async { Ok(super::new_vfs::top::TimeInfo::new_zero()) })
    }
    fn update_time(&self, _info: super::new_vfs::top::TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move { Ok(self.poll_write(offset, buf)) })
    }

    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move { Err(SysError::EPERM) })
    }

    fn get_page(
        &self,
        _offset: usize,
        _kind: super::new_vfs::top::MmapKind,
    ) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("stderr::get_page")
    }
    fn truncate(&self, _length: usize) -> ASysResult {
        unimplemented!("stderr::truncate")
    }

    fn poll_ready(
        &self,
        _offset: usize,
        len: usize,
        kind: super::new_vfs::top::PollKind,
    ) -> ASysResult<usize> {
        // ensure_offset_is_tail!(offset);
        dyn_future(async move {
            if kind != super::new_vfs::top::PollKind::Write {
                Err(SysError::EPERM)
            } else {
                Ok(len)
            }
        })
    }
    fn poll_read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
        panic!("stderr::poll_read")
    }
    fn poll_write(&self, _offset: usize, buf: &[u8]) -> usize {
        // ensure_offset_is_tail!(offset);
        if let Ok(data) = core::str::from_utf8(buf) {
            cfg_if::cfg_if! {
                if #[cfg(debug_assertions)] {
                    warn!("User stderr: {}", data);
                } else {
                    use crate::print;
                    print!("{}", data);
                }
            }
        } else {
            for i in 0..buf.len() {
                warn!("User stderr (non-utf8): {} ", buf[i]);
            }
        }
        buf.len()
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
