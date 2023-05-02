//! Filesystem related syscall
//!

use log::{debug, info};

use crate::{
    axerrno::AxError,
    fs::{self, vfs::filesystem::VfsNode},
    utils,
};

use super::within_sum;
use super::{Syscall, SyscallResult};

/// 文件信息类
#[repr(C)]
pub struct Kstat {
    /// 设备
    pub st_dev: u64,
    /// inode 编号
    pub st_ino: u64,
    /// 文件类型
    pub st_mode: u32,
    /// 硬链接数
    pub st_nlink: u32,
    /// 用户id
    pub st_uid: u32,
    /// 用户组id
    pub st_gid: u32,
    /// 设备号
    pub st_rdev: u64,
    _pad0: u64,
    /// 文件大小
    pub st_size: u64,
    /// 块大小
    pub st_blksize: u32,
    _pad1: u32,
    /// 块个数
    pub st_blocks: u64,
    /// 最后一次访问时间(秒)
    pub st_atime_sec: isize,
    /// 最后一次访问时间(纳秒)
    pub st_atime_nsec: isize,
    /// 最后一次修改时间(秒)
    pub st_mtime_sec: isize,
    /// 最后一次修改时间(纳秒)
    pub st_mtime_nsec: isize,
    /// 最后一次改变状态时间(秒)
    pub st_ctime_sec: isize,
    /// 最后一次改变状态时间(纳秒)
    pub st_ctime_nsec: isize,
}

impl<'a> Syscall<'a> {
    pub fn sys_write(&mut self, fd: usize, buf: *const u8, len: usize) -> SyscallResult {
        info!("Syscall: write, fd {fd}, len: {len}");

        self.lproc.with_mut_fdtable(|f| {
            if let Some(fd) = f.get(fd) {
                let write_len = within_sum(|| {
                    fd.file.write_at(0, unsafe { core::slice::from_raw_parts(buf, len) })
                })?;

                Ok(write_len)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }
    pub fn sys_read(&mut self, fd: usize, buf: *mut u8, len: usize) -> SyscallResult {
        info!("Syscall: read, fd {fd}");

        self.lproc.with_mut_fdtable(|f| {
            if let Some(fd) = f.get(fd) {
                let read_len = within_sum(|| {
                    fd.file.read_at(0, unsafe { core::slice::from_raw_parts_mut(buf, len) })
                })?;

                Ok(read_len)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }

    pub fn sys_openat(
        &mut self,
        _dir_fd: i32,
        path: *const u8,
        _flags: u32,
        _user_mode: i32,
    ) -> SyscallResult {
        info!("Syscall: openat");

        let root_fs = fs::root::get_root_dir();
        let file = within_sum(|| root_fs.lookup(unsafe { utils::raw_ptr_to_ref_str(path) }))
            .expect("Error looking up file");

        self.lproc.with_mut_fdtable(|f| Ok(f.alloc(file) as usize))
    }

    pub fn sys_close(&mut self, fd: usize) -> SyscallResult {
        info!("Syscall: close");

        self.lproc.with_mut_fdtable(|m| {
            if let Some(_) = m.remove(fd) {
                Ok(fd)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }

    pub fn sys_fstat(&self, _fd: usize, kstat: *mut Kstat) -> SyscallResult {
        info!("Syscall: fstat");
        // within_sum(|| unsafe {
        //     *kstat = Kstat {
        //         st_dev: 0,
        //         st_ino: 0,
        //         st_mode: 0,
        //         st_nlink: 0,
        //         st_uid: 0,
        //         st_gid: 0,
        //         st_rdev: 0,
        //         _pad0: 0,
        //         st_size: 0,
        //         st_blksize: 0,
        //         _pad1: 0,
        //         st_blocks: 0,
        //         st_atime_sec: 0,
        //         st_atime_nsec: 0,
        //         st_mtime_sec: 0,
        //         st_mtime_nsec: 0,
        //         st_ctime_sec: 0,
        //         st_ctime_nsec: 0,
        //     }
        // });
        Ok(0)
    }
}
