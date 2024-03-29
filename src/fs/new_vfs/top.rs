use super::{path::Path, DeviceID, VfsFileKind};
use crate::{
    consts,
    memory::address::PhysAddr4K,
    timer::get_time_us,
    tools::errors::{dyn_future, ASysResult, SysResult},
};
use alloc::{string::String, sync::Arc, vec::Vec};
use core::{
    any::Any,
    ops::{Deref, DerefMut},
};

pub enum VfsFSKind {
    Fat,
    Dev,
    Tmp,
    Proc,
}

pub struct VfsFSAttr {
    pub kind: VfsFSKind,
    pub fs_id: usize,

    pub total_block_size: usize,
    pub free_block_size: usize,

    // will be the fat count for fat32
    pub total_file_count: usize,
    // will be the free fat count for fat32
    pub free_file_count: usize,

    pub max_file_name_length: usize,
}

impl VfsFSAttr {
    pub fn default_mem(kind: VfsFSKind, id: usize) -> Self {
        Self {
            kind,
            fs_id: id,
            total_block_size: 0,
            free_block_size: 0,
            total_file_count: 0,
            free_file_count: 0,
            max_file_name_length: NORMAL_FILE_NAME_LENGTH,
        }
    }
}

/// 如果理论上一个 FS 能支持无限长的文件名, 那么它可以取这个典型值
pub const NORMAL_FILE_NAME_LENGTH: usize = 511;

