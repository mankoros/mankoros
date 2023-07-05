use crate::executor::util_futures::yield_now;
use crate::memory::{UserReadPtr, UserWritePtr};
use crate::process::lproc::LightProcess;

use crate::timer::{TimeSpec, TimeVal, Tms};
use crate::{axerrno::AxError, syscall::misc::UtsName, trap::context::UKContext};

use log::debug;

mod fs;
mod memory;
mod misc;
mod process;

use alloc::sync::Arc;
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
        // 用作系统调用的 ecall 指令只能是 4 byte 长的, 它没有 C 扩展版本
        self.cx.set_user_pc_to_next(4);

        let syscall_no = self.cx.syscall_no();
        let args = self.cx.syscall_args();
        let result: SyscallResult = match syscall_no {
            // File related
            SYSCALL_GETCWD => self.sys_getcwd(),
            SYSCALL_PIPE2 => self.sys_pipe(),
            SYSCALL_DUP => self.sys_dup(),
            SYSCALL_DUP3 => self.sys_dup3(),
            SYSCALL_OPENAT => self.sys_openat(),
            SYSCALL_CHDIR => self.sys_chdir(),
            SYSCALL_CLOSE => self.sys_close(),
            SYSCALL_GETDENTS => self.sys_getdents(),
            SYSCALL_READ => self.sys_read().await,
            SYSCALL_WRITE => self.sys_write().await,
            SYSCALL_LINKAT => todo!(),
            SYSCALL_UNLINKAT => self.sys_unlinkat(),
            SYSCALL_MKDIRAT => self.sys_mkdir(),
            SYSCALL_UMOUNT => self.sys_umount(),
            SYSCALL_MOUNT => self.sys_mount(),
            SYSCALL_FSTAT => self.sys_fstat(),
            // Process related
            SYSCALL_CLONE => self.sys_clone(),
            SYSCALL_EXECVE => self.sys_execve(),
            SYSCALL_WAIT => self.sys_wait().await,
            SYSCALL_EXIT => self.sys_exit(),
            SYSCALL_GETPPID => self.sys_getppid(),
            SYSCALL_GETPID => self.sys_getpid(),
            SYSCALL_SET_TID_ADDRESS => self.sys_set_tid_address(),
            // Memory related
            SYSCALL_BRK => self.sys_brk(),
            SYSCALL_MUNMAP => self.sys_munmap(),
            SYSCALL_MMAP => self.sys_mmap(),
            // Misc
            SYSCALL_TIMES => self.sys_times(),
            SYSCALL_UNAME => self.sys_uname(),
            SYSCALL_SCHED_YIELD => self.sys_sched_yield().await,
            SYSCALL_GETTIMEOFDAY => self.sys_gettimeofday(),
            SYSCALL_NANOSLEEP => self.sys_nanosleep().await,
            SYSCALL_GETUID => self.sys_getuid(),
            _ => panic!("Unknown syscall_id: {}", syscall_no),
        };

        // 设置返回值
        // TODO: 设计 ENOSYS 之类的全局错误码信息供用户程序使用
        let ret = match result {
            Ok(ret) => ret,
            Err(_) => -1isize as usize,
        };

        debug!("Syscall ret: {:?}", result);

        self.cx.set_user_a0(ret);
        self.do_exit
    }
}

pub type SyscallResult = Result<usize, AxError>;

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
pub const SYSCALL_SENDFILE: usize = 71;
pub const SYSCALL_PSELECT6: usize = 72;
pub const SYSCALL_PPOLL: usize = 73;
pub const SYSCALL_READLINKAT: usize = 78;
pub const SYSCALL_NEWFSTATAT: usize = 79;
pub const SYSCALL_FSTAT: usize = 80;
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
pub const SYSCALL_SIGACTION: usize = 134;
pub const SYSCALL_SIGPROCMASK: usize = 135;
pub const SYSCALL_RT_SIGTIMEDWAIT: usize = 137;
pub const SYSCALL_SIGRETURN: usize = 139;
pub const SYSCALL_TIMES: usize = 153;
pub const SYSCALL_GETPGID: usize = 155;
pub const SYSCALL_UNAME: usize = 160;
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
