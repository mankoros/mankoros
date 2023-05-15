//! Filesystem related syscall
//!

use alloc::{borrow::ToOwned, string::ToString};
use log::{debug, info};

use crate::{
    axerrno::AxError,
    fs::{
        self,
        vfs::{filesystem::VfsNode, path::Path},
    },
    tools::user_check::UserCheck,
    utils,
};

use super::{Syscall, SyscallResult};
use crate::arch::within_sum;

/// 文件信息类
#[repr(C)]
pub struct Kstat {
    /// 设备
    pub st_dev: u64,
    /// inode 编号
    pub st_ino: u64,
    /// 文件类型
    pub st_mode: u32,
    /// 硬链接数
    pub st_nlink: u32,
    /// 用户id
    pub st_uid: u32,
    /// 用户组id
    pub st_gid: u32,
    /// 设备号
    pub st_rdev: u64,
    _pad0: u64,
    /// 文件大小
    pub st_size: u64,
    /// 块大小
    pub st_blksize: u32,
    _pad1: u32,
    /// 块个数
    pub st_blocks: u64,
    /// 最后一次访问时间(秒)
    pub st_atime_sec: isize,
    /// 最后一次访问时间(纳秒)
    pub st_atime_nsec: isize,
    /// 最后一次修改时间(秒)
    pub st_mtime_sec: isize,
    /// 最后一次修改时间(纳秒)
    pub st_mtime_nsec: isize,
    /// 最后一次改变状态时间(秒)
    pub st_ctime_sec: isize,
    /// 最后一次改变状态时间(纳秒)
    pub st_ctime_nsec: isize,
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
        /// 非阻塞读写?(虽然不知道为什么但 date.lua 也要)
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

impl<'a> Syscall<'a> {
    pub fn sys_write(&mut self, fd: usize, buf: *const u8, len: usize) -> SyscallResult {
        info!("Syscall: write, fd {fd}, len: {len}");

        self.lproc.with_mut_fdtable(|f| {
            if let Some(fd) = f.get(fd) {
                let write_len = within_sum(|| {
                    fd.file.write_at(0, unsafe { core::slice::from_raw_parts(buf, len) })
                })?;

                Ok(write_len)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }
    pub fn sys_read(&mut self, fd: usize, buf: *mut u8, len: usize) -> SyscallResult {
        info!("Syscall: read, fd {fd}");

        self.lproc.with_mut_fdtable(|f| {
            if let Some(fd) = f.get(fd) {
                let read_len = within_sum(|| {
                    fd.file.read_at(0, unsafe { core::slice::from_raw_parts_mut(buf, len) })
                })?;

                Ok(read_len)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }

    pub fn sys_openat(
        &mut self,
        _dir_fd: i32,
        path: *const u8,
        raw_flags: u32,
        _user_mode: i32,
    ) -> SyscallResult {
        info!("Syscall: openat");

        // Parse flags
        let flags = OpenFlags::from_bits_truncate(raw_flags);

        let root_fs = fs::root::get_root_dir();
        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path).map_err(|_| AxError::InvalidInput)?;
        let path = Path::from_string(path).expect("Error parsing path");
        let file = match within_sum(|| root_fs.clone().lookup(&path.to_string())) {
            Ok(file) => file,
            Err(AxError::NotFound) => {
                // Check if CREATE flag is set
                if !flags.contains(OpenFlags::CREATE) {
                    return Err(AxError::NotFound);
                }
                // Create file
                root_fs.create(path.to_string().as_str(), fs::vfs::node::VfsNodeType::File)?;
                let file = root_fs
                    .lookup(&path.to_string())
                    .expect("File just created is not found, very wrong");
                file
            }
            Err(_) => {
                return Err(AxError::NotFound);
            }
        };

        self.lproc.with_mut_fdtable(|f| Ok(f.alloc(file) as usize))
    }

    pub fn sys_close(&mut self, fd: usize) -> SyscallResult {
        info!("Syscall: close");

        self.lproc.with_mut_fdtable(|m| {
            if let Some(_) = m.remove(fd) {
                // close() returns zero on success.  On error, -1 is returned
                // https://man7.org/linux/man-pages/man2/close.2.html
                Ok(0)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }

    pub fn sys_fstat(&self, fd: usize, kstat: *mut Kstat) -> SyscallResult {
        info!("Syscall: fstat");
        self.lproc.with_mut_fdtable(|f| {
            if let Some(fd) = f.get(fd) {
                // TODO: check stat() returned error
                let fstat = fd.file.stat().unwrap();

                within_sum(|| unsafe {
                    *kstat = Kstat {
                        st_dev: 0,
                        st_ino: 0,
                        st_mode: 0,
                        st_nlink: 0,
                        st_uid: 0,
                        st_gid: 0,
                        st_rdev: 0,
                        _pad0: 0,
                        st_size: fstat.size(),
                        st_blksize: 0,
                        _pad1: 0,
                        st_blocks: 0,
                        st_atime_sec: 0,
                        st_atime_nsec: 0,
                        st_mtime_sec: 0,
                        st_mtime_nsec: 0,
                        st_ctime_sec: 0,
                        st_ctime_nsec: 0,
                    }
                });
                return Ok(0);
            }
            Err(AxError::NotFound)
        })
    }

    pub fn sys_mkdir(&self, _dir_fd: usize, path: *const u8, _user_mode: usize) -> SyscallResult {
        info!("Syscall: mkdir");
        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path).map_err(|_| AxError::InvalidInput)?;
        let mut path = Path::from_string(path).expect("Error parsing path");

        let root_fs = fs::root::get_root_dir();

        if !path.is_absolute {
            // FIXME: us dir_fd to determine current dir
            let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone()).to_string();
            let mut path_str = path.to_string();
            path_str.push_str(&cwd);
            path = Path::from_str(path_str.as_str()).expect("Error parsing path");
        }
        debug!("Creating directory: {:?}", path);
        if root_fs.clone().lookup(&path.to_string()).is_ok() {
            debug!("Directory already exists: {:?}", path);
            return Ok(0);
        }
        root_fs.create(path.to_string().as_str(), fs::vfs::node::VfsNodeType::Dir)?;
        Ok(0)
    }

    pub fn sys_dup(&mut self, fd: usize) -> SyscallResult {
        self.lproc.with_mut_fdtable(|table| {
            if let Some(old_fd) = table.get(fd) {
                let new_fd = table.alloc(old_fd.file.clone());
                Ok(new_fd)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }
    pub fn sys_dup3(&mut self, old_fd: usize, new_fd: usize) -> SyscallResult {
        self.lproc.with_mut_fdtable(|table| {
            if let Some(old_fd) = table.get(old_fd) {
                table.insert(new_fd, old_fd.file.clone());
                Ok(new_fd)
            } else {
                Err(AxError::InvalidInput)
            }
        })
    }

    pub fn sys_chdir(&mut self, path: *const u8) -> SyscallResult {
        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path).map_err(|_| AxError::InvalidInput)?;
        let path = Path::from_string(path).map_err(|_| AxError::InvalidInput)?;

        // check whether the path is a directory
        let root_fs = fs::root::get_root_dir();
        let node = root_fs.lookup(&path.to_string())?;
        let node_stat = node.stat()?;
        if node_stat.type_() != fs::vfs::node::VfsNodeType::Dir {
            return Err(AxError::NotADirectory);
        }

        // change the cwd
        self.lproc.with_mut_fsinfo(|f| f.cwd = path);

        Ok(0)
    }
}
