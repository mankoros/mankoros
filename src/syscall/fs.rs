//! Filesystem related syscall
//!

use log::debug;

use crate::{axerrno::AxError, memory::kernel_phys_to_virt};

use super::{Syscall, SyscallResult};

impl<'a> Syscall<'a> {
    pub fn sys_write(&mut self, fd: usize, buf: *const u8, len: usize) -> SyscallResult {
        self.process.with_alive(|a| {
            debug!("sys_write fd {fd}");

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
}
