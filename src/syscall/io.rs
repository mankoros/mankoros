use log::{debug, info};

use crate::{
    arch::within_sum,
    executor::util_futures::{within_sum_async, yield_now, AnyFuture},
    fs::{
        self,
        new_vfs::{
            path::Path,
            top::{PollKind, VfsFileRef},
            VfsFileKind,
        },
        pipe::Pipe,
    },
    memory::{address::VirtAddr, UserInOutPtr, UserReadPtr, UserWritePtr},
    syscall::fs::AT_FDCWD,
    tools::{
        errors::{dyn_future, Async, SysError, SysResult},
        user_check::UserCheck,
    },
};

use super::{Syscall, SyscallResult};
use alloc::{collections::BTreeMap, vec::Vec};

impl Syscall<'_> {
    pub async fn sys_write(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, buf, len) = (args[0], UserWritePtr::from_usize(args[1]), args[2]);

        info!("Syscall: write, fd {fd}, len: {len}");

        let buf = unsafe { core::slice::from_raw_parts(buf.raw_ptr(), len) };
        let fd = self.lproc.with_mut_fdtable(|f| f.get(fd));
        // TODO: is it safe ?
        if let Some(fd) = fd {
            let write_len = within_sum_async(fd.file.write_at(fd.curr(), buf)).await?;
            fd.add_curr(write_len);
            Ok(write_len)
        } else {
            Err(SysError::EBADF)
        }
    }
    pub async fn sys_read(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, buf, len) = (args[0], UserWritePtr::from_usize(args[1]), args[2]);

        info!(
            "Syscall: read, fd {}, buf: {:x}, len: {}",
            fd,
            buf.as_usize(),
            len
        );

        // *mut u8 does not implement Send
        let buf = unsafe { core::slice::from_raw_parts_mut(buf.raw_ptr_mut(), len) };

        let fd = self.lproc.with_mut_fdtable(|f| f.get(fd));
        if let Some(fd) = fd {
            let read_len = within_sum_async(fd.file.read_at(fd.curr(), buf)).await?;
            if args[0] == 0 && read_len == 1 {
                within_sum(|| {
                    // '\r' -> '\n'
                    if buf[0] == 0xd {
                        buf[0] = 0xa;
                        log::warn!("replace \\r -> \\n")
                    }
                })
            }
            fd.add_curr(read_len);
            Ok(read_len)
        } else {
            Err(SysError::EBADF)
        }
    }

    pub async fn sys_openat(&mut self) -> SyscallResult {
        // TODO: refactor using `at_helper`
        let args = self.cx.syscall_args();
        let (dir_fd, path, raw_flags, _user_mode) =
            (args[0], args[1], args[2] as u32, args[3] as i32);

        info!("Syscall: openat");

        // Parse flags
        let flags = OpenFlags::from_bits_truncate(raw_flags);

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path as *const u8)?;

        info!("Open path: {}", path);
        let path = Path::from_string(path).expect("Error parsing path");

        let dir = if path.is_absolute() {
            fs::root::get_root_dir()
        } else if dir_fd == AT_FDCWD {
            let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone());
            fs::root::get_root_dir().resolve(&cwd).await?
        } else {
            let file = self
                .lproc
                .with_mut_fdtable(|f| f.get(dir_fd))
                .map(|fd| fd.file.clone())
                .ok_or(SysError::EBADF)?;
            if file.attr().await?.kind != VfsFileKind::Directory {
                return Err(SysError::ENOTDIR);
            }
            file
        };

        let file = match dir.resolve(&path).await {
            Ok(file) => file,
            Err(SysError::ENOENT) => {
                // Check if CREATE flag is set
                if !flags.contains(OpenFlags::CREATE) {
                    return Err(SysError::ENOENT);
                }
                // Create file
                // 1. ensure file dir exists
                let (dir_path, file_name) = path.split_dir_file();
                let direct_dir = dir.resolve(&dir_path).await?;
                // 2. create file

                direct_dir.create(&file_name, VfsFileKind::RegularFile).await?
            }
            Err(e) => {
                return Err(e);
            }
        };

        self.lproc.with_mut_fdtable(|f| Ok(f.alloc(file)))
    }

    /// 创建管道，在 *pipe 记录读管道的 fd，在 *(pipe+1) 记录写管道的 fd。
    /// 成功时返回 0，失败则返回 -1
    pub fn sys_pipe(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let pipe = UserWritePtr::from(args[0]);

        info!("Syscall: pipe");
        let (pipe_read, pipe_write) = Pipe::new_pipe();
        let _user_check = UserCheck::new_with_sum(&self.lproc);

        self.lproc.with_mut_fdtable(|table| {
            let read_fd = table.alloc(VfsFileRef::new(pipe_read));
            let write_fd = table.alloc(VfsFileRef::new(pipe_write));

            info!("read_fd: {}, write_fd: {}", read_fd, write_fd);

            // TODO: check user permissions
            unsafe { *pipe.raw_ptr_mut() = read_fd as u32 }
            unsafe { *pipe.raw_ptr_mut().add(1) = write_fd as u32 }
        });
        Ok(0)
    }

    pub fn sys_close(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let fd = args[0];
        info!("Syscall: close: fd: {}", fd);

        let fd_opt = self.lproc.with_mut_fdtable(|f| f.remove(fd));
        if let Some(_) = fd_opt {
            // close() returns zero on success.  On error, -1 is returned
            // https://man7.org/linux/man-pages/man2/close.2.html
            Ok(0)
        } else {
            Err(SysError::EBADF)
        }
    }

    pub async fn sys_lseek(&mut self) -> SyscallResult {
        const SEEK_SET: usize = 0; /* Seek from beginning of file.  */
        const SEEK_CUR: usize = 1; /* Seek from current position.  */
        const SEEK_END: usize = 2; /* Seek from end of file.  */

        let args = self.cx.syscall_args();
        let (fd, offset, whence) = (args[0], args[1], args[2]);

        let whence_str = match whence {
            SEEK_SET => "SEEK_SET",
            SEEK_CUR => "SEEK_CUR",
            SEEK_END => "SEEK_END",
            _ => "UNKNOWN",
        };
        info!(
            "Syscall: lseek: fd: {}, offset: {}, whence: {}",
            fd, offset, whence_str
        );

        let fd = self.lproc.with_fdtable(|f| f.get(fd)).ok_or(SysError::EBADF)?;
        match whence {
            SEEK_SET => {
                fd.set_curr(offset);
            }
            SEEK_CUR => {
                let result = (fd.curr() as isize) + (offset as isize);
                log::info!("SEEK_CUR: {result}");
                fd.set_curr(result as usize);
            }
            SEEK_END => {
                let size = fd.file.attr().await?.byte_size;
                let offset = (size as isize) + (offset as isize);
                log::info!("SEEK_END: {offset}");
                fd.set_curr(offset as usize);
            }
            _ => {
                return Err(SysError::EINVAL);
            }
        }

        Ok(fd.curr())
    }

    pub async fn sys_ppoll(&mut self) -> SyscallResult {
        #[repr(C)]
        #[derive(Debug, Copy, Clone)]
        struct PollFd {
            // int   fd;         /* file descriptor */
            // short events;     /* requested events */
            // short revents;    /* returned events */
            fd: i32,
            events: i16,
            revents: i16,
        }

        impl PollFd {
            pub const POLLIN: i16 = 0x001;
            pub const POLLPRI: i16 = 0x002;
            pub const POLLOUT: i16 = 0x004;
            pub const POLLERR: i16 = 0x008;
            pub const POLLHUP: i16 = 0x010;
            pub const POLLNVAL: i16 = 0x020;
        }

        let args = self.cx.syscall_args();
        let (fds, nfds, timeout_ts, _sigmask) = (
            UserReadPtr::<PollFd>::from_usize(args[0]),
            args[1],
            args[2],
            args[3],
        );

        info!(
            "Syscall: ppoll, fds: 0x{:x}, nfds: {}, timeout_ts: {}",
            fds.as_usize(),
            nfds,
            timeout_ts,
        );

        let user_check = UserCheck::new_with_sum(&self.lproc);
        // future_idx -> (fd_idx, event)
        let mut mapping = BTreeMap::<usize, (usize, i16)>::new();
        let mut futures = Vec::<Async<SysResult<usize>>>::new();

        let mut poll_fd_ptr = fds;
        for i in 0..nfds {
            let poll_fd = user_check.checked_read(poll_fd_ptr.raw_ptr())?;
            let fd = poll_fd.fd as usize;
            debug!("ppoll on fd: {}", fd);
            let events = poll_fd.events;
            let fd = self.lproc.with_fdtable(|f| f.get(fd)).ok_or(SysError::EBADF)?;

            if events & PollFd::POLLIN != 0 {
                // 使用一个新的 Future 将 file 的所有权移动进去并保存, 以供给 poll_ready 使用
                // TODO: 是否需要更加仔细地考虑 VfsFile 上方法对 self 的占有方式?
                let copy_fd = fd.clone();
                let future =
                    async move { copy_fd.file.poll_ready(copy_fd.curr(), 1, PollKind::Read).await };
                futures.push(dyn_future(future));
                mapping.insert(futures.len() - 1, (i, PollFd::POLLIN));
            }
            if events & PollFd::POLLOUT != 0 {
                let copy_fd = fd.clone();
                let future = async move {
                    copy_fd.file.poll_ready(copy_fd.curr(), 1, PollKind::Write).await
                };
                futures.push(dyn_future(future));
                mapping.insert(futures.len() - 1, (i, PollFd::POLLOUT));
            }
            if events & !(PollFd::POLLIN | PollFd::POLLOUT) != 0 {
                log::warn!("Unsupported poll event: {}", events);
            }
            poll_fd_ptr = poll_fd_ptr.add(1);
        }

        let (future_id, _) = AnyFuture::new_with(futures).await;
        let (fd_idx, event) = mapping.remove(&future_id).unwrap();

        let ready_poll_fd_ptr = fds.add(fd_idx);
        let mut ready_poll_fd_value = user_check.checked_read(ready_poll_fd_ptr.raw_ptr())?;
        ready_poll_fd_value.revents = match event {
            PollFd::POLLIN => PollFd::POLLIN,
            PollFd::POLLOUT => PollFd::POLLOUT,
            _ => unreachable!(),
        };
        debug!("poll fd: {:?}", ready_poll_fd_value);
        user_check.checked_write(ready_poll_fd_ptr.raw_ptr() as *mut _, ready_poll_fd_value)?;

        // Return value is the number of file descriptors that were ready.
        // Currently, this is always 1.
        Ok(1)
    }

    pub async fn sys_pselect(&mut self) -> SyscallResult {
        #[repr(C)]
        #[derive(Debug, Copy, Clone)]
        struct FdSet {
            fds_bits: [u64; 1024 / 64],
        }
        impl FdSet {
            pub fn zero() -> Self {
                Self {
                    fds_bits: [0; 1024 / 64],
                }
            }

            pub fn clear(&mut self) {
                for i in 0..self.fds_bits.len() {
                    self.fds_bits[i] = 0;
                }
            }

            pub fn set(&mut self, fd: usize) {
                let idx = fd / 64;
                let bit = fd % 64;
                let mask = 1 << bit;
                self.fds_bits[idx] |= mask;
            }

            pub fn is_set(&self, fd: usize) -> bool {
                let idx = fd / 64;
                let bit = fd % 64;
                let mask = 1 << bit;
                self.fds_bits[idx] & mask != 0
            }
        }

        let args = self.cx.syscall_args();
        let (maxfdp1, readfds_ptr, writefds_ptr, exceptfds_ptr, tsptr, _sigmask) = (
            args[0],
            UserInOutPtr::<FdSet>::from_usize(args[1]),
            UserInOutPtr::<FdSet>::from_usize(args[2]),
            UserInOutPtr::<FdSet>::from_usize(args[3]),
            args[4],
            args[5],
        );

        info!(
            "Syscall: pselect, maxfdp1: {}, readfds: 0x{:x}, writefds: 0x{:x}, exceptfds: 0x{:x}, tsptr: {:x}, sigmask: {}",
            maxfdp1,
            readfds_ptr.as_usize(),
            writefds_ptr.as_usize(),
            exceptfds_ptr.as_usize(),
            tsptr,
            _sigmask,
        );

        if maxfdp1 == 0 {
            // avoid reading read/write/except fds when maxfdp1 is 0
            if tsptr != 0 {
                // when all fds are empty, we should sleep for the time specified by tsptr
                // if not sleep, we may starvate other processes
                yield_now().await;
            }
            return Ok(0);
        }

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let mut readfds = user_check.checked_read(readfds_ptr.raw_ptr())?;
        let mut writefds = user_check.checked_read(writefds_ptr.raw_ptr())?;

        // future_idx -> (fd_idx, event)
        let mut mapping = BTreeMap::<usize, (usize, PollKind)>::new();
        let mut futures = Vec::<Async<SysResult<usize>>>::new();
        for fd in 0..maxfdp1 {
            let fd_file = self.lproc.with_fdtable(|f| f.get(fd as usize));
            let fd_file = match fd_file {
                Some(f) => f,
                None => continue,
            };

            if readfds.is_set(fd) {
                let copy_fd = fd_file.clone();
                let future =
                    async move { copy_fd.file.poll_ready(copy_fd.curr(), 1, PollKind::Read).await };
                futures.push(dyn_future(future));
                mapping.insert(futures.len() - 1, (fd as usize, PollKind::Read));
            }
            if writefds.is_set(fd) {
                let copy_fd = fd_file.clone();
                let future = async move {
                    copy_fd.file.poll_ready(copy_fd.curr(), 1, PollKind::Write).await
                };
                futures.push(dyn_future(future));
                mapping.insert(futures.len() - 1, (fd as usize, PollKind::Write));
            }
        }

        readfds.clear();
        writefds.clear();

        let (future_id, _) = AnyFuture::new_with(futures).await;
        let (fd_idx, event) = mapping.remove(&future_id).unwrap();

        match event {
            PollKind::Read => readfds.set(fd_idx),
            PollKind::Write => writefds.set(fd_idx),
        }

        user_check.checked_write(readfds_ptr.raw_ptr_mut(), readfds)?;
        user_check.checked_write(writefds_ptr.raw_ptr_mut(), writefds)?;
        user_check.checked_write(exceptfds_ptr.raw_ptr_mut(), FdSet::zero())?;

        Ok(1)
    }

    pub async fn sys_writev(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, iov, iovcnt) = (args[0], UserReadPtr::<IoVec>::from(args[1]), args[2]);

        info!(
            "Syscall: writev, fd: {}, iov: 0x{:x}, iovcnt: {}",
            fd,
            iov.as_usize(),
            iovcnt
        );

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let fd = self.lproc.with_fdtable(|f| f.get(fd)).ok_or(SysError::EBADF)?;
        let file = fd.file.clone();

        let mut offset = fd.curr();
        let mut total_len = 0;
        let mut iov_ptr = iov;
        for i in 0..iovcnt {
            let iov = user_check.checked_read(iov_ptr.raw_ptr())?;
            log::debug!(
                "syscall writev: iov #{}: iov_ptr: 0x{:x}, len: {}",
                i,
                iov_ptr.as_usize(),
                iov.len
            );
            // TODO: 检查用户给的指针是不是合法的
            let buf = unsafe { VirtAddr::from(iov.base).as_slice(iov.len) };
            let write_len = file.write_at(offset, buf).await?;
            total_len += write_len;
            offset += write_len;
            iov_ptr = iov_ptr.add(1);
        }

        fd.add_curr(total_len);
        Ok(total_len)
    }

    pub async fn sys_readv(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, iov, iovcnt) = (args[0], UserWritePtr::<IoVec>::from(args[1]), args[2]);

        info!(
            "Syscall: readv, fd: {}, iov: 0x{:x}, iovcnt: {}",
            fd,
            iov.as_usize(),
            iovcnt
        );

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let fd = self.lproc.with_fdtable(|f| f.get(fd)).ok_or(SysError::EBADF)?;
        let file = fd.file.clone();

        let mut offset = fd.curr();
        let mut total_len = 0;
        let mut iov_ptr = iov;
        for i in 0..iovcnt {
            let iov = user_check.checked_read(iov_ptr.raw_ptr())?;
            log::debug!(
                "syscall readv: iov #{}: iov_ptr: 0x{:x}, len: {}",
                i,
                iov_ptr.as_usize(),
                iov.len
            );
            // TODO: 检查用户给的指针是不是合法的
            let buf = unsafe { VirtAddr::from(iov.base).as_mut_slice(iov.len) };
            let read_len = file.read_at(offset, buf).await?;
            total_len += read_len;
            offset += read_len;
            iov_ptr = iov_ptr.add(1);
        }

        fd.add_curr(total_len);
        Ok(total_len)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct IoVec {
    base: usize,
    len: usize,
}

