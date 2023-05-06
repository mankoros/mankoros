use bitflags::bitflags;

use crate::axerrno::AxError;

use super::{Syscall, SyscallResult};

bitflags! {
    pub struct CloneFlags: u32 {
        /* 共享内存 */
        const VM = 0x0000100;
        /* 共享文件系统信息 */
        const FS = 0x0000200;
        /* 共享已打开的文件 */
        const FILES = 0x0000400;
        /* 共享信号处理句柄 */
        const SIGHAND = 0x00000800;
        /* 共享 parent (新旧 task 的 getppid 返回结果相同) */
        const PARENT = 0x00008000;
        /* 新旧 task 置于相同线程组 */
        const THREAD = 0x00010000;
        /* share system V SEM_UNDO semantics */
        const SYSVSEM = 0x00040000;
        /* create a new TLS for the child */
        const SETTLS = 0x00080000;
        /* set the TID in the parent */
        const PARENT_SETTID = 0x00100000;
        /* clear the TID in the child */
        const CHILD_CLEARTID = 0x00200000;
        /* Unused, ignored */
        const CLONE_DETACHED = 0x00400000;
        /* set the TID in the child */
        const SETTID = 0x01000000;
    }
}

impl<'a> Syscall<'a> {
    pub fn sys_clone(
        &mut self,
        flags: CloneFlags,
        child_stack: usize,
        parent_tid_ptr: usize,
        child_tid_ptr: usize,
        new_thread_local_storage_ptr: usize,
    ) -> SyscallResult {
        todo!()
    }
}
