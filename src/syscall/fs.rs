//! Filesystem related syscall
//!

use log::info;

use crate::{
    axerrno::AxError,
    fs::{self, vfs::filesystem::VfsNode},
    memory::kernel_phys_to_virt,
    utils,
};

use super::{Syscall, SyscallResult};

impl<'a> Syscall<'a> {
    pub fn sys_write(&mut self, fd: usize, buf: *const u8, len: usize) -> SyscallResult {
        info!("Syscall: write, fd {fd}");
        self.process.with_alive(|a| {
            let mut fds = a.get_file_descripter().clone();

            // Sanity check
            if fd >= fds.len() {
                return Err(AxError::InvalidInput);
            }
            // Convert user vaddr
            // TODO: do not panic when invalid vaddr
            let paddr = a.get_user_space().page_table.get_paddr_from_vaddr((buf as usize).into());

            let kernel_vaddr = kernel_phys_to_virt(paddr.into());

            let file = &mut fds[fd];

            let write_len = file.write_at(0, unsafe {
                core::slice::from_raw_parts(kernel_vaddr as *const u8, len)
            })?;

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
            // Convert user vaddr
            // TODO: do not panic when invalid vaddr
            let paddr = a.get_user_space().page_table.get_paddr_from_vaddr((buf as usize).into());

            let kernel_vaddr = kernel_phys_to_virt(paddr.into());

            let file = &mut fds[fd];

            let read_len = file.read_at(0, unsafe {
                core::slice::from_raw_parts_mut(kernel_vaddr as *mut u8, len)
            })?;

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

            // Convert user vaddr
            // TODO: do not panic when invalid vaddr
            let paddr = a.get_user_space().page_table.get_paddr_from_vaddr((path as usize).into());

            let kernel_vaddr = kernel_phys_to_virt(paddr.into());

            let file = root_fs
                .lookup(unsafe { utils::raw_ptr_to_ref_str(kernel_vaddr as *const u8) })
                .expect("Error looking up file");

            let fds = a.get_file_descripter();
            let fd = fds.len();
            fds.push(file);
            Ok(fd)
        })
    }
}
