//! 标准输入输出流的 File 封装
//!
//! Adapted from MaturinOS
//! Copyright 2022 (C) MaturinOS
//! Copyright 2023 (C) MankorOS

use alloc::boxed::Box;
use log::warn;
use super::new_vfs::{underlying::FsNode, info::NodeStat};
use crate::{tools::errors::{ASysResult, SysResult, dyn_future, SysError}, impl_default_non_dir};

/// 标准输入流
pub struct Stdin;
/// 标准输出流
pub struct Stdout;
/// 错误输出流。目前会和 Stdout 一样直接打印出来，
/// TODO: 当stdout卡死的时候照常输出
pub struct Stderr;

impl FsNode for Stdin {
    
    fn read_at<'a>(&'a self, _offset: u64, buf: &'a mut [u8]) -> ASysResult<usize> {
        // Offset is ignored
        dyn_future(async move {
            if buf.len() == 0 {
                return Ok(0);
            }
            // TODO: implement read
            Ok(1)
        })
    }
    fn write_at(&self, _offset: u64, _buf: &[u8]) -> ASysResult<usize> {
        // Stdin is not writable
        dyn_future(async { Err(SysError::EPERM) })
    }

    fn stat(&self) -> ASysResult<NodeStat> {
        dyn_future(async {
            Ok(NodeStat::default(super::new_vfs::info::NodeType::CharDevice))
        })
    }

    impl_default_non_dir!(Stdin);
}

impl FsNode for Stdout {
    fn write_at<'a>(&'a self, _offset: u64, buf: &'a [u8]) -> ASysResult<usize> {
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
                Err(SysError::EINVAL)
            }
        })
    }

    fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> ASysResult<usize> {
        // Stdout is not readable
        dyn_future(async move { Err(SysError::EPERM) })
    }

    fn stat(&self) -> ASysResult<NodeStat> {
        dyn_future(async {
            Ok(NodeStat::default(super::new_vfs::info::NodeType::CharDevice))
        })
    }

    impl_default_non_dir!(Stdout);
}

impl FsNode for Stderr {
    fn write_at<'a>(&'a self, _offset: u64, buf: &'a [u8]) -> ASysResult<usize> {
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

    fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> ASysResult<usize> {
        // Stdout is not readable
        dyn_future(async move { Err(SysError::EPERM) })
    }

    fn stat(&self) -> ASysResult<NodeStat> {
        dyn_future(async {
            Ok(NodeStat::default(super::new_vfs::info::NodeType::CharDevice))
        })
    }

    impl_default_non_dir!(Stderr);
}
