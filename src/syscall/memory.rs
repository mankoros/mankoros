//! Memory related syscall
//!

use log::info;

use super::{Syscall, SyscallResult};

impl<'a> Syscall<'a> {
    pub fn sys_brk(&mut self, brk: usize) -> SyscallResult {
        info!("Syscall brk: brk {}", brk);
        self.process.with_alive(|a| {
            let brk = a.get_user_space_mut().set_heap(brk.into());
            Ok(brk.into())
            // Allocation is not done here, so no OOM here
        })
    }
}
