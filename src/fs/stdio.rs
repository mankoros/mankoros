//! 标准输入输出流的 File 封装
//!
//! Adapted from MaturinOS
//! Copyright 2022 (C) MaturinOS
//! Copyright 2023 (C) MankorOS

use crate::{axerrno::AxError, impl_vfs_non_dir_default};

use super::vfs::{
    filesystem::VfsNode,
    node::{VfsNodeAttr, VfsNodePermission, VfsNodeType},
    AVfsResult, VfsResult,
};
use alloc::boxed::Box;
use log::warn;

/// 标准输入流
pub struct Stdin;
/// 标准输出流
pub struct Stdout;
/// 错误输出流。目前会和 Stdout 一样直接打印出来，
/// TODO: 当stdout卡死的时候照常输出
pub struct Stderr;

impl VfsNode for Stdin {
    impl_vfs_non_dir_default! {}

    fn write_at(&self, _offset: u64, _buf: &[u8]) -> AVfsResult<usize> {
        // Stdin is not writable
        Box::pin(async move { crate::ax_err!(Unsupported) })
    }

    fn fsync(&self) -> VfsResult {
        crate::ax_err!(Unsupported)
    }

    fn truncate(&self, _size: u64) -> VfsResult {
        crate::ax_err!(Unsupported)
    }
    fn read_at<'a>(&'a self, _offset: u64, buf: &'a mut [u8]) -> AVfsResult<usize> {
        // Offset is ignored
        Box::pin(async move {
            if buf.len() == 0 {
                return Ok(0);
            }
            // TODO: implement read
            Ok(1)
        })
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

impl VfsNode for Stdout {
    impl_vfs_non_dir_default! {}

    fn write_at<'a>(&'a self, _offset: u64, buf: &'a [u8]) -> AVfsResult<usize> {
        Box::pin(async move {
            if let Ok(data) = core::str::from_utf8(buf) {
                cfg_if::cfg_if! {
                    // See https://doc.rust-lang.org/reference/conditional-compilation.html#debug_assertions
                    if #[cfg(debug_assertions)] {
                        warn!("User stdout: {}", data);
                    } else {
                        crate::print!("{}", data);
                    }
                }
                Ok(buf.len())
            } else {
                Err(AxError::InvalidData)
            }
        })
    }

    fn fsync(&self) -> VfsResult {
        crate::ax_err!(Unsupported)
    }

    fn truncate(&self, _size: u64) -> VfsResult {
        crate::ax_err!(Unsupported)
    }
    fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> AVfsResult<usize> {
        // Stdout is not readable
        Box::pin(async move { crate::ax_err!(Unsupported) })
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

impl VfsNode for Stderr {
    impl_vfs_non_dir_default! {}

    fn write_at<'a>(&'a self, _offset: u64, buf: &'a [u8]) -> AVfsResult<usize> {
        Box::pin(async move {
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

    fn fsync(&self) -> VfsResult {
        crate::ax_err!(Unsupported)
    }

    fn truncate(&self, _size: u64) -> VfsResult {
        crate::ax_err!(Unsupported)
    }
    fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> AVfsResult<usize> {
        // Stderr is not readable
        Box::pin(async move { crate::ax_err!(Unsupported) })
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
