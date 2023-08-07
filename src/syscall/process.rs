use alloc::{string::String, vec::Vec};
use bitflags::bitflags;

use crate::{
    executor::util_futures::yield_now,
    fs::new_vfs::path::Path,
    memory::{address::VirtAddr, UserReadPtr, UserWritePtr},
    process::{
        self, lproc::ProcessStatus, lproc_mgr::GlobalLProcManager, pid::Pid,
        user_space::user_area::UserAreaPerm,
    },
    signal,
    tools::errors::{SysError, SysResult},
    when_debug,
};

use super::super::fs;
use super::{Syscall, SyscallResult};
use core::cmp::min;
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

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct WaitOptions: u32 {
        const WNOHANG = 0x00000001;
        const WUNTRACED = 0x00000002;
        const WCONTINUED = 0x00000004;
    }
}

impl<'a> Syscall<'a> {
    pub async fn sys_chdir(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let path = UserReadPtr::<u8>::from(args[0]);

        let path = path.read_cstr(&self.lproc)?;
        info!("Syscall: chdir: {}", path);

        let path = Path::from_string(path)?;

        // check whether the path is a directory
        let root_fs = fs::get_root_dir();
        let file = root_fs.resolve(&path).await?;
        if !file.is_dir().await? {
            return Err(SysError::ENOTDIR);
        }

        // change the cwd
        self.lproc.with_mut_fsinfo(|f| f.cwd = path);

        Ok(0)
    }

    pub fn sys_getcwd(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (buf, len) = (UserWritePtr::<u8>::from(args[0]), args[1]);

        info!("Syscall: getcwd: buf: {}, len: {}", buf, len);

        if len == 0 && buf.not_null() {
            return Err(SysError::EINVAL);
        }
        if buf.is_null() {
            return Err(SysError::EINVAL);
        }

        let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone()).to_string();

        if cwd.len() + 1 > len {
            // the spec said:
            //      If the length of the absolute pathname of the current working directory,
            //      including the terminating null byte, exceeds buflen bytes, NULL is returned,
            //      and errno shall be set to ERANGE.
            return Ok(0);
        }

        let length = min(cwd.len(), len);

        log::debug!("getcwd: cwd: '{}', min-len: {}", cwd, length);
        buf.as_mut_slice(length, &self.lproc)?
            .copy_from_slice(&cwd.as_bytes()[..length]);
        buf.add(length).write(&self.lproc, 0)?;

