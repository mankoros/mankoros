use alloc::vec::Vec;
use bitflags::bitflags;

use crate::{
    arch::within_sum,
    axerrno::AxError,
    executor::yield_future::yield_now,
    memory::address::VirtAddr,
    process::{self, lproc::ProcessStatus, user_space::user_area::UserAreaPerm},
    signal,
};

use super::{Syscall, SyscallResult};
use log::{debug, warn};

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
        const CHILD_SETTID = 0x01000000;
    }
}

impl<'a> Syscall<'a> {
    pub async fn sys_wait(
        &mut self,
        _pid: usize,
        wstatus: usize, // TODO: not sure
        _options: usize,
    ) -> SyscallResult {
        debug!("syscall: wait");
        let pid = loop {
            yield_now().await;
            // Check if the child has exited.
            if let Some(child) = self
                .lproc
                .children()
                .into_iter()
                .filter(|lp| lp.status() == ProcessStatus::STOPPED)
                .collect::<Vec<_>>()
                .first()
            {
                self.lproc.clone().remove_child(&child.clone());
                // Reset SIGCHLC signal
                self.lproc.clone().clear_signal(signal::SignalSet::SIGCHLD);
                break child.id();
            }
        };
        if wstatus != 0 {
            // No write to NULL
            let wstatus = wstatus as *mut usize;
            within_sum(|| {
                unsafe { *wstatus = pid.into() };
            });
        }
        Ok(pid.into())
    }

    pub fn sys_clone(
        &mut self,
        flags: u32,
        child_stack: usize,
        parent_tid_ptr: usize,
        child_tid_ptr: usize,
        new_thread_local_storage_ptr: usize,
    ) -> SyscallResult {
        debug!("syscall: clone");

        let flags = CloneFlags::from_bits(flags & !0xff).ok_or(AxError::InvalidInput)?;

        debug!("clone flags: {:#?}", flags);

        let stack_begin = 
            if child_stack != 0 {
                if child_stack % 16 != 0 {
                    warn!("child stack is not aligned: {:#x}", child_stack);
                    // TODO: 跟组委会确认这种情况是不是要返回错误
                    // return Err(AxError::InvalidInput);
                    Some((child_stack - 8).into())
                } else {
                    Some(child_stack.into()) 
                }
            } else {
                None
            };
        
        let old_lproc = self.lproc.clone();
        let new_lproc = old_lproc.do_clone(flags, stack_begin);

        if flags.contains(CloneFlags::CHILD_CLEARTID) {
            todo!("clear child tid, wait for signal subsystem");
        }

        let checked_write_u32 = |ptr, value| -> Result<(), AxError> {
            let vaddr = VirtAddr(ptr);
            let writeable = new_lproc.with_memory(|m| m.has_perm(vaddr, UserAreaPerm::WRITE));

            if !writeable {
                // todo: is that right?
                return Err(AxError::PermissionDenied);
            }
            unsafe {
                let ctptr = &mut *(vaddr.as_mut_ptr() as *mut u32);
                *ctptr = value;
            }

            Ok(())
        };

        if flags.contains(CloneFlags::CHILD_SETTID) {
            let tid = Into::<usize>::into(new_lproc.id()) as u32;
            checked_write_u32(child_tid_ptr, tid)?;
        }

        if flags.contains(CloneFlags::PARENT_SETTID) {
            let parent_tid = Into::<usize>::into(new_lproc.parent_id()) as u32;
            checked_write_u32(parent_tid_ptr, parent_tid)?;
        }

        if flags.contains(CloneFlags::SETTLS) {
            new_lproc.context().set_user_tp(new_thread_local_storage_ptr);
        }

        // syscall clone returns 0 in child process
        new_lproc.context().set_user_a0(0);

        // save the tid of the new process and add it to queue
        let new_proc_tid = new_lproc.id();
        debug!("Spawning new process with tid {:#?}", new_proc_tid);
        process::spawn_proc(new_lproc);
        Ok(new_proc_tid.into())
    }
}
