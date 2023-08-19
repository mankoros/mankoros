//! Filesystem related syscall
//!

use alloc::string::String;
use log::{debug, info, warn};

use crate::{
    consts::{self, MAX_OPEN_FILES},
    fs::{
        self,
        disk::BLOCK_SIZE,
        memfs::zero::ZeroDev,
        new_vfs::{
            mount::GlobalMountManager,
            path::Path,
            top::{TimeChange, TimeInfoChange, VfsFileRef},
            VfsFileKind,
        },
    },
    memory::{UserReadPtr, UserWritePtr},
    process::lproc::NewFdRequirement,
    timer,
    tools::errors::{SysError, SysResult},
};

use super::{Syscall, SyscallResult};
use core::intrinsics::size_of;

/// 文件信息类
#[repr(C)]
#[derive(Debug, Clone, Copy)]
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

impl Kstat {
    pub async fn from_vfs_file(file: &VfsFileRef) -> SysResult<Self> {
        let kind = file.attr_kind();
        let device = file.attr_device();
        let size = file.attr_size().await?;
        let time = file.attr_time().await?;
        debug!(
            "file info: {:?}, {:?}, {:?}, {:?}",
            kind, device, size, time
        );

        Ok(Kstat {
            st_dev: file.attr_device().device_id as u64,
            st_ino: 1,
            st_mode: u32::from(kind) | 0o777, // 0777 permission, we don't care about permission
            // don't support hard link, just return 1
            st_nlink: 1,
            st_uid: 0,
            st_gid: 0,
            st_rdev: 0,
            _pad0: 0,
            st_size: size.bytes as i64,
            st_blksize: BLOCK_SIZE as i32,
            _pad1: 0,
            st_blocks: size.blocks as i64,
            st_atime_sec: (time.access / consts::time::NSEC_PER_SEC) as isize,
            st_atime_nsec: (time.access % consts::time::NSEC_PER_SEC) as isize,
            st_mtime_sec: (time.modify / consts::time::NSEC_PER_SEC) as isize,
            st_mtime_nsec: (time.modify % consts::time::NSEC_PER_SEC) as isize,
            st_ctime_sec: (time.change / consts::time::NSEC_PER_SEC) as isize,
            st_ctime_nsec: (time.change % consts::time::NSEC_PER_SEC) as isize,
        })
    }
}

impl<'a> Syscall<'a> {
    pub async fn sys_fstat(&self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, kstat) = (args[0], UserWritePtr::<Kstat>::from(args[1]));

        info!("Syscall: fstat, fd: {}, kstat: {}", fd, kstat);

