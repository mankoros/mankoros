use log::{debug, info};

use crate::{
    executor::util_futures::within_sum_async,
    fs::{
        self,
        new_vfs::{path::Path, top::VfsFileRef, VfsFileKind},
        pipe::Pipe,
    },
    memory::UserWritePtr,
    syscall::fs::AT_FDCWD,
    tools::{errors::SysError, user_check::UserCheck},
};

use super::{Syscall, SyscallResult};

impl Syscall<'_> {
    pub async fn sys_write(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, buf, len) = (args[0], UserWritePtr::from_usize(args[1]), args[2]);

        info!("Syscall: write, fd {fd}, len: {len}");

        let buf = unsafe { core::slice::from_raw_parts(buf.raw_ptr(), len) };
        let fd = self.lproc.with_mut_fdtable(|f| f.get(fd));
        // TODO: is it safe ?
        if let Some(fd) = fd {
            let write_len = within_sum_async(fd.file.write_at(0, buf)).await?;
            Ok(write_len)
        } else {
            Err(SysError::EBADF)
        }
    }
    pub async fn sys_read(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, buf, len) = (args[0], UserWritePtr::from_usize(args[1]), args[2]);

        info!("Syscall: read, fd {fd}");

        // *mut u8 does not implement Send
        let buf = unsafe { core::slice::from_raw_parts_mut(buf.raw_ptr_mut(), len) };

        let fd = self.lproc.with_mut_fdtable(|f| f.get(fd));
        if let Some(fd) = fd {
            let read_len = within_sum_async(fd.file.read_at(0, buf)).await?;
            Ok(read_len)
        } else {
            Err(SysError::EBADF)
        }
    }

    pub async fn sys_openat(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (dir_fd, path, raw_flags, _user_mode) =
            (args[0], args[1], args[2] as u32, args[3] as i32);

        info!("Syscall: openat");

        // Parse flags
        let flags = OpenFlags::from_bits_truncate(raw_flags);

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path as *const u8)?;
        let path = Path::from_string(path).expect("Error parsing path");

        let dir = if path.is_absolute() {
            fs::root::get_root_dir()
        } else {
            if dir_fd == AT_FDCWD {
                let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone());
                fs::root::get_root_dir().resolve(&cwd).await?
            } else {
                let file = self
                    .lproc
                    .with_mut_fdtable(|f| f.get(dir_fd as usize))
                    .map(|fd| fd.file.clone())
                    .ok_or(SysError::EBADF)?;
                if file.attr().await?.kind != VfsFileKind::Directory {
                    return Err(SysError::ENOTDIR);
                }
                file
            }
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
                let file = direct_dir.create(&file_name, VfsFileKind::RegularFile).await?;
                file
            }
            Err(e) => {
                return Err(e);
            }
        };

        self.lproc.with_mut_fdtable(|f| Ok(f.alloc(file) as usize))
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

            debug!("read_fd: {}", read_fd);
            debug!("write_fd: {}", write_fd);

            // TODO: check user permissions
            unsafe { *pipe.raw_ptr_mut() = read_fd as u32 }
            unsafe { *pipe.raw_ptr_mut().add(1) = write_fd as u32 }
        });
        Ok(0)
    }

    pub fn sys_close(&mut self) -> SyscallResult {
        info!("Syscall: close");
        let args = self.cx.syscall_args();
        let fd = args[0];

        self.lproc.with_mut_fdtable(|m| {
            if let Some(_) = m.remove(fd) {
                // close() returns zero on success.  On error, -1 is returned
                // https://man7.org/linux/man-pages/man2/close.2.html
                Ok(0)
            } else {
                Err(SysError::EBADF)
            }
        })
    }

    pub async fn sys_ppoll(&mut self) -> SyscallResult {
        todo!()
    }

    pub async fn sys_writev(&mut self) -> SyscallResult {
        todo!()
    }
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