pub trait VfsFS {
    fn root(&self) -> VfsFileRef;
    fn attr(&self) -> VfsFSAttr;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmapKind {
    Shared,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollKind {
    Read,
    Write,
}

pub const OFFSET_TAIL: usize = 0;

/// 代表一个 ioctl 指令.
/// 具体能响应什么 ioctl 指令由 VfsFile 实现者决定,
/// 在具体的模块中重新打开该结构体并提供一些常量即可.
///
/// 可参考 src/fs/memfs/tty.rs 中的实现.
/// Unix 中各类 ioctl cmd 的定义可参考 musl:
/// https://github.com/bminor/musl/blob/v1.2.4/arch/generic/bits/ioctl.h
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IOCTLCmd(pub usize);

impl From<usize> for IOCTLCmd {
    fn from(cmd: usize) -> Self {
        Self(cmd)
    }
}
impl Into<usize> for IOCTLCmd {
    fn into(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct DeviceInfo {
    pub device_id: DeviceID,
    pub self_device_id: DeviceID,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SizeInfo {
    pub bytes: usize,
    pub blocks: usize,
}
impl SizeInfo {
    pub fn new_bytes_only(bytes: usize) -> Self {
        Self { bytes, blocks: 0 }
    }
    pub fn new_zero() -> Self {
        Self {
            bytes: 0,
            blocks: 0,
        }
    }
}

/// 所有时间的单位都是纳秒
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TimeInfo {
    pub access: usize,
    pub modify: usize,
    pub change: usize,
}
impl TimeInfo {
    pub fn new_zero() -> Self {
        Self {
            access: 0,
            modify: 0,
            change: 0,
        }
    }
}

pub struct TimeChange(usize);
impl TimeChange {
    pub fn new_omit() -> Self {
        Self(consts::time::UTIME_OMIT)
    }
    pub fn new_now() -> Self {
        Self(consts::time::UTIME_NOW)
    }
    pub fn new_time(time: usize) -> Self {
        debug_assert!(time < consts::time::UTIME_NOW);
        Self(time)
    }
}
impl Into<usize> for TimeChange {
    fn into(self) -> usize {
        debug_assert!(self.0 < consts::time::UTIME_NOW);
        self.0
    }
}

pub struct TimeInfoChange {
    pub access: TimeChange,
    pub modify: TimeChange,
}

impl TimeInfoChange {
    pub fn new(access: TimeChange, modify: TimeChange) -> Self {
        Self { access, modify }
    }
}
impl From<TimeInfo> for TimeInfoChange {
    fn from(info: TimeInfo) -> Self {
        Self {
            access: TimeChange::new_time(info.access),
            modify: TimeChange::new_time(info.modify),
        }
    }
}
impl TimeInfo {
    pub fn apply_change(&mut self, change: TimeInfoChange) {
        self.access = match change.access.0 {
            consts::time::UTIME_OMIT => self.access,
            consts::time::UTIME_NOW => get_time_us() * 1000,
            time => time,
        };
        self.modify = match change.modify.0 {
            consts::time::UTIME_OMIT => self.modify,
            consts::time::UTIME_NOW => get_time_us() * 1000,
            time => time,
        };
    }
}

pub trait VfsFile: Send + Sync {
    // 通用操作
    /// 获取文件的各类属性,
    /// 例如文件类型, 文件大小, 文件创建时间等等
    fn attr_kind(&self) -> VfsFileKind;
    fn attr_device(&self) -> DeviceInfo;
    fn attr_size(&self) -> ASysResult<SizeInfo>;
    fn attr_time(&self) -> ASysResult<TimeInfo>;
    fn update_time(&self, info: TimeInfoChange) -> ASysResult;

    // 文件操作
    /// 读取文件内容
    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize>;
    /// 写入文件内容
    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize>;
    /// 获得代表文件 [offset, offset + PAGE_SIZE) 范围内内容的物理页.
    /// offset 必须是 PAGE_SIZE 的倍数.
    fn get_page(&self, offset: usize, kind: MmapKind) -> ASysResult<PhysAddr4K>;
    /// 改变文件长度
    fn truncate(&self, length: usize) -> ASysResult;

    // 高级文件操作
    /// 要求文件准备好 [offset, offset + len) 范围内的内容以供读取或写入.
    /// 如果没准备好会返回 Pending, 准备好了就会 wake / 直接返回 Ready.
    fn poll_ready(&self, offset: usize, len: usize, kind: PollKind) -> ASysResult<usize>;
    /// 阻塞地读取文件内容, 逻辑上应该只在 poll_ready 返回 Ready 之后调用.
    fn poll_read(&self, offset: usize, buf: &mut [u8]) -> usize;
    /// 阻塞地写入文件内容, 逻辑上应该只在 poll_ready 返回 Ready 之后调用.
    fn poll_write(&self, offset: usize, buf: &[u8]) -> usize;
    /// 响应自定义的 ioctl 指令
    fn ioctl(&self, _cmd: IOCTLCmd, _arg: usize) -> ASysResult<usize> {
        dyn_future(async { Ok(0) })
    }

    // 文件夹操作
    /// 列出文件夹中的所有文件的名字
    fn list(&self) -> ASysResult<Vec<(String, VfsFileRef)>>;
    /// 根据名字查找文件夹中的文件, 不会递归查找
    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef>;
    /// 新建一个文件, 并在当前文件夹中创建一个 名字->新建文件 的映射
    fn create<'a>(&'a self, name: &'a str, kind: VfsFileKind) -> ASysResult<VfsFileRef>;
    /// 删除一个文件, 并在当前文件夹中删除一个 名字->文件 的映射
    fn remove<'a>(&'a self, name: &'a str) -> ASysResult;

    /// 在目录中删除 名字->文件 的映射, 但并不真的删掉它.
    /// 可用于实现延迟删除
    fn detach<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef>;
    /// 尝试将一个可能并不属于当前文件系统的文件 "贴" 到当前文件夹中.
    /// 可用于实现 mount
    fn attach<'a>(&'a self, name: &'a str, file: VfsFileRef) -> ASysResult;

    // 通用操作
    fn as_any(&self) -> &dyn Any;
}

#[derive(Clone)]
pub struct VfsFileRef(Arc<dyn VfsFile>);

impl Deref for VfsFileRef {
    type Target = dyn VfsFile;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl DerefMut for VfsFileRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::get_mut(&mut self.0).expect("VfsFileRef is not unique")
    }
}

impl VfsFileRef {
    pub fn new<T: VfsFile + 'static>(file: T) -> Self {
        Self(Arc::new(file))
    }

    pub async fn kind(&self) -> SysResult<VfsFileKind> {
        Ok(self.0.attr_kind())
    }
    pub async fn size(&self) -> SysResult<usize> {
        self.0.attr_size().await.map(|s| s.bytes)
    }

    pub async fn is_dir(&self) -> SysResult<bool> {
        Ok(self.kind().await? == VfsFileKind::Directory)
    }
    pub async fn is_file(&self) -> SysResult<bool> {
        Ok(self.kind().await? == VfsFileKind::RegularFile)
    }
    pub async fn is_symlink(&self) -> SysResult<bool> {
        Ok(self.kind().await? == VfsFileKind::SymbolLink)
    }

    pub async fn resolve(&self, path: &Path) -> SysResult<Self> {
        let mut cur = self.clone();
        for name in path.iter() {
            cur = cur.lookup(name).await?;
        }
        Ok(cur)
    }
}

#[derive(Clone)]
pub struct VfsFSRef(Arc<dyn VfsFS>);

unsafe impl Send for VfsFSRef {}
unsafe impl Sync for VfsFSRef {}

impl VfsFSRef {
    pub fn new<T: VfsFS + 'static>(fs: T) -> Self {
        Self(Arc::new(fs))
    }
}

impl Deref for VfsFSRef {
    type Target = dyn VfsFS;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl DerefMut for VfsFSRef {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::get_mut(&mut self.0).expect("VfsFileRef is not unique")
    }
}

#[macro_export]
macro_rules! impl_vfs_default_non_dir {
    ($ty:ident) => {
        fn list(
            &self,
        ) -> $crate::tools::errors::ASysResult<
            alloc::vec::Vec<(alloc::string::String, $crate::fs::new_vfs::top::VfsFileRef)>,
        > {
            unimplemented!(concat!(stringify!($ty), "::list"))
        }
        fn lookup(
            &self,
            _name: &str,
        ) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::lookup"))
        }
        fn create(
            &self,
            _name: &str,
            _kind: $crate::fs::new_vfs::VfsFileKind,
        ) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::create"))
        }
        fn remove(&self, _name: &str) -> $crate::tools::errors::ASysResult {
            unimplemented!(concat!(stringify!($ty), "::remove"))
        }
        fn detach(
            &self,
            _name: &str,
        ) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::detach"))
        }
        fn attach(
            &self,
            _name: &str,
            _node: crate::fs::new_vfs::top::VfsFileRef,
        ) -> crate::tools::errors::ASysResult {
            unimplemented!(concat!(stringify!($ty), "::attach"))
        }
    };
}

