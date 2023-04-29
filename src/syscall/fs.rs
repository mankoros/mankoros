//! Filesystem related syscall
//!

use log::debug;

use super::SyscallResult;

pub fn sys_write(fd: usize, buf: *const u8, len: usize) -> SyscallResult {
    debug!("sys_write fd {fd}");

    Ok(len)
}
