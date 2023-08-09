mod fs;
mod io;
mod memory;
mod misc;
mod process;
mod signal;

use crate::tools::errors::SysResult;
use crate::trap::context::UKContext;
use crate::{process::lproc::LightProcess, tools::errors::SysError};
use alloc::sync::Arc;
use log::{info, warn};

use crate::executor::hart_local::AutoSUM;
pub use process::CloneFlags;

pub struct Syscall<'a> {
    cx: &'a mut UKContext,
    lproc: Arc<LightProcess>,
    do_exit: bool,
}

impl<'a> Syscall<'a> {
    pub fn new(cx: &'a mut UKContext, lproc: Arc<LightProcess>) -> Self {
        Self {
            cx,
            lproc,
            do_exit: false,
        }
    }

    #[inline(always)]
    pub async fn syscall(&mut self) -> bool {
        let _auto_sum = AutoSUM::new();

        // 用作系统调用的 ecall 指令只能是 4 byte 长的, 它没有 C 扩展版本
        self.cx.set_user_pc_to_next(4);

        let syscall_no = self.cx.syscall_no();
        let _args = self.cx.syscall_args();
        let result: SyscallResult = match syscall_no {
            // IO related
            SYSCALL_OPENAT => self.sys_openat().await,
            SYSCALL_PIPE2 => self.sys_pipe(),
            SYSCALL_DUP => self.sys_dup(),
            SYSCALL_DUP3 => self.sys_dup3(),
            SYSCALL_CLOSE => self.sys_close(),
            SYSCALL_READ => self.sys_read().await,
            SYSCALL_WRITE => self.sys_write().await,
            SYSCALL_PPOLL => self.sys_ppoll().await,
            SYSCALL_PSELECT => self.sys_pselect().await,
            SYSCALL_WRITEV => self.sys_writev().await,
            SYSCALL_READV => self.sys_readv().await,
            SYSCALL_LSEEK => self.sys_lseek().await,
            SYSCALL_PREAD => self.sys_pread().await,
            SYSCALL_PWRITE => self.sys_pwrite().await,
            SYSCALL_PREADV => self.sys_preadv().await,
            SYSCALL_PWRITEV => self.sys_pwritev().await,

            // FS related
            SYSCALL_NEWFSTAT => self.sys_fstat().await,
            SYSCALL_NEWFSTATAT => self.sys_fstatat().await,
            SYSCALL_GETDENTS => self.sys_getdents().await,
            SYSCALL_LINKAT => todo!(),
            SYSCALL_UNLINKAT => self.sys_unlinkat().await,
            SYSCALL_FCNTL => self.sys_fcntl(),
            SYSCALL_MKDIRAT => self.sys_mkdir().await,
            SYSCALL_UMOUNT => self.sys_umount().await,
            SYSCALL_MOUNT => self.sys_mount().await,
            SYSCALL_SYNC => self.sys_do_nothing("sync"),
            SYSCALL_FSYNC => self.sys_do_nothing("fsync"),
            SYSCALL_FTURNCATE => self.sys_fturncate().await,
            SYSCALL_READLINKAT => self.sys_readlinkat().await,
            SYSCALL_RENAMEAT2 => self.sys_renameat2().await,
            SYSCALL_UTIMENSAT => self.sys_utimensat().await,
            SYSCALL_FACCESSAT => self.sys_faccessat().await,
            SYSCALL_STATFS => self.sys_statfs().await,

            // Process related
            SYSCALL_GETCWD => self.sys_getcwd(),
            SYSCALL_CHDIR => self.sys_chdir().await,
            SYSCALL_CLONE => self.sys_clone(),
            SYSCALL_EXECVE => self.sys_execve().await,
            SYSCALL_WAIT => self.sys_wait().await,
            SYSCALL_EXIT => self.sys_exit(),
            SYSCALL_GETPPID => self.sys_getppid(),
            SYSCALL_GETPID => self.sys_getpid(),
            SYSCALL_GETTID => self.sys_gettid(),
            SYSCALL_SET_TID_ADDRESS => self.sys_set_tid_address(),
            SYSCALL_GETRLIMIT => self.sys_getrlimit(),
            SYSCALL_PRLIMIT => self.sys_prlimit(),
            // Signal system
            SYSCALL_RT_SIGTIMEDWAIT => self.sys_sigwait().await,
            SYSCALL_RT_SIGACTION => self.sys_sigaction(),
            SYSCALL_KILL => self.sys_kill(),

            // Memory related
            SYSCALL_BRK => self.sys_brk(),
            SYSCALL_MUNMAP => self.sys_munmap(),
            SYSCALL_MMAP => self.sys_mmap(),
            SYSCALL_MPROTECT => self.sys_do_nothing("mprotect"),
            SYSCALL_SHMGET => self.sys_shmget(),
            SYSCALL_SHMCTL => self.sys_shmctl(),
            SYSCALL_SHMAT => self.sys_shmat(),
            SYSCALL_SHMDT => self.sys_shmdt(),

            // Misc
            SYSCALL_TIMES => self.sys_times(),
            SYSCALL_UNAME => self.sys_uname(),
            SYSCALL_SCHED_YIELD => self.sys_sched_yield().await,
            SYSCALL_GETTIMEOFDAY => self.sys_gettimeofday(),
            SYSCALL_CLOCKGETTIME => self.sys_clockgettime(),
            SYSCALL_NANOSLEEP => self.sys_nanosleep().await,
            SYSCALL_GETUID => self.sys_getuid(),
            SYSCALL_GETRUSAGE => self.sys_getrusage(),
            SYSCALL_SYSLOG => self.sys_do_nothing("syslog"),
            SYSCALL_SETITIMER => self.sys_setitimer(),

            // unimplemented
            29 => self.sys_do_nothing("ioctl"),
            94 => self.sys_do_nothing("exit_group"),
            135 => self.sys_do_nothing("rt_sigprocmask"),
            155 => self.sys_do_nothing("getpgid"),
            154 => self.sys_do_nothing("setpgid"),
            166 => self.sys_do_nothing("umask"),
            175 => self.sys_do_nothing("geteuid"),
            176 => self.sys_do_nothing("getgid"),
            177 => self.sys_do_nothing("getegid"),

            _ => {
                warn!("Unknown syscall_id: {}", syscall_no);
                Err(SysError::EINVAL)
            }
        };

        // 设置返回值
        let ret = match result {
            Ok(ret) => ret,
            Err(err) => (-(err as isize)) as usize,
        };

        info!("Syscall {} ret: {:?}", self.cx.syscall_no(), result);

        self.cx.set_user_a0(ret);
        self.do_exit
    }

