//! 标准输入输出流的 File 封装
//!
//! Adapted from MaturinOS
//! Copyright 2022 (C) MaturinOS
//! Copyright 2023 (C) MankorOS

use log::warn;
use super::new_vfs::{top::VfsFile, VfsFileAttr, DeviceIDCollection};
use crate::{impl_vfs_default_non_dir, tools::errors::{dyn_future, SysError, ASysResult}};

/// 标准输入流
pub struct Stdin;
/// 标准输出流
pub struct Stdout;
/// 错误输出流。目前会和 Stdout 一样直接打印出来，
/// TODO: 当stdout卡死的时候照常输出
pub struct Stderr;

impl VfsFile for Stdin {
    impl_vfs_default_non_dir!(Stdin);

    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async { Err(SysError::EPERM) })
    }

    fn read_at<'a>(&'a self, _offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        // Offset is ignored
        dyn_future(async move {
            if buf.len() == 0 {
                return Ok(0);
            }
            // TODO: implement read
            Ok(1)
        })
    }

    fn get_page(&self, _offset: usize, _kind: super::new_vfs::top::MmapKind) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("Stdin::get_page")
    }

    fn attr(&self) -> ASysResult<VfsFileAttr> {
        dyn_future(async { 
            Ok(VfsFileAttr {
                kind: super::new_vfs::VfsFileKind::CharDevice,
                device_id: DeviceIDCollection::DEV_FS_ID,
                self_device_id: DeviceIDCollection::STDIN_FS_ID,
                byte_size: 0,
                block_count: 0,
                access_time: 0,
                modify_time: 0,
                create_time: 0, // TODO: create time
            })
        })
    }
}

impl VfsFile for Stdout {
    impl_vfs_default_non_dir!(Stdout);

    fn write_at<'a>(&'a self, _offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            if let Ok(data) = core::str::from_utf8(buf) {
                warn!("User stdout: {}", data);
                Ok(buf.len())
            } else {
                for i in 0..buf.len() {
                    warn!("User stdout (non-utf8): {} ", buf[i]);
                }
                Ok(buf.len())
            }
        })
    }

    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move { Err(SysError::EPERM) })
    }

    fn get_page(&self, _offset: usize, _kind: super::new_vfs::top::MmapKind) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("Stdout::get_page")
    }

    fn attr(&self) -> ASysResult<VfsFileAttr> {
        dyn_future(async { 
            Ok(VfsFileAttr {
                kind: super::new_vfs::VfsFileKind::CharDevice,
                device_id: DeviceIDCollection::DEV_FS_ID,
                self_device_id: DeviceIDCollection::STDOUT_FS_ID,
                byte_size: 0,
                block_count: 0,
                access_time: 0,
                modify_time: 0,
                create_time: 0, // TODO: create time
            })
        })
    }
}

impl VfsFile for Stderr {
    impl_vfs_default_non_dir!(Stderr);

    fn write_at<'a>(&'a self, _offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            if let Ok(data) = core::str::from_utf8(buf) {
                warn!("User stderr: {}", data);
                Ok(buf.len())
            } else {
                for i in 0..buf.len() {
                    warn!("User stderr (non-utf8): {} ", buf[i]);
                }
                Ok(buf.len())
            }
        })
    }

    fn read_at<'a>(&'a self, _offset: usize, _buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move { Err(SysError::EPERM) })
    }

    fn get_page(&self, _offset: usize, _kind: super::new_vfs::top::MmapKind) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("Stderr::get_page")
    }

    fn attr(&self) -> ASysResult<VfsFileAttr> {
        dyn_future(async { 
            Ok(VfsFileAttr {
                kind: super::new_vfs::VfsFileKind::CharDevice,
                device_id: DeviceIDCollection::DEV_FS_ID,
                self_device_id: DeviceIDCollection::STDERR_FS_ID,
                byte_size: 0,
                block_count: 0,
                access_time: 0,
                modify_time: 0,
                create_time: 0, // TODO: create time
            })
        })
    }
}
