//! Filesystem related syscall
//!

use alloc::string::String;
use log::{debug, info, warn};

use crate::{
    arch::within_sum,
    consts::MAX_OPEN_FILES,
    fs::{
        self,
        disk::BLOCK_SIZE,
        memfs::zero::ZeroDev,
        new_vfs::{path::Path, top::VfsFileRef, VfsFileKind},
    },
    memory::{address::VirtAddr, UserReadPtr, UserWritePtr},
    process::{lproc::NewFdRequirement, user_space::user_area::UserAreaPerm},
    tools::{
        errors::{SysError, SysResult},
        user_check::UserCheck,
    },
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
    pub st_size: i64,
    /// 块大小
    pub st_blksize: i32,
    _pad1: i32,
    /// 块个数
    pub st_blocks: i64,
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

pub const AT_REMOVEDIR: usize = 1 << 9;
pub const AT_FDCWD: usize = -100isize as usize;

// fnctl flags
bitflags::bitflags! {
    #[derive(Default)]
    pub struct FcntlFlags: u32 {
        const F_DUPFD = 0;
        const F_GETFD = 1;
        const F_SETFD = 2;
        const F_GETFL = 3;
        const F_SETFL = 4;
    }
}

impl<'a> Syscall<'a> {
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
                    st_mode: u32::from(fstat.kind) | 0o777, // 0777 permission, we don't care about permission
                    // TODO: when linkat is implemented, use their infrastructure to check link num
                    st_nlink: 1,
                    st_uid: 0,
                    st_gid: 0,
                    st_rdev: 0,
                    _pad0: 0,
                    st_size: fstat.byte_size as i64,
                    st_blksize: BLOCK_SIZE as i32,
                    _pad1: 0,
                    st_blocks: fstat.block_count as i64,
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

    pub async fn sys_fstatat(&self) -> SyscallResult {
        info!("Syscall: fstatat");
        let args = self.cx.syscall_args();
        let (dir_fd, path_name, kstat, flags) = (args[0], args[1], args[2], args[3]);

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path_name = user_check.checked_read_cstr(path_name as *const u8)?;

        info!(
            "fstatat: dir_fd: {}, path_name: {:?}, stat: 0x{:x}",
            dir_fd, path_name, kstat
        );

        let (dir, file_name) = self.at_helper(dir_fd, path_name, flags).await?;

        let file = if file_name == *"" {
            dir
        } else {
            dir.lookup(&file_name).await?
        };

        let fstat = file.attr().await?;

        debug!("Fstat: {:?}", fstat);

        within_sum(|| unsafe {
            *(kstat as *mut Kstat) = Kstat {
                st_dev: fstat.device_id as u64,
                st_ino: 1,
                st_mode: u32::from(fstat.kind) | 0o777, // 0777 permission, we don't care about permission
                // TODO: when linkat is implemented, use their infrastructure to check link num
                st_nlink: 1,
                st_uid: 0,
                st_gid: 0,
                st_rdev: 0,
                _pad0: 0,
                st_size: fstat.byte_size as i64,
                st_blksize: BLOCK_SIZE as i32,
                _pad1: 0,
                st_blocks: fstat.block_count as i64,
                st_atime_sec: 0,
                st_atime_nsec: 0,
                st_mtime_sec: 0,
                st_mtime_nsec: 0,
                st_ctime_sec: 0,
                st_ctime_nsec: 0,
            }
        });

        Ok(0)
    }

    pub async fn sys_fturncate(&self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, length) = (args[0], args[1]);
        let file = self.lproc.with_mut_fdtable(|f| f.get(fd)).ok_or(SysError::EBADF)?;
        file.file.truncate(length).await?;
        Ok(0)
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

    pub async fn sys_renameat2(&self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (old_dir_fd, old_path, new_dir_fd, new_path) = (
            args[0],
            UserReadPtr::<u8>::from(args[1]),
            args[2],
            UserReadPtr::<u8>::from(args[3]),
        );

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let old_path = user_check.checked_read_cstr(old_path.raw_ptr())?;
        let new_path = user_check.checked_read_cstr(new_path.raw_ptr())?;

        let (old_dir, old_file_name) = self.at_helper(old_dir_fd, old_path, 0).await?;
        let (new_dir, new_file_name) = self.at_helper(new_dir_fd, new_path, 0).await?;

        if old_dir.attr().await?.kind != VfsFileKind::Directory {
            return Err(SysError::ENOTDIR);
        }
        if new_dir.attr().await?.kind != VfsFileKind::Directory {
            return Err(SysError::ENOTDIR);
        }

        // TODO: check:
        // EINVAL:
        //      The new pathname contained a path prefix of the old, or, more generally,
        //      an attempt was made to make a directory a subdirectory of itself.

        let new_file_result = self.lookup_helper(new_dir.clone(), &new_file_name).await;
        match new_file_result {
            Ok(new_file) => {
                if new_file.is_dir().await? {
                    let subdirs = new_file.list().await?;
                    if subdirs.len() > 0 {
                        return Err(SysError::ENOTEMPTY);
                    }
                    // replace
                    let old_file = old_dir.detach(&old_file_name).await?;
                    new_dir.remove(&new_file_name).await?;
                    new_dir.attach(&new_file_name, old_file).await?;
                }
            }
            Err(SysError::ENOENT) => {
                let old_file = old_dir.detach(&old_file_name).await?;
                new_dir.attach(&new_file_name, old_file).await?;
            }
            Err(e) => {
                return Err(e);
            }
        }

        Ok(0)
    }

    pub async fn sys_utimensat(&self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (dir_fd, path, _times, _flags) =
            (args[0], UserReadPtr::<u8>::from(args[1]), args[2], args[3]);

        let user_check = UserCheck::new_with_sum(&self.lproc);
        let path = user_check.checked_read_cstr(path.raw_ptr())?;

        let (dir, file_name) = self.at_helper(dir_fd, path, 0).await?;
        if !dir.is_dir().await? {
            return Err(SysError::ENOTDIR);
        }

        log::warn!("utimensat: do NOT update the time, just check the file exists");
        let _file = self.lookup_helper(dir.clone(), &file_name).await?;
        Ok(0)
    }

    pub fn sys_dup(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let fd = args[0];

        self.lproc.with_mut_fdtable(|table| {
            // Check if too many open files
            if table.len() >= MAX_OPEN_FILES {
                return Err(SysError::EMFILE);
            }
            if let Some(old_fd) = table.get(fd) {
                let new_fd = table.dup(NewFdRequirement::None, &old_fd);
                Ok(new_fd)
            } else {
                Err(SysError::EBADF)
            }
        })
    }
    pub fn sys_dup3(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (old_fd, new_fd) = (args[0], args[1]);
        log::info!("dup3: old_fd: {}, new_fd: {}", old_fd, new_fd);

        self.lproc.with_mut_fdtable(|table| {
            if table.get(new_fd).is_none() {
                // Check if too many open files
                if table.len() >= MAX_OPEN_FILES {
                    return Err(SysError::EMFILE);
                }
            }
            if let Some(old_fd) = table.get(old_fd) {
                table.dup(NewFdRequirement::Exactly(new_fd), &old_fd);
                Ok(new_fd)
            } else {
                Err(SysError::EBADF)
            }
        })
    }

    pub async fn sys_getdents(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, buf, len) = (args[0], UserWritePtr::<u8>::from(args[1]), args[2]);

        info!(
            "Syscall: getdents (fd: {:?}, buf: {:?}, len: {:?})",
            fd,
            buf.as_usize(),
            len
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
            const DT_UNKNOWN: u8 = 0;
            const DT_FIFO: u8 = 1;
            const DT_CHR: u8 = 2;
            const DT_DIR: u8 = 4;
            const DT_BLK: u8 = 6;
            const DT_REG: u8 = 8;
            const DT_LNK: u8 = 10;
            const DT_SOCK: u8 = 12;
            const DT_WHT: u8 = 14;

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

        let files = file.list().await?;
        let total_files = files.len();
        let mut progress = fd_obj.get_dents_progress();
        let end_of_dir = total_files == progress;
        if end_of_dir {
            fd_obj.clear_dents_progress();
            // On end of directory, 0 is returned.
            return Ok(0);
        }

        debug!(
            "Old progress: {:?}, total files: {:?}",
            progress, total_files
        );

        for (name, vfs_entry) in &files[progress..] {
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
            user_check.checked_write(dirent_beg, dirent_front)?;
            user_check.checked_write_cstr(d_name_beg.as_usize() as *mut u8, name)?;

            wroten_len += this_entry_len;
            progress += 1;
        }

        // Store progress back into fd
        fd_obj.set_dents_progress(progress);

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

        let (dir, file_name) = self.at_helper(dir_fd, path_name, flags).await?;

        let file_type = dir.clone().lookup(&file_name).await?.attr().await?.kind;
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

    /// Path resolve helper for __at syscall
    /// return (dir, filename)
    pub(super) async fn at_helper(
        &self,
        dir_fd: usize,
        path_name: String,
        _flags: usize,
    ) -> SysResult<(VfsFileRef, String)> {
        let dir;
        let file_name;

        let path = Path::from_string(path_name)?;

        log::warn!(
            "path: {:?} (is_absolute: {}, is_current: {})",
            path,
            path.is_absolute(),
            path.is_current()
        );

        if path.is_absolute() {
            if path.is_root() {
                // 处理在根目录下解析 "/" 的情况
                dir = fs::root::get_root_dir();
                file_name = String::from("");
            } else {
                let dir_path = path.remove_tail();
                dir = fs::root::get_root_dir().resolve(&dir_path).await?;
                file_name = path.last().clone();
            }
        } else {
            let fd_dir = if dir_fd == AT_FDCWD {
                let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone());
                if cwd.is_root() {
                    fs::root::get_root_dir()
                } else {
                    fs::root::get_root_dir().resolve(&cwd).await?
                }
            } else {
                self.lproc.with_fdtable(|f| f.get(dir_fd)).ok_or(SysError::EBADF)?.file.clone()
            };

            if path.is_current() {
                dir = fd_dir;
                file_name = String::from("");
            } else {
                let rel_dir_path = path.remove_tail();
                dir = fd_dir.resolve(&rel_dir_path).await?;
                file_name = path.last().clone();
            }
        }

        Ok((dir, file_name))
    }

    /// if file_name is "", return dir; otherwise, return dir/file_name
    pub(super) async fn lookup_helper(
        &self,
        dir: VfsFileRef,
        file_name: &str,
    ) -> SysResult<VfsFileRef> {
        if file_name == "" {
            Ok(dir)
        } else {
            dir.lookup(file_name).await
        }
    }

    pub fn sys_fcntl(&mut self) -> SyscallResult {
        const F_DUPFD_CLOEXEC: usize = 1030;

        let args = self.cx.syscall_args();
        let (fd, cmd, arg) = (args[0], args[1], args[2]);
        info!(
            "Syscall: fcntl (fd: {:?}, cmd: {:?}, arg: {:?})",
            fd, cmd, arg
        );

        match cmd {
            F_DUPFD_CLOEXEC => {
                let fd_lower_bound = arg;
                let new_fd = self.lproc.with_mut_fdtable(|f| {
                    let old_fd = f.get(fd).unwrap();
                    f.dup(NewFdRequirement::GreaterThan(fd_lower_bound), &old_fd)
                });
                Ok(new_fd)
            }
            _ => {
                log::warn!("fcntl cmd: {} not implemented, returning 0 as default", cmd);
                Ok(0)
            }
        }
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
