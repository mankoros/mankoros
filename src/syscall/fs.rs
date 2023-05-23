//! Filesystem related syscall
//!

use alloc::sync::Arc;
use core::cmp::min;
use log::{debug, info};

use crate::{
    arch::within_sum,
    axerrno::AxError,
    executor::util_futures::within_sum_async,
    fs::{
        self,
        pipe::Pipe,
        vfs::{
            filesystem::{VfsNode, VfsWrapper},
            node::{VfsDirEntry, VfsNodeType},
            path::Path,
        },
    },
    memory::{UserReadPtr, UserWritePtr},
    tools::user_check::UserCheck,
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

const AT_REMOVEDIR: usize = 1 << 9;
const AT_FDCWD: usize = -100isize as usize;

impl<'a> Syscall<'a> {
    pub async fn sys_write(
        &mut self,
        fd: usize,
        buf: UserReadPtr<u8>,
        len: usize,
    ) -> SyscallResult {
        info!("Syscall: write, fd {fd}, len: {len}");

        let buf = unsafe { core::slice::from_raw_parts(buf.raw_ptr(), len) };
        let fd = self.lproc.with_mut_fdtable(|f| f.get(fd));
        // TODO: is it safe ?
        if let Some(fd) = fd {
            let write_len = within_sum_async(fd.file.write_at(0, buf)).await?;
            Ok(write_len)
        } else {
            Err(AxError::InvalidInput)
        }
    }
    pub async fn sys_read(
        &mut self,
        fd: usize,
        buf: UserWritePtr<u8>,
        len: usize,
    ) -> SyscallResult {
        info!("Syscall: read, fd {fd}");

        // *mut u8 does not implement Send
        let buf = unsafe { core::slice::from_raw_parts_mut(buf.raw_ptr_mut(), len) };

        let fd = self.lproc.with_mut_fdtable(|f| f.get(fd));
        if let Some(fd) = fd {
            let read_len = within_sum_async(fd.file.read_at(0, buf)).await?;
            Ok(read_len)
        } else {
            Err(AxError::InvalidInput)
        }
    }

    pub fn sys_openat(
        &mut self,
        dir_fd: usize,
        path: *const u8,
        raw_flags: u32,
        _user_mode: i32,
    ) -> SyscallResult {
        info!("Syscall: openat");

        // Parse flags
        let flags = OpenFlags::from_bits_truncate(raw_flags);

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path).map_err(|_| AxError::InvalidInput)?;
        let path = Path::from_string(path).expect("Error parsing path");

        let dir = if path.is_absolute {
            fs::root::get_root_dir()
        } else {
            if dir_fd == AT_FDCWD {
                let cwd = self.lproc.with_fsinfo(|f| f.cwd.to_string());
                fs::root::get_root_dir().lookup(&cwd).map_err(|_| AxError::InvalidInput)?
            } else {
                let file = self
                    .lproc
                    .with_mut_fdtable(|f| f.get(dir_fd as usize))
                    .map(|fd| fd.file.clone())
                    .ok_or(AxError::InvalidInput)?; // TODO: return EBADF
                if file.stat().unwrap().file_type() != VfsNodeType::Dir {
                    return Err(AxError::InvalidInput);
                }
                file
            }
        };

        let file = match dir.clone().lookup(&path.to_string()) {
            Ok(file) => file,
            Err(AxError::NotFound) => {
                // Check if CREATE flag is set
                if !flags.contains(OpenFlags::CREATE) {
                    return Err(AxError::NotFound);
                }
                // Create file
                dir.create(path.to_string().as_str(), VfsNodeType::File)?;
                let file = dir
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

    /// 创建管道，在 *pipe 记录读管道的 fd，在 *(pipe+1) 记录写管道的 fd。
    /// 成功时返回 0，失败则返回 -1
    pub fn sys_pipe(&mut self, pipe: UserWritePtr<u32>) -> SyscallResult {
        info!("Syscall: pipe");
        let (pipe_read, pipe_write) = Pipe::new_pipe();
        let user_check = UserCheck::new_with_sum(&self.lproc);

        self.lproc.with_mut_fdtable(|table| {
            let read_fd = table.alloc(Arc::new(pipe_read));
            let write_fd = table.alloc(Arc::new(pipe_write));

            debug!("read_fd: {}", read_fd);
            debug!("write_fd: {}", write_fd);

            // TODO: check user permissions
            unsafe { *pipe.raw_ptr_mut() = read_fd as u32 }
            unsafe { *pipe.raw_ptr_mut().add(1) = write_fd as u32 }
        });
        Ok(0)
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
                        st_dev: 1,
                        st_ino: 1,
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
        root_fs.create(path.to_string().as_str(), VfsNodeType::Dir)?;
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
        if node_stat.file_type() != VfsNodeType::Dir {
            return Err(AxError::NotADirectory);
        }

        // change the cwd
        self.lproc.with_mut_fsinfo(|f| f.cwd = path);

        Ok(0)
    }

    pub fn sys_getcwd(&mut self, buf: *mut u8, len: usize) -> SyscallResult {
        info!("Syscall: getcwd");
        let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone()).to_string();
        let length = min(cwd.len(), len);
        within_sum(|| unsafe {
            core::ptr::copy_nonoverlapping(cwd.as_ptr(), buf, length);
            *buf.add(length) = 0;
        });
        Ok(buf as usize)
    }

    pub fn sys_getdents(&mut self, fd: usize, buf: *mut u8, len: usize) -> SyscallResult {
        info!(
            "Syscall: getdents (fd: {:?}, buf: {:?}, len: {:?})",
            fd, buf, len
        );

        // TODO: should return EBADF
        let fd_obj = self.lproc.with_fdtable(|f| f.get(fd)).ok_or(AxError::InvalidInput)?;
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

        let mut wroten_len = 0;

        let vfs_entry_iter = VfsDirEntryIter::new(file);
        for vfs_entry in vfs_entry_iter {
            // TODO-BUG: 检查写入后的长度是否满足 u64 的对齐要求, 不满足补 0
            // TODO: d_name 是 &str, 末尾可能会有很多 \0, 想办法去掉它们
            let this_entry_len = core::mem::size_of::<DirentFront>() + vfs_entry.d_name().len() + 1;
            if wroten_len + this_entry_len > len {
                break;
            }

            let dirent_front = DirentFront {
                d_ino: 1,
                d_off: this_entry_len as u64,
                d_reclen: this_entry_len as u16,
                d_type: vfs_entry.d_type().as_char() as u8,
            };

            let dirent_beg = unsafe { buf.add(wroten_len) } as *mut DirentFront;
            let d_name_beg = unsafe { buf.add(wroten_len + core::mem::size_of::<DirentFront>()) };

            let user_check = UserCheck::new_with_sum(&self.lproc);
            user_check
                .checked_write(dirent_beg, dirent_front.clone())
                .map_err(|_| AxError::InvalidInput)?;
            user_check
                .checked_write_cstr(d_name_beg, vfs_entry.d_name())
                .map_err(|_| AxError::InvalidInput)?;

            wroten_len += this_entry_len;
        }

        Ok(wroten_len)
    }

    pub fn sys_unlinkat(
        &mut self,
        dir_fd: usize,
        path_name: *const u8,
        flags: usize,
    ) -> SyscallResult {
        info!(
            "Syscall: unlinkat (dir_fd: {:?}, path_name: {:?}, flags: {:?})",
            dir_fd, path_name, flags
        );

        let need_to_be_dir = (flags & AT_REMOVEDIR) != 0;

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path_name =
            user_check.checked_read_cstr(path_name).map_err(|_| AxError::InvalidInput)?;

        debug!("unlinkat: path_name: {:?}", path_name);

        let path = Path::from_string(path_name).map_err(|_| AxError::InvalidInput)?;

        let dir;
        let file_name;
        if path.is_absolute {
            let dir_path = path.remove_tail();
            dir = fs::root::get_root_dir().lookup(&dir_path.to_string())?;
            file_name = path.last();
        } else {
            let fd_dir = if dir_fd == AT_FDCWD {
                let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone());
                fs::root::get_root_dir().lookup(&cwd.to_string())?
            } else {
                self.lproc
                    .with_fdtable(|f| f.get(dir_fd))
                    .ok_or(AxError::InvalidInput)?
                    .file
                    .clone()
            };

            let rel_dir_path = path.remove_tail();
            dir = fd_dir.lookup(&rel_dir_path.to_string())?;
            file_name = path.last();
        }

        let file_type = dir.clone().lookup(file_name)?.stat()?.file_type();
        if need_to_be_dir && file_type != VfsNodeType::Dir {
            return Err(AxError::NotADirectory);
        }
        if !need_to_be_dir && file_type == VfsNodeType::Dir {
            return Err(AxError::IsADirectory);
        }

        // TODO: 延迟删除: 这个操作会直接让底层 FS 删除文件, 但是如果有其他进程正在使用这个文件, 应该延迟删除
        // 已知 fat32 fs 要求被删除的文件夹是空的, 不然会返回错误, 可能该行为需要被明确到 VFS 层
        dir.remove(&file_name).map(|_| 0)
    }

    pub fn sys_mount(
        &mut self,
        device: UserReadPtr<u8>,
        mount_point: UserReadPtr<u8>,
        _fs_type: UserReadPtr<u8>,
        _flags: u32,
        _data: UserReadPtr<u8>,
    ) -> SyscallResult {
        let user_check = UserCheck::new_with_sum(&self.lproc);
        let device = user_check
            .checked_read_cstr(device.raw_ptr())
            .map_err(|_| AxError::InvalidInput)?;
        let mount_point = user_check
            .checked_read_cstr(mount_point.raw_ptr())
            .map_err(|_| AxError::InvalidInput)?;
        info!(
            "Syscall: mount (device: {:?}, mount_point: {:?})",
            device, mount_point
        );

        // TODO: deal with relative path?
        let dir = fs::root::get_root_dir().lookup(&device)?;
        let cwd = self.lproc.with_mut_fsinfo(|f| f.cwd.clone());
        let mut mount_point = Path::from_string(mount_point).map_err(|_| AxError::InvalidInput)?;
        if !mount_point.is_root() {
            // Canonicalize path
            let tmp = cwd.to_string() + "/" + &mount_point.to_string();
            mount_point = Path::from_string(tmp).map_err(|_| AxError::InvalidInput)?;
        }
        unsafe {
            Arc::get_mut_unchecked(&mut fs::root::get_root_dir())
                .mount(mount_point.to_string(), Arc::new(VfsWrapper::new(dir)))?;
        }
        Ok(0)
    }

    pub fn sys_umount(&mut self, mount_point: UserReadPtr<u8>, _flags: u32) -> SyscallResult {
        let user_check = UserCheck::new_with_sum(&self.lproc);
        let mount_point = user_check
            .checked_read_cstr(mount_point.raw_ptr())
            .map_err(|_| AxError::InvalidInput)?;
        info!("Syscall: umount (mount_point: {:?})", mount_point);

        let cwd = self.lproc.with_mut_fsinfo(|f| f.cwd.clone());
        let mount_point = cwd.to_string() + "/" + &mount_point;
        // Canonicalize path
        let mount_point = Path::from_string(mount_point).map_err(|_| AxError::InvalidInput)?;
        unsafe {
            Arc::get_mut_unchecked(&mut fs::root::get_root_dir()).umount(&mount_point.to_string());
        }
        Ok(0)
    }
}

// 下面是用来实现 getdents 的基建, 可能需要修改 VFS 的接口
struct VfsDirEntryIter {
    dir: Arc<dyn VfsNode>,
    vfs_ent_cnt: usize,
}

impl Iterator for VfsDirEntryIter {
    type Item = VfsDirEntry;

    fn next(&mut self) -> Option<Self::Item> {
        #[allow(invalid_value)]
        let mut vfs_entries: [VfsDirEntry; 1] =
            unsafe { core::mem::MaybeUninit::uninit().assume_init() };

        let read_cnt = self.dir.read_dir(self.vfs_ent_cnt, &mut vfs_entries).unwrap();
        if read_cnt == 0 {
            return None;
        }

        self.vfs_ent_cnt += 1;
        Some(vfs_entries[0].clone())
    }
}

impl VfsDirEntryIter {
    fn new(dir: Arc<dyn VfsNode>) -> Self {
        Self {
            dir,
            vfs_ent_cnt: 0,
        }
    }
}
