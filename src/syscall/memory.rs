//! Memory related syscall
//!

use super::{Syscall, SyscallResult};

impl<'a> Syscall<'a> {
    pub fn sys_brk(&mut self, brk: usize) -> SyscallResult {
        self.process.with_alive(|a| {
            let brk = a.get_user_space_mut().set_heap(brk.into());
            Ok(brk.into())
            // Allocation is not done here, so no OOM here
        })
    }
}