    fn sys_do_nothing(&self, name: &str) -> SyscallResult {
        log::info!(
            "Not implemented syscall that specified to do nothing (#{}, {})",
            self.cx.syscall_no(),
            name
        );
        Ok(0)
    }
}

pub type SyscallResult = SysResult<usize>;

// only for debug usage
pub const SYSCALL_DBG_1: usize = 0;
pub const SYSCALL_DBG_2: usize = 1;

// Syscall Numbers from Oops
pub const SYSCALL_GETCWD: usize = 17;
pub const SYSCALL_DUP: usize = 23;
pub const SYSCALL_DUP3: usize = 24;
pub const SYSCALL_FCNTL: usize = 25;
pub const SYSCALL_IOCTL: usize = 29;
pub const SYSCALL_MKDIRAT: usize = 34;
pub const SYSCALL_UNLINKAT: usize = 35;
pub const SYSCALL_LINKAT: usize = 37;
pub const SYSCALL_UMOUNT: usize = 39;
pub const SYSCALL_MOUNT: usize = 40;
pub const SYSCALL_STATFS: usize = 43;
pub const SYSCALL_FTURNCATE: usize = 46;
pub const SYSCALL_FACCESSAT: usize = 48;
pub const SYSCALL_CHDIR: usize = 49;
pub const SYSCALL_OPENAT: usize = 56;
pub const SYSCALL_CLOSE: usize = 57;
pub const SYSCALL_PIPE2: usize = 59;
pub const SYSCALL_GETDENTS: usize = 61;
pub const SYSCALL_LSEEK: usize = 62;
pub const SYSCALL_READ: usize = 63;
pub const SYSCALL_WRITE: usize = 64;
pub const SYSCALL_READV: usize = 65;
pub const SYSCALL_WRITEV: usize = 66;
pub const SYSCALL_PREAD: usize = 67;
pub const SYSCALL_PWRITE: usize = 68;
pub const SYSCALL_PREADV: usize = 69;
pub const SYSCALL_PWRITEV: usize = 70;
pub const SYSCALL_SENDFILE: usize = 71;
pub const SYSCALL_PSELECT: usize = 72;
pub const SYSCALL_PPOLL: usize = 73;
pub const SYSCALL_READLINKAT: usize = 78;
pub const SYSCALL_NEWFSTATAT: usize = 79;
pub const SYSCALL_NEWFSTAT: usize = 80;
pub const SYSCALL_SYNC: usize = 81;
pub const SYSCALL_FSYNC: usize = 82;
pub const SYSCALL_UTIMENSAT: usize = 88;
pub const SYSCALL_EXIT: usize = 93;
pub const SYSCALL_EXIT_GROUP: usize = 94;
pub const SYSCALL_SET_TID_ADDRESS: usize = 96;
pub const SYSCALL_FUTEX: usize = 98;
pub const SYSCALL_SET_ROBUST_LIST: usize = 99;
pub const SYSCALL_GET_ROBUST_LIST: usize = 100;
pub const SYSCALL_NANOSLEEP: usize = 101;
pub const SYSCALL_SETITIMER: usize = 103;
pub const SYSCALL_CLOCKGETTIME: usize = 113;
pub const SYSCALL_SYSLOG: usize = 116;
pub const SYSCALL_SCHED_YIELD: usize = 124;
pub const SYSCALL_KILL: usize = 129;
pub const SYSCALL_TKILL: usize = 130;
pub const SYSCALL_TGKILL: usize = 131;
pub const SYSCALL_RT_SIGACTION: usize = 134;
pub const SYSCALL_RT_SIGPROCMASK: usize = 135;
pub const SYSCALL_RT_SIGTIMEDWAIT: usize = 137;
pub const SYSCALL_RT_SIGRETURN: usize = 139;
pub const SYSCALL_TIMES: usize = 153;
pub const SYSCALL_GETPGID: usize = 155;
pub const SYSCALL_UNAME: usize = 160;
pub const SYSCALL_GETRLIMIT: usize = 163;
pub const SYSCALL_GETRUSAGE: usize = 165;
pub const SYSCALL_UMASK: usize = 166;
pub const SYSCALL_GETTIMEOFDAY: usize = 169;
pub const SYSCALL_GETPID: usize = 172;
pub const SYSCALL_GETPPID: usize = 173;
pub const SYSCALL_GETUID: usize = 174;
pub const SYSCALL_GETEUID: usize = 175;
pub const SYSCALL_GETEGID: usize = 177;
pub const SYSCALL_GETTID: usize = 178;
pub const SYSCALL_SYSINFO: usize = 179;
pub const SYSCALL_SHMGET: usize = 194;
pub const SYSCALL_SHMCTL: usize = 195;
pub const SYSCALL_SHMAT: usize = 196;
pub const SYSCALL_SHMDT: usize = 197;
pub const SYSCALL_SOCKET: usize = 198;
pub const SYSCALL_BIND: usize = 200;
pub const SYSCALL_LISTEN: usize = 201;
pub const SYSCALL_ACCEPT: usize = 202;
pub const SYSCALL_CONNECT: usize = 203;
pub const SYSCALL_GETSOCKNAME: usize = 204;
pub const SYSCALL_SENDTO: usize = 206;
pub const SYSCALL_RECVFROM: usize = 207;
pub const SYSCALL_SETSOCKOPT: usize = 208;
pub const SYSCALL_BRK: usize = 214;
pub const SYSCALL_MUNMAP: usize = 215;
pub const SYSCALL_CLONE: usize = 220;
pub const SYSCALL_EXECVE: usize = 221;
pub const SYSCALL_MMAP: usize = 222;
pub const SYSCALL_MPROTECT: usize = 226;
pub const SYSCALL_MSYNC: usize = 227;
pub const SYSCALL_MADVISE: usize = 233;
pub const SYSCALL_WAIT: usize = 260;
pub const SYSCALL_PRLIMIT: usize = 261;
pub const SYSCALL_RENAMEAT2: usize = 276;
pub const SYSCALL_MEMBARRIER: usize = 283;
pub const SYSCALL_STOP: usize = 998;
pub const SYSCALL_SHUTDOWN: usize = 999;
