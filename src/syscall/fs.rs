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
}
