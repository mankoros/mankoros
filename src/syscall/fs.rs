//! Filesystem related syscall
//!

use log::info;

use crate::{
    axerrno::AxError,
    fs::{self, vfs::filesystem::VfsNode},
    memory::kernel_phys_to_virt,
    process::process::FileDescriptor,
    utils,
};

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
        // TODO: check if closed
        info!("Syscall: write, fd {fd}");
        self.process.with_alive(|a| {
            let mut fds = a.get_file_descripter().clone();

            // Sanity check
            if fd >= fds.len() {
                return Err(AxError::InvalidInput);
            }
            // Allow acessing user vaddr
            unsafe { riscv::register::sstatus::set_sum() };

            let file = &mut fds[fd];

            let write_len =
                file.file.write_at(0, unsafe { core::slice::from_raw_parts(buf, len) })?;

            unsafe { riscv::register::sstatus::clear_sum() };

            Ok(write_len)
        })
    }
    pub fn sys_read(&mut self, fd: usize, buf: *mut u8, len: usize) -> SyscallResult {
        info!("Syscall: read, fd {fd}");
        self.process.with_alive(|a| {
            let mut fds = a.get_file_descripter().clone();

            // Sanity check
            if fd >= fds.len() {
                return Err(AxError::InvalidInput);
            }
            // Allow acessing user vaddr
            unsafe { riscv::register::sstatus::set_sum() };

            let file = &mut fds[fd];

            let read_len =
                file.file.read_at(0, unsafe { core::slice::from_raw_parts_mut(buf, len) })?;

            unsafe { riscv::register::sstatus::clear_sum() };

            Ok(read_len)
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
        self.process.with_alive(|a| {
            let root_fs = fs::root::get_root_dir();

            // Allow acessing user vaddr
            unsafe { riscv::register::sstatus::set_sum() };

            let file = root_fs
                .lookup(unsafe { utils::raw_ptr_to_ref_str(path) })
                .expect("Error looking up file");

            let fds = a.get_file_descripter();
            let fd = fds.len();
            fds.push(FileDescriptor::new(file));

            unsafe { riscv::register::sstatus::clear_sum() };

            Ok(fd)
        })
    }

    pub fn sys_close(&mut self, fd: usize) -> SyscallResult {
        info!("Syscall: close");
        self.process.with_alive(|a| {
            let mut fds = a.get_file_descripter().clone();

            // Sanity check
            if fd >= fds.len() {
                return Err(AxError::InvalidInput);
            }
            fds[fd].is_closed = true;
            Ok(fd)
        })
    }

    pub fn sys_fstat(&self, fd: usize, kstat: *mut Kstat) -> SyscallResult {
        Ok(0)
    }
}
