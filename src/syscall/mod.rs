use crate::{
    axerrno::AxError,
    process::process::{ProcessInfo, ThreadInfo},
    trap::context::UKContext,
};
use log::debug;
use log::info;

mod fs;
mod memory;

pub struct Syscall<'a> {
    cx: &'a mut UKContext,
    thread: &'a ThreadInfo,
    process: &'a ProcessInfo,
    do_exit: bool,
}

impl<'a> Syscall<'a> {
    pub fn new(cx: &'a mut UKContext, thread: &'a ThreadInfo, process: &'a ProcessInfo) -> Self {
        Self {
            cx,
            thread,
            process,
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
            // normal path
            SYSCALL_DBG_1 => self.sys_dbg_1().await,
            SYSCALL_DBG_2 => self.sys_dbg_2().await,
            // File related
            SYSCALL_GETCWD => todo!(),
            SYSCALL_PIPE2 => todo!(),
            SYSCALL_DUP => todo!(),
            SYSCALL_DUP3 => todo!(),
            SYSCALL_OPENAT => self.sys_openat(
                args[0] as i32,
                args[1] as *const u8,
                args[2] as u32,
                args[3] as i32,
            ),
            SYSCALL_CHDIR => todo!(),
            SYSCALL_CLOSE => self.sys_close(args[0]),
            SYSCALL_GETDENTS => todo!(),
            SYSCALL_READ => self.sys_read(args[0], args[1] as *mut u8, args[2]),
            SYSCALL_WRITE => self.sys_write(args[0], args[1] as *const u8, args[2]),
            SYSCALL_LINKAT => todo!(),
            SYSCALL_UNLINKAT => todo!(),
            SYSCALL_MKDIRAT => todo!(),
            SYSCALL_UMOUNT => todo!(),
            SYSCALL_MOUNT => todo!(),
            SYSCALL_FSTAT => self.sys_fstat(args[0], args[1] as *mut fs::Kstat),
            // Process related
            SYSCALL_CLONE => todo!(),
            SYSCALL_EXECVE => todo!(),
            SYSCALL_WAIT => todo!(),
            SYSCALL_EXIT => {
                self.do_exit = true;
                Ok(args[0])
            }
            SYSCALL_GETPPID => todo!(),
            SYSCALL_GETPID => self.sys_getpid(),
            // Memory related
            SYSCALL_BRK => self.sys_brk(args[0]),
            SYSCALL_MUNMAP => todo!(),
            SYSCALL_MMAP => self.sys_mmap(
                args[0],
                args[1],
                memory::MMAPPROT::from_bits(args[2] as u32).unwrap(),
                memory::MMAPFlags::from_bits(args[3] as u32).unwrap(),
                args[4] as i32,
                args[5],
            ),
            // Misc
            SYSCALL_TIMES => todo!(),
            SYSCALL_UNAME => todo!(),
            SYSCALL_SCHED_YIELD => todo!(),
            SYSCALL_GETTIMEOFDAY => todo!(),
            SYSCALL_NANOSLEEP => todo!(),
            _ => panic!("Unknown syscall_id: {}", syscall_no),
        };

        // 设置返回值
        // TODO: 设计 ENOSYS 之类的全局错误码信息供用户程序使用
        let ret = match result {
            Ok(ret) => ret,
            Err(_) => -1isize as usize,
        };

        debug!("Syscall ret: {}", ret);

        self.cx.set_user_a0(ret);
        self.do_exit
    }

    #[inline(always)]
    pub async fn sys_dbg_1(&mut self) -> SyscallResult {
        info!("Syscall: dbg_1");
        Ok(0)
    }

    #[inline(always)]
    pub async fn sys_dbg_2(&mut self) -> SyscallResult {
        info!("Syscall: dbg_2");
        Ok(0)
    }

    #[inline(always)]
    pub fn sys_getpid(&mut self) -> SyscallResult {
        info!("Syscall: getpid");
        Ok(self.process.pid().into())
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