bitflags::bitflags! {
    /// 指定文件打开时的权限
    pub struct OpenFlags: u32 {
        /// 只读
        const RDONLY = 0;
        /// 只能写入
        const WRONLY = 1 << 0;
        /// 读写
        const RDWR = 1 << 1;
        /// 如文件不存在，可创建它
        const CREATE = 1 << 6;
        /// 确认一定是创建文件。如文件已存在，返回 EEXIST。
        const EXCLUSIVE = 1 << 7;
        /// 使打开的文件不会成为该进程的控制终端。目前没有终端设置，不处理
        const NOCTTY = 1 << 8;
        /// 同上，在不同的库中可能会用到这个或者上一个
        const EXCL = 1 << 9;
        /// 非阻塞读写？(虽然不知道为什么但 date.lua 也要)
        const NON_BLOCK = 1 << 11;
        /// 要求把 CR-LF 都换成 LF
        const TEXT = 1 << 14;
        /// 和上面不同，要求输入输出都不进行这个翻译
        const BINARY = 1 << 15;
        /// 对这个文件的输出需符合 IO 同步一致性。可以理解为随时 fsync
        const DSYNC = 1 << 16;
        /// 如果是符号链接，不跟随符号链接去寻找文件，而是针对连接本身
        const NOFOLLOW = 1 << 17;
        /// 在 exec 时需关闭
        const CLOEXEC = 1 << 19;
        /// 是否是目录
        const DIR = 1 << 21;
    }
}

impl OpenFlags {
    /// 获得文件的读/写权限
    pub fn read_write(&self) -> (bool, bool) {
        if self.is_empty() {
            (true, false)
        } else if self.contains(Self::WRONLY) {
            (false, true)
        } else {
            (true, true)
        }
    }
    /// 获取读权限
    pub fn readable(&self) -> bool {
        !self.contains(Self::WRONLY)
    }
    /// 获取写权限
    pub fn writable(&self) -> bool {
        self.contains(Self::WRONLY) || self.contains(Self::RDWR)
    }
}