#[macro_export]
macro_rules! impl_vfs_default_non_file {
    ($ty:ident) => {
        fn read_at(
            &self,
            _offset: usize,
            _buf: &mut [u8],
        ) -> $crate::tools::errors::ASysResult<usize> {
            unimplemented!(concat!(stringify!(ty), "::read_at"))
        }
        fn write_at(
            &self,
            _offset: usize,
            _buf: &[u8],
        ) -> $crate::tools::errors::ASysResult<usize> {
            unimplemented!(concat!(stringify!(ty), "::write_at"))
        }
        fn get_page(
            &self,
            _offset: usize,
            _kind: $crate::fs::new_vfs::top::MmapKind,
        ) -> $crate::tools::errors::ASysResult<$crate::memory::address::PhysAddr4K> {
            unimplemented!(concat!(stringify!(ty), "::get_page"))
        }
        fn truncate(&self, _len: usize) -> $crate::tools::errors::ASysResult {
            unimplemented!(concat!(stringify!(ty), "::truncate"))
        }
        fn poll_ready(
            &self,
            _offset: usize,
            _len: usize,
            _kind: $crate::fs::new_vfs::top::PollKind,
        ) -> $crate::tools::errors::ASysResult<usize> {
            unimplemented!(concat!(stringify!(ty), "::poll_ready"))
        }
        fn poll_read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
            unimplemented!(concat!(stringify!(ty), "::poll_read"))
        }
        fn poll_write(&self, _offset: usize, _buf: &[u8]) -> usize {
            unimplemented!(concat!(stringify!(ty), "::poll_write"))
        }
    };
}

