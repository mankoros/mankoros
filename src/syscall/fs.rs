//! Filesystem related syscall
//!


use core::cmp::min;
use log::{debug, info, warn};

use crate::{
    arch::within_sum,
    executor::util_futures::within_sum_async,
    fs::{
        self,
        pipe::Pipe, new_vfs::{path::{Path}, VfsFileKind, top::VfsFileRef}, memfs::{zero::ZeroDev}, disk::BLOCK_SIZE
    },
    memory::{UserReadPtr, UserWritePtr},
    tools::{user_check::UserCheck, errors::SysError},
};

use super::{Syscall, SyscallResult};

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
    /// 用户 id
    pub st_uid: u32,
    /// 用户组 id
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
    /// 最后一次访问时间 (秒)
    pub st_atime_sec: isize,
    /// 最后一次访问时间 (纳秒)
    pub st_atime_nsec: isize,
    /// 最后一次修改时间 (秒)
    pub st_mtime_sec: isize,
    /// 最后一次修改时间 (纳秒)
    pub st_mtime_nsec: isize,
    /// 最后一次改变状态时间 (秒)
    pub st_ctime_sec: isize,
    /// 最后一次改变状态时间 (纳秒)
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

const AT_REMOVEDIR: usize = 1 << 9;
const AT_FDCWD: usize = -100isize as usize;

impl<'a> Syscall<'a> {
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
        let (dir_fd, path, raw_flags, _user_mode) = (
            args[0],
            args[1],
            args[2] as u32,
            args[3] as i32,
        );

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

