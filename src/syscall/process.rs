use alloc::{string::String, vec::Vec};
use bitflags::bitflags;

use crate::{
    executor::util_futures::yield_now,
    memory::address::VirtAddr,
    process::{self, lproc::ProcessStatus, user_space::user_area::UserAreaPerm},
    signal,
    tools::{user_check::UserCheck, errors::{SysError, SysResult}}, fs::new_vfs::path::Path,
};

use super::super::fs;
use super::{Syscall, SyscallResult};
use log::{debug, info, warn};

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
        /* create a new TLS for the child */
        const SETTLS = 0x00080000;
        /* set the TID in the parent */
        const PARENT_SETTID = 0x00100000;
        /* clear the TID in the child */
        const CHILD_CLEARTID = 0x00200000;
        /* set the TID in the child */
        const CHILD_SETTID = 0x01000000;
    }
}

impl<'a> Syscall<'a> {
    pub async fn sys_wait(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (pid, wstatus, options) = (args[0] as isize, args[1], args[2]);

        info!(
            "syscall: wait: pid: {}, &wstatus: {:x}, options: {}",
            pid, wstatus, options
        );

        if options != 0 {
            todo!("wait: options != 0");
        }

        let result_lproc = loop {
            yield_now().await;

            // Check if the child has exited.
            let stopped_children = self
                .lproc
                .children()
                .into_iter()
                .filter(|lp| lp.status() == ProcessStatus::STOPPED)
                .collect::<Vec<_>>();

            let target_child_opt = if pid < -1 {
                let target_tgid = -pid as usize;
                stopped_children.iter().find(|lp| lp.tgid() == target_tgid)
            } else if pid == -1 {
                stopped_children.last()
            } else if pid == 0 {
                let target_tgid = self.lproc.tgid();
                stopped_children.iter().find(|lp| lp.tgid() == target_tgid)
            } else {
                debug_assert!(pid > 0);
                let pid = pid as usize;
                stopped_children.iter().find(|lp| lp.id() == pid)
            };

            if let Some(child) = target_child_opt {
                self.lproc.clone().remove_child(&child.clone());
                // Reset SIGCHLC signal
                self.lproc.clone().clear_signal(signal::SignalSet::SIGCHLD);
                break child.clone();
            }
        };

        let wstatus = wstatus as *mut u32;
        if !wstatus.is_null() {
            // 末尾 8 位是 SIG 信息，再上 8 位是退出码
            let status = ((result_lproc.exit_code() as u32 & 0xff) << 8) | 0x00;
            debug!("wstatus: {:#x}", status);
            let user_check = UserCheck::new_with_sum(&self.lproc);
            user_check.checked_write(wstatus, status)?;
        }

        Ok(result_lproc.id().into())
    }

    pub fn sys_clone(&mut self) -> SyscallResult {
        info!("syscall: clone");
        let args = self.cx.syscall_args();
        let (flags, child_stack, parent_tid_ptr, child_tid_ptr, new_thread_local_storage_ptr) =
            (args[0] as u32, args[1], args[2], args[3], args[4]);

        let flags = CloneFlags::from_bits(flags & !0xff).ok_or(SysError::EINVAL)?;

        debug!("clone flags: {:#?}", flags);

        let stack_begin = if child_stack != 0 {
            if child_stack % 16 != 0 {
                warn!("child stack is not aligned: {:#x}", child_stack);
                // TODO: 跟组委会确认这种情况是不是要返回错误
                // return Err(AxError::InvalidInput);
                Some(child_stack - 8).map(VirtAddr::from)
            } else {
                Some(child_stack).map(VirtAddr::from)
            }
        } else {
            None
        };

        let old_lproc = self.lproc.clone();
        let new_lproc = old_lproc.do_clone(flags, stack_begin);

        if flags.contains(CloneFlags::CHILD_CLEARTID) {
            todo!("clear child tid, wait for signal subsystem");
        }

        let checked_write_u32 = |ptr, value| -> SysResult<()> {
            let vaddr = VirtAddr::from(ptr);
            let writeable = new_lproc.with_memory(|m| m.has_perm(vaddr, UserAreaPerm::WRITE));

            if !writeable {
                // todo: is that right?
                return Err(SysError::EPERM);
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
        debug!("Spawning new process with tid {:?}", new_proc_tid);
        process::spawn_proc(new_lproc);
        Ok(new_proc_tid.into())
    }

    pub async fn sys_execve(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (path, argv, envp) = (
            args[0],
            args[1],
            args[2],
        );

        let user_check = UserCheck::new_with_sum(&self.lproc);

        let path_str = user_check.checked_read_cstr(path as *const u8)?;
        let path = Path::from_string(path_str)?;
        let filename = path.last().clone();

        let mut argv = user_check.checked_read_2d_cstr(argv as *const *const u8)?;
        let mut envp = user_check.checked_read_2d_cstr(envp as *const *const u8)?;

        drop(user_check);
        info!(
            "syscall: execve: path: {:?}, argv: {:?}, envp: {:?}",
            path, argv, envp
        );

        // 不知道为什么要加，从 Oops 抄过来的
        envp.push(String::from("LD_LIBRARY_PATH=."));
        envp.push(String::from("SHELL=/busybox"));
        envp.push(String::from("PWD=/"));
        envp.push(String::from("USER=root"));
        envp.push(String::from("MOTD_SHOWN=pam"));
        envp.push(String::from("LANG=C.UTF-8"));
        envp.push(String::from(
            "INVOCATION_ID=e9500a871cf044d9886a157f53826684",
        ));
        envp.push(String::from("TERM=vt220"));
        envp.push(String::from("SHLVL=2"));
        envp.push(String::from("JOURNAL_STREAM=8:9265"));
        envp.push(String::from("OLDPWD=/root"));
        envp.push(String::from("_=busybox"));
        envp.push(String::from("LOGNAME=root"));
        envp.push(String::from("HOME=/"));
        envp.push(String::from("PATH=/"));

        let file = if filename.ends_with(".sh") {
            argv.insert(0, String::from("busybox"));
            argv.insert(1, String::from("sh"));
            fs::root::get_root_dir().lookup("busybox").await?
        } else {
            fs::root::get_root_dir().resolve(&path).await?
        };

        self.lproc.clone().do_exec(file, argv, envp);
        Ok(0)
    }

    pub fn sys_getpid(&mut self) -> SyscallResult {
        info!("Syscall: getpid");
        Ok(self.lproc.id().into())
    }

    pub fn sys_getppid(&mut self) -> SyscallResult {
        info!("Syscall: getppid");
        Ok(self.lproc.parent_id().into())
    }

    pub fn sys_exit(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        info!("Syscall: exit");
        self.do_exit = true;
        self.lproc.set_exit_code(args[0] as i32);
        Ok(0)
    }
}