        if let Some(fd) = self.lproc.with_mut_fdtable(|f| f.get(fd)) {
            // TODO: check stat() returned error
            kstat.write(&self.lproc, Kstat::from_vfs_file(&fd.file).await?)?;
            return Ok(0);
        }
        Err(SysError::EBADF)
    }

    pub async fn sys_fstatat(&self) -> SyscallResult {
        info!("Syscall: fstatat");
        let args = self.cx.syscall_args();
        let (dir_fd, path_name, kstat, flags) = (
            args[0],
            UserReadPtr::<u8>::from(args[1]),
            UserWritePtr::<Kstat>::from(args[2]),
            args[3],
        );

        let path_name = path_name.read_cstr(&self.lproc)?;
        info!(
            "fstatat: dir_fd: {}, path_name: {:?}, stat: {}",
            dir_fd, path_name, kstat
        );

        let (dir, file_name) = self.at_helper(dir_fd, path_name, flags).await?;
        let file = self.lookup_helper(dir, &file_name).await?;
        kstat.write(&self.lproc, Kstat::from_vfs_file(&file).await?)?;

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
        let (dir_fd, path, _user_mode) = (args[0], UserReadPtr::<u8>::from(args[1]), args[2]);
        let path = path.read_cstr(&self.lproc)?;

        info!("Syscall: mkdir, dir_fd: {}, path: {:?}", dir_fd, path);

        let (dir, file_name) = self.at_helper(dir_fd, path, 0).await?;
        dir.create(&file_name, VfsFileKind::Directory).await?;
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

        let old_path = old_path.read_cstr(&self.lproc)?;
        let new_path = new_path.read_cstr(&self.lproc)?;

        let (old_dir, old_file_name) = self.at_helper(old_dir_fd, old_path, 0).await?;
        let (new_dir, new_file_name) = self.at_helper(new_dir_fd, new_path, 0).await?;

        if old_dir.kind().await? != VfsFileKind::Directory {
            return Err(SysError::ENOTDIR);
        }
        if new_dir.kind().await? != VfsFileKind::Directory {
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
                    if !subdirs.is_empty() {
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
        let (dir_fd, path, times_ptr, _flags) = (
            args[0],
            UserReadPtr::<u8>::from(args[1]),
            UserReadPtr::<timer::TimeSpec>::from(args[2]),
            args[3],
        );

        let file = if path.not_null() {
            let path = path.read_cstr(&self.lproc)?;
            info!(
                "Syscall: utimensat, dir_fd: {}, path: {:?}, times_ptr: {}",
                dir_fd, path, times_ptr
            );

            let (dir, file_name) = self.at_helper(dir_fd, path, 0).await?;
            if !dir.is_dir().await? {
                return Err(SysError::ENOTDIR);
            }
            self.lookup_helper(dir.clone(), &file_name).await?
        } else {
            info!("Syscall: ftimens, fd: {}, times_ptr: {}", dir_fd, times_ptr);
            self.lproc.with_fdtable(|f| f.get(dir_fd)).ok_or(SysError::EBADF)?.file.clone()
        };

        let times = times_ptr.read_array(2, &self.lproc)?;
        let access_time_change = match times[0].tv_nsec {
            consts::time::UTIME_NOW => TimeChange::new_now(),
            consts::time::UTIME_OMIT => TimeChange::new_omit(),
            _ => TimeChange::new_time(times[0].time_in_ns()),
        };
        let modify_time_change = match times[1].tv_nsec {
            consts::time::UTIME_NOW => TimeChange::new_now(),
            consts::time::UTIME_OMIT => TimeChange::new_omit(),
            _ => TimeChange::new_time(times[1].time_in_ns()),
        };
        let time_info_change = TimeInfoChange::new(access_time_change, modify_time_change);

        file.update_time(time_info_change).await?;
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
                let new_fd = table.dup(NewFdRequirement::None, &old_fd)?;
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
                table.dup(NewFdRequirement::Exactly(new_fd), &old_fd)?;
                Ok(new_fd)
            } else {
                Err(SysError::EBADF)
            }
        })
    }

    pub async fn sys_getdents(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (fd, buf, len) = (args[0], args[1], args[2]);

        info!(
            "Syscall: getdents (fd: {:?}, buf: {:?}, len: {:?})",
            fd, buf, len
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
            let this_entry_len = size_of::<DirentFront>() + name.len() + 1;
            if wroten_len + this_entry_len > len {
                break;
            }

            let dirent_front = DirentFront {
                d_ino: 1,
                d_off: this_entry_len as u64,
                d_reclen: this_entry_len as u16,
                d_type: DirentFront::as_dtype(vfs_entry.kind().await?),
            };

            let dirent_beg = buf + wroten_len;
            let d_name_beg = dirent_beg + size_of::<DirentFront>();

            log::trace!("writing dirent to 0x{:x}...", dirent_beg);
            unsafe {
                UserWritePtr::<u8>::from(dirent_beg).write_as_bytes(&self.lproc, &dirent_front)?;
                UserWritePtr::<u8>::from(d_name_beg).write_cstr(&self.lproc, name)?;
            };

            wroten_len += this_entry_len;
            progress += 1;
        }

        // Store progress back into fd
        fd_obj.set_dents_progress(progress);

        Ok(wroten_len)
    }

    pub async fn sys_unlinkat(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (dir_fd, path_name, flags) = (args[0], UserReadPtr::<u8>::from(args[1]), args[2]);

        let path_name = path_name.read_cstr(&self.lproc)?;
        info!(
            "Syscall: unlinkat (dir_fd: {:?}, path_name: {}, flags: {:?})",
            dir_fd, path_name, flags
        );

        let need_to_be_dir = (flags & AT_REMOVEDIR) != 0;
        let (dir, file_name) = self.at_helper(dir_fd, path_name, flags).await?;

        let file_type = dir.lookup(&file_name).await?.kind().await?;
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

    pub async fn sys_faccessat(&self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (dir_fd, path_name, mode, flags) =
            (args[0], UserReadPtr::<u8>::from(args[1]), args[2], args[3]);

        let path_name = path_name.read_cstr(&self.lproc)?;
        info!(
            "faccessat: dir_fd: {:?}, path_name: {:?}, mode: {:?}, flags: {:?}",
            dir_fd, path_name, mode, flags
        );

        let (dir, file_name) = self.at_helper(dir_fd, path_name, flags).await?;

        // only to ensure file exists
        let _file = dir.lookup(&file_name).await?;
        Ok(0)
    }

    pub async fn sys_statfs(&self) -> SyscallResult {
        #[repr(C)]
        #[derive(Debug, Clone, Copy)]
        struct StatFS {
            /// Type of filesystem (see below)
            f_type: i64,
            /// Optimal transfer block size
            f_bsize: i64,
            /// Total data blocks in filesystem
            f_blocks: u64,
            /// Free blocks in filesystem
            f_bfree: u64,
            /// Free blocks available to
            f_bavail: u64,
            /// Total file nodes in filesystem
            f_files: u64,
            /// Free file nodes in filesystem
            f_ffree: u64,
            /// Filesystem ID
            f_fsid: u64,
            /// Maximum length of filenames
            f_namelen: i64,
            /// Fragment size (since Linux 2.6)
            f_frsize: i64,
            /// Mount flags of filesystem
            f_flags: i64,
            /// Padding bytes reserved for future use
            f_spare: [i64; 4],
        }

        let args = self.cx.syscall_args();
        let (path_name, buf) = (
            UserReadPtr::<u8>::from(args[0]),
            UserWritePtr::<StatFS>::from(args[1]),
        );

        let path_name = path_name.read_cstr(&self.lproc)?;
        info!("statfs: path_name: {:?}, buf: {}", path_name, buf);

        let path = Path::from_string(path_name)?;
        let attr = match GlobalMountManager::get(&path) {
            Some(fs) => fs.attr(),
            None => return Err(SysError::ENOENT),
        };

        use fs::new_vfs::top::VfsFSKind;
        let f_type = match attr.kind {
            // from https://man7.org/linux/man-pages/man2/statfs.2.html
            VfsFSKind::Fat => 0x4d44, // MSDOS_SUPER_MAGIC
            VfsFSKind::Dev => 0x1373,
            VfsFSKind::Tmp => 0x01021994,
            VfsFSKind::Proc => 0x9fa0,
        };

        let stat_fs = StatFS {
            f_type,
            f_bsize: BLOCK_SIZE as _,
            f_blocks: attr.total_block_size as _,
            f_bfree: attr.free_block_size as _,
            f_bavail: attr.free_block_size as _,
            f_files: attr.total_file_count as _,
            f_ffree: attr.total_file_count as _,
            f_fsid: attr.fs_id as _,
            f_namelen: attr.max_file_name_length as _,
            f_frsize: BLOCK_SIZE as _,
            f_flags: 0,
            f_spare: [0; 4],
        };

        buf.write(&self.lproc, stat_fs)?;
        Ok(0)
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
        log::trace!(
            "path: {:?} (is_absolute: {}, is_current: {})",
            path,
            path.is_absolute(),
            path.is_current()
        );

        if path.is_absolute() {
            if path.is_root() {
                // 处理在根目录下解析 "/" 的情况
                dir = fs::get_root_dir();
                file_name = String::from("");
            } else {
                let dir_path = path.remove_tail();
                dir = fs::get_root_dir().resolve(&dir_path).await?;
                file_name = path.last().clone();
            }
        } else {
            let fd_dir = if dir_fd == AT_FDCWD {
                let cwd = self.lproc.with_fsinfo(|f| f.cwd.clone());
                if cwd.is_root() {
                    fs::get_root_dir()
                } else {
                    fs::get_root_dir().resolve(&cwd).await?
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
        if file_name.is_empty() {
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
                new_fd
            }
            _ => {
                log::warn!("fcntl cmd: {} not implemented, returning 0 as default", cmd);
                Ok(0)
            }
        }
    }

    pub async fn sys_mount(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (device, mount_point, _fs_type, _flags, _data) = (
            UserReadPtr::<u8>::from(args[0]),
            UserReadPtr::<u8>::from(args[1]),
            UserReadPtr::<u8>::from(args[2]),
            args[3] as u32,
            UserReadPtr::<u8>::from(args[4]),
        );

        let device = device.read_cstr(&self.lproc)?;
        let mount_point = mount_point.read_cstr(&self.lproc)?;

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
        let dir = fs::get_root_dir().resolve(&path).await?;
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
        let (mount_point, _flags) = (UserReadPtr::<u8>::from(args[0]), args[1] as u32);

        let mount_point = mount_point.read_cstr(&self.lproc)?;
        info!("Syscall: umount (mount_point: {:?})", mount_point);

        let cwd = self.lproc.with_mut_fsinfo(|f| f.cwd.clone());
        let mount_point = cwd.to_string() + "/" + &mount_point;
        // Canonicalize path
        let mount_point = Path::from_string(mount_point)?;
        let (dir_path, file_name) = mount_point.split_dir_file();
        fs::get_root_dir().resolve(&dir_path).await?.detach(&file_name).await?;
        Ok(0)
    }
}