#[macro_export]
macro_rules! impl_vfs_forward_dir {
    ($($e:tt)+) => {
        fn list(&self) -> $crate::tools::errors::ASysResult<alloc::vec::Vec<(alloc::string::String, $crate::fs::new_vfs::top::VfsFileRef)>> {
            self.$($e)+.list()
        }
        fn lookup<'a>(&'a self, name: &'a str) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::VfsFileRef> {
            self.$($e)+.lookup(name)
        }
        fn create<'a>(&'a self, name: &'a str, kind: $crate::fs::new_vfs::VfsFileKind) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::VfsFileRef> {
            self.$($e)+.create(name, kind)
        }
        fn remove<'a>(&'a self, name: &'a str) -> $crate::tools::errors::ASysResult {
            self.$($e)+.remove(name)
        }
        fn detach<'a>(&'a self, name: &'a str) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::VfsFileRef> {
            self.$($e)+.detach(name)
        }
        fn attach<'a>(&'a self, name: &'a str, node: crate::fs::new_vfs::top::VfsFileRef) -> crate::tools::errors::ASysResult {
            self.$($e)+.attach(name, node)
        }
    };
}

#[macro_export]
macro_rules! impl_vfs_forward_file {
    ($($e:tt)+) => {
        fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> $crate::tools::errors::ASysResult<usize> {
            self.$($e)+.read_at(offset, buf)
        }
        fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> $crate::tools::errors::ASysResult<usize> {
            self.$($e)+.write_at(offset, buf)
        }
        fn get_page(&self, offset: usize, kind: $crate::fs::new_vfs::top::MmapKind) -> $crate::tools::errors::ASysResult<$crate::memory::address::PhysAddr4K> {
            self.$($e)+.get_page(offset, kind)
        }
        fn truncate(&self, len: usize) -> $crate::tools::errors::ASysResult {
            self.$($e)+.truncate(len)
        }
        fn poll_ready(
            &self,
            offset: usize,
            len: usize,
            kind: $crate::fs::new_vfs::top::PollKind,
        ) -> $crate::tools::errors::ASysResult<usize> {
            self.$($e)+.poll_ready(offset, len, kind)
        }
        fn poll_read(&self, offset: usize, buf: &mut [u8]) -> usize {
            self.$($e)+.poll_read(offset, buf)
        }
        fn poll_write(&self, offset: usize, buf: &[u8]) -> usize {
            self.$($e)+.poll_write(offset, buf)
        }
    };
}

#[macro_export]
macro_rules! impl_vfs_forward_attr_getter {
    ($($e:tt)+) => {
        fn attr_kind(&self) -> $crate::fs::new_vfs::VfsFileKind {
            self.$($e)+.attr_kind()
        }
        fn attr_device(&self) -> $crate::fs::new_vfs::top::DeviceInfo {
            self.$($e)+.attr_device()
        }
        fn attr_size(&self) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::SizeInfo> {
            self.$($e)+.attr_size()
        }
        fn attr_time(&self) -> $crate::tools::errors::ASysResult<$crate::fs::new_vfs::top::TimeInfo> {
            self.$($e)+.attr_time()
        }
    };
}

#[macro_export]
macro_rules! impl_vfs_forward_attr_setter {
    ($($e:tt)+) => {
        fn update_time(&self, info: $crate::fs::new_vfs::top::TimeInfoChange) -> $crate::tools::errors::ASysResult {
            self.$($e)+.update_time(info)
        }
    };
}