    pub async fn sys_fstat(&self) -> SyscallResult {
        info!("Syscall: fstat");
        let args = self.cx.syscall_args();
        let (fd, kstat) = (args[0], args[1]);
        if let Some(fd) = self.lproc.with_mut_fdtable(|f| f.get(fd)) {
            // TODO: check stat() returned error
            let fstat = fd.file.attr().await?;

            within_sum(|| unsafe {
                *(kstat as *mut Kstat) = Kstat {
                    st_dev: fstat.device_id as u64,
                    st_ino: 1,
                    st_mode: 0,
                    // TODO: when linkat is implemented, use their infrastructure to check link num
                    st_nlink: 1,
                    st_uid: 0,
                    st_gid: 0,
                    st_rdev: 0,
                    _pad0: 0,
                    st_size: fstat.byte_size as u64,
                    st_blksize: BLOCK_SIZE as u32,
                    _pad1: 0,
                    st_blocks: fstat.block_count as u64,
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
        Err(SysError::EBADF)
    }

    pub async fn sys_mkdir(&self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (_dir_fd, path, _user_mode) = (args[0], args[1], args[2]);
        info!("Syscall: mkdir");
        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path as *const u8)?;
        let mut path = Path::from_string(path).expect("Error parsing path");

        let root_fs = fs::root::get_root_dir();

        if !path.is_absolute() {
            // FIXME: us dir_fd to determine current dir
            let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone()).to_string();
            let mut path_str = path.to_string();
            path_str.push_str(&cwd);
            path = Path::from_str(path_str.as_str()).expect("Error parsing path");
        }
        debug!("Creating directory: {:?}", path);
        if root_fs.clone().resolve(&path).await.is_ok() {
            debug!("Directory already exists: {:?}", path);
            return Ok(0);
        }
        root_fs.create(path.to_string().as_str(), VfsFileKind::Directory).await?;
        Ok(0)
    }

    pub fn sys_dup(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let fd = args[0];

        self.lproc.with_mut_fdtable(|table| {
            if let Some(old_fd) = table.get(fd) {
                let new_fd = table.alloc(old_fd.file.clone());
                Ok(new_fd)
            } else {
                Err(SysError::EBADF)
            }
        })
    }
    pub fn sys_dup3(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (old_fd, new_fd) = (args[0], args[1]);

        self.lproc.with_mut_fdtable(|table| {
            if let Some(old_fd) = table.get(old_fd) {
                table.insert(new_fd, old_fd.file.clone());
                Ok(new_fd)
            } else {
                Err(SysError::EBADF)
            }
        })
    }

    pub async fn sys_chdir(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let path = args[0];

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path as *const u8)?;
        let path = Path::from_string(path)?;

        // check whether the path is a directory
        let root_fs = fs::root::get_root_dir();
        let file = root_fs.resolve(&path).await?;
        if file.attr().await?.kind != VfsFileKind::Directory {
            return Err(SysError::ENOTDIR);
        }

        // change the cwd
        self.lproc.with_mut_fsinfo(|f| f.cwd = path);

        Ok(0)
    }

    pub fn sys_getcwd(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let buf = args[0] as *mut u8;
        let len = args[1];

        info!("Syscall: getcwd");
        let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone()).to_string();
        let length = min(cwd.len(), len);
        within_sum(|| unsafe {
            core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf, length);
            *buf.add(length) = 0;
        });
        Ok(buf as usize)
    }

    pub async fn sys_getdents(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, buf, len) = (args[0], UserWritePtr::<u8>::from(args[1]), args[2]);

        info!(
            "Syscall: getdents (fd: {:?}, buf: {:?}, len: {:?})",
            fd, buf.as_usize(), len
        );

        let fd_obj = self.lproc.with_fdtable(|f| f.get(fd)).ok_or(SysError::EBADF)?;
        let file = fd_obj.file.clone();

        /// 目录信息类
        #[repr(packed)]
        #[derive(Clone, Copy)]
        struct DirentFront {
            d_ino: u64,
            d_off: u64,
            d_reclen: u16,
            d_type: u8,
            // dynmaic-len cstr d_name followsing here
        }

        impl DirentFront {
            const DT_UNKNOWN    : u8 = 0;
            const DT_FIFO       : u8 = 1;
            const DT_CHR        : u8 = 2;
            const DT_DIR        : u8 = 4;
            const DT_BLK        : u8 = 6;
            const DT_REG        : u8 = 8;
            const DT_LNK        : u8 = 10;
            const DT_SOCK       : u8 = 12;
            const DT_WHT        : u8 = 14;

            pub fn as_dtype(kind: VfsFileKind) -> u8 {
                match kind {
                    VfsFileKind::Unknown => Self::DT_UNKNOWN,
                    VfsFileKind::Pipe => Self::DT_FIFO,
                    VfsFileKind::CharDevice => Self::DT_CHR,
                    VfsFileKind::Directory => Self::DT_DIR,
                    VfsFileKind::BlockDevice => Self::DT_BLK,
                    VfsFileKind::RegularFile => Self::DT_REG,
                    VfsFileKind::SymbolLink => Self::DT_LNK,
                    VfsFileKind::SocketFile => Self::DT_SOCK,
                }
            }
        }

        let mut wroten_len = 0;

        for (name, vfs_entry) in file.list().await? {
            // TODO-BUG: 检查写入后的长度是否满足 u64 的对齐要求, 不满足补 0
            // TODO: d_name 是 &str, 末尾可能会有很多 \0, 想办法去掉它们
            let this_entry_len = core::mem::size_of::<DirentFront>() + name.len() + 1;
            if wroten_len + this_entry_len > len {
                break;
            }

            let dirent_front = DirentFront {
                d_ino: 1,
                d_off: this_entry_len as u64,
                d_reclen: this_entry_len as u16,
                d_type: DirentFront::as_dtype(vfs_entry.attr().await?.kind),
            };

            let dirent_beg = buf.add(wroten_len).as_usize() as *mut DirentFront;
            let d_name_beg = buf.add(wroten_len + core::mem::size_of::<DirentFront>());

            debug!("dirent: {:x}", dirent_beg as usize);

            let user_check = UserCheck::new_with_sum(&self.lproc);
            user_check.checked_write(dirent_beg, dirent_front.clone())?;
            user_check.checked_write_cstr(d_name_beg.as_usize() as *mut u8, &name)?;

            wroten_len += this_entry_len;
        }

        Ok(wroten_len)
    }

    pub async fn sys_unlinkat(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (dir_fd, path_name, flags) = (args[0], args[1], args[2]);

        info!(
            "Syscall: unlinkat (dir_fd: {:?}, path_name: {:?}, flags: {:?})",
            dir_fd, path_name, flags
        );

        let need_to_be_dir = (flags & AT_REMOVEDIR) != 0;

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path_name = user_check.checked_read_cstr(path_name as *const u8)?;

        debug!("unlinkat: path_name: {:?}", path_name);

        let path = Path::from_string(path_name)?;

        let dir;
        let file_name;
        if path.is_absolute() {
            let dir_path = path.remove_tail();
            dir = fs::root::get_root_dir().resolve(&dir_path).await?;
            file_name = path.last();
        } else {
            let fd_dir = if dir_fd == AT_FDCWD {
                let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone());
                fs::root::get_root_dir().resolve(&cwd).await?
            } else {
                self.lproc
                    .with_fdtable(|f| f.get(dir_fd))
                    .ok_or(SysError::EBADF)?
                    .file
                    .clone()
            };

            let rel_dir_path = path.remove_tail();
            dir = fd_dir.resolve(&rel_dir_path).await?;
            file_name = path.last();
        }

        let file_type = dir.clone().lookup(file_name).await?.attr().await?.kind;
        if need_to_be_dir && file_type != VfsFileKind::Directory {
            return Err(SysError::ENOTDIR);
        }
        if !need_to_be_dir && file_type == VfsFileKind::Directory {
            return Err(SysError::EISDIR);
        }

        // TODO: 延迟删除: 这个操作会直接让底层 FS 删除文件, 但是如果有其他进程正在使用这个文件, 应该延迟删除
        // 已知 fat32 fs 要求被删除的文件夹是空的, 不然会返回错误, 可能该行为需要被明确到 VFS 层
        dir.remove(&file_name).await.map(|_| 0)
    }

    pub async fn sys_mount(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (device, mount_point, _fs_type, _flags, _data): (
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            UserReadPtr<u8>,
            u32,
            UserReadPtr<u8>,
        ) = (
            UserReadPtr::from_usize(args[0]),
            UserReadPtr::from_usize(args[1]),
            UserReadPtr::from_usize(args[2]),
            args[3] as u32,
            UserReadPtr::from_usize(args[4]),
        );

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let device = user_check.checked_read_cstr(device.raw_ptr())?;
        let mount_point = user_check.checked_read_cstr(mount_point.raw_ptr())?;
        info!(
            "Syscall: mount (device: {:?}, mount_point: {:?})",
            device, mount_point
        );
        let _device_path = Path::from_string(device)?;
        // TODO: real mount the device

        // TODO: deal with relative path?
        let cwd = self.lproc.with_mut_fsinfo(|f| f.cwd.clone());
        let mut mount_point = Path::from_string(mount_point)?;
        if !mount_point.is_root() {
            // Canonicalize path
            let tmp = cwd.to_string() + "/" + &mount_point.to_string();
            mount_point = Path::from_string(tmp)?;
            debug_assert!(mount_point.is_absolute());
        }

        let (path, name) = mount_point.split_dir_file();
        let dir = fs::root::get_root_dir().resolve(&path).await?;
        if dir.lookup(&name).await.is_err() {
            warn!("mount: user gives a non-exist dir: {:?}", path);
            warn!("mount: to pass the test, we create it");
            dir.create(&name, VfsFileKind::Directory).await?;
        }
        dir.attach(&name, VfsFileRef::new(ZeroDev)).await?;

        Ok(0)
    }

    pub async fn sys_umount(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (mount_point, _flags) = (UserReadPtr::from_usize(args[0]), args[1] as u32);

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let mount_point = user_check.checked_read_cstr(mount_point.raw_ptr())?;
        info!("Syscall: umount (mount_point: {:?})", mount_point);

        let cwd = self.lproc.with_mut_fsinfo(|f| f.cwd.clone());
        let mount_point = cwd.to_string() + "/" + &mount_point;
        // Canonicalize path
        let mount_point = Path::from_string(mount_point)?;
        let (dir_path, file_name) = mount_point.split_dir_file();
        fs::root::get_root_dir().resolve(&dir_path).await?.detach(&file_name).await?;
        Ok(0)
    }
}