        Ok(buf.as_usize())
    }

    pub async fn sys_wait(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (pid, wstatus, options) = (
            args[0] as isize,
            UserWritePtr::<u32>::from(args[1]),
            args[2],
        );
        let options = WaitOptions::from_bits_truncate(options as u32);

        info!(
            "syscall: wait: pid: {}, &wstatus: {}, options: {:?}",
            pid, wstatus, options
        );

        if self.lproc.signal_pending().intersects(signal::SignalSet::SIGCHLD.complement()) {
            return Err(SysError::EINTR);
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

            log::trace!(
                "syscall wait: all children pids: {:?} (self: {:?})",
                self.lproc.children_pid_usize(),
                self.lproc.id()
            );

            when_debug!({
                if !stopped_children.is_empty() {
                    log::debug!(
                        "syscall wait: stopped children pids: {:?}",
                        stopped_children.iter().map(|lp| lp.id().into()).collect::<Vec<usize>>()
                    );
                }
            });

            // If WNOHANG is specified, return immediately if no child exited.
            if options.contains(WaitOptions::WNOHANG) && stopped_children.is_empty() {
                return Ok(0);
            }

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
                self.lproc.remove_child(child);
                // Reset SIGCHLC signal
                self.lproc.clear_signal(signal::SignalSet::SIGCHLD);
                break child.clone();
            }
        };

        if wstatus.not_null() {
            // 末尾 8 位是 SIG 信息，再上 8 位是退出码
            let status = (result_lproc.exit_code() as u32 & 0xff) << 8;
            debug!("wstatus: {:#x}", status);
            wstatus.write(&self.lproc, status)?;
        }

        Ok(result_lproc.id().into())
    }

    pub fn sys_clone(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (flags, child_stack, parent_tid_ptr, child_tid_ptr, _new_thread_local_storage_ptr) =
            (args[0] as u32, args[1], args[2], args[3], args[4]);

        let flags = CloneFlags::from_bits(flags & !0xff).ok_or(SysError::EINVAL)?;

        info!(
            "syscall: clone: flags: {:?}, child_stack: {:#x}, parent_tid_ptr: {:#x}, child_tid_ptr: {:#x}",
            flags, child_stack, parent_tid_ptr, child_tid_ptr
        );

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
            warn!("clear child tid, wait for signal subsystem");
        }

        let checked_write_u32 = |ptr: usize, value| -> SysResult<()> {
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

        if flags.contains(CloneFlags::CHILD_SETTID) && child_tid_ptr != 0 {
            let tid = Into::<usize>::into(new_lproc.id()) as u32;
            checked_write_u32(child_tid_ptr, tid)?;
        }

        if flags.contains(CloneFlags::PARENT_SETTID) && parent_tid_ptr != 0 {
            let parent_tid = Into::<usize>::into(new_lproc.parent_id()) as u32;
            checked_write_u32(parent_tid_ptr, parent_tid)?;
        }

        if flags.contains(CloneFlags::SETTLS) {
            // tp currently is used for hartid
            // new_lproc.context().set_user_tp(new_thread_local_storage_ptr);
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
            UserReadPtr::<u8>::from(args[0]),
            UserReadPtr::<usize>::from(args[1]),
            UserReadPtr::<usize>::from(args[2]),
        );

        let path_str = path.read_cstr(&self.lproc)?;
        let path = Path::from_string(path_str)?;
        let path = if path.is_absolute() {
            path
        } else {
            self.lproc.with_fsinfo(|f| f.cwd.append(&path))
        };
        let filename = path.last().clone();

        let read_2d_cstr = |mut ptr2d: UserReadPtr<usize>| -> SysResult<Vec<String>> {
            let mut result = Vec::new();

            loop {
                let ptr = ptr2d.read(&self.lproc)?;
                if ptr == 0 {
                    break;
                }
                let str = UserReadPtr::from(ptr).read_cstr(&self.lproc)?;
                result.push(str);
                ptr2d = ptr2d.add(1);
            }

            Ok(result)
        };

        let mut argv = read_2d_cstr(argv)?;
        let mut envp = read_2d_cstr(envp)?;

        info!(
            "syscall: execve: path: {:?}, argv: {:?}, envp: {:?}",
            path, argv, envp
        );
        log::debug!("syscall: execve: pid: {:?}", self.lproc.id());

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
            fs::get_root_dir().lookup("busybox").await?
        } else {
            fs::get_root_dir().resolve(&path).await?
        };

        self.lproc.do_exec(file, argv, envp).await;
        self.lproc.with_mut_procfs_info(|info| {
            info.exe_path = Some(path);
        });
        Ok(0)
    }

    pub fn sys_getpid(&mut self) -> SyscallResult {
        info!("Syscall: getpid");
        Ok(self.lproc.with_group(|g| g.tgid()).into())
    }

    pub fn sys_gettid(&mut self) -> SyscallResult {
        info!("Syscall: gettid");
        Ok(self.lproc.id().into())
    }

    pub fn sys_getppid(&mut self) -> SyscallResult {
        info!("Syscall: getppid");
        Ok(self.lproc.parent_id().into())
    }

    pub fn sys_exit(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        info!("Syscall: exit (code: {})", args[0]);
        self.do_exit = true;
        self.lproc.set_exit_code(args[0] as i32);
        Ok(0)
    }

    pub fn sys_set_tid_address(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        info!("Syscall: set_tid_address");
        self.lproc.with_mut_private_info(|i| i.clear_child_tid = Some(args[0]));

        let tid: usize = self.lproc.id().into();
        Ok(tid)
    }

    pub fn sys_getrlimit(&self) -> SyscallResult {
        info!("Syscall: getrlimit");
        let args = self.cx.syscall_args();
        info!("type: {}", args[0]);
        Ok(0)
    }

    pub fn sys_prlimit(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (pid, res, new_limit, old_limit) = (
            Pid::from(args[0]),
            RLimitResource::from_usize(args[1]),
            UserReadPtr::<RLimit>::from(args[2]),
            UserWritePtr::<RLimit>::from(args[3]),
        );

        info!(
            "Syscall: prlimit, pid: {:?}, res: {:?}, new_limit: {}, old_limit: {}",
            pid, res, new_limit, old_limit
        );

        let target_lproc = if pid == 0 {
            self.lproc.clone()
        } else {
            GlobalLProcManager::get(pid).ok_or(SysError::ESRCH)?
        };

        let res = res.ok_or(SysError::EINVAL)?;

        if old_limit.not_null() {
            let limit = match res {
                RLimitResource::NOFILE => target_lproc.with_fdtable(|f| f.get_limit()) as u64,
                _ => {
                    // not impl yet, just return 0
                    log::warn!("prlimit(get): not impl for {:?} yet, use INF", res);
                    RL_INFINITY
                }
            };
            old_limit.write(&self.lproc, RLimit::default(limit))?;
        }

        if new_limit.not_null() {
            let limit = new_limit.read(&self.lproc)?.cur;
            match res {
                RLimitResource::NOFILE => {
                    target_lproc.with_mut_fdtable(|f| f.set_limit(limit as usize))
                }
                _ => {
                    log::warn!("prlimit(set): not impl for {:?} yet, do nothing", res);
                }
            }
        }

        Ok(0)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct RLimit {
    cur: u64,
    max: u64,
}

impl RLimit {
    pub const fn default(cur: u64) -> Self {
        Self { cur, max: cur }
    }
}

pub const RL_INFINITY: u64 = -1i64 as u64;

#[derive(Debug, Clone, Copy)]
enum RLimitResource {
    /// Per-process CPU limit, in seconds.
    CPU = 0,
    /// Largest file that can be created, in bytes.
    FSIZE = 1,
    /// Maximum size of data segment, in bytes.
    DATA = 2,
    /// Maximum size of stack segment, in bytes.
    STACK = 3,
    /// Largest core file that can be created, in bytes.
    CORE = 4,
    /// Largest resident set size, in bytes.
    RSS = 5,
    /// Number of open files.
    NOFILE = 7,
    /// Address space limit.
    AS = 9,
    /// Number of processes.
    NPROC = 6,
    /// Locked-in-memory address space.
    MEMLOCK = 8,
    /// Maximum number of file locks.
    LOCKS = 10,
    /// Maximum number of pending signals.
    SIGPENDING = 11,
    /// Maximum bytes in POSIX message queues.
    MSGQUEUE = 12,
    /// Maximum nice priority allowed to raise to.
    NICE = 13,
    /// Maximum realtime priority allowed for non-priviledged processes.
    RTPRIO = 14,
    /// Maximum CPU time in µs that a process scheduled under a real-time
    RTTIME = 15,
    /// Maximum number of bytes in FUSE requests.
    NLIMITS = 16,
}

impl RLimitResource {
    pub fn from_usize(v: usize) -> Option<Self> {
        match v {
            0 => Some(RLimitResource::CPU),
            1 => Some(RLimitResource::FSIZE),
            2 => Some(RLimitResource::DATA),
            3 => Some(RLimitResource::STACK),
            4 => Some(RLimitResource::CORE),
            5 => Some(RLimitResource::RSS),
            7 => Some(RLimitResource::NOFILE),
            9 => Some(RLimitResource::AS),
            6 => Some(RLimitResource::NPROC),
            8 => Some(RLimitResource::MEMLOCK),
            10 => Some(RLimitResource::LOCKS),
            11 => Some(RLimitResource::SIGPENDING),
            12 => Some(RLimitResource::MSGQUEUE),
            13 => Some(RLimitResource::NICE),
            14 => Some(RLimitResource::RTPRIO),
            15 => Some(RLimitResource::RTTIME),
            16 => Some(RLimitResource::NLIMITS),
            _ => None,
        }
    }
}
