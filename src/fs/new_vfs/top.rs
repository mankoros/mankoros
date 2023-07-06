use super::{path::Path, VfsFileAttr, VfsFileKind};
use crate::{
    memory::address::PhysAddr4K,
    tools::errors::{ASysResult, SysResult},
};
use alloc::{string::String, sync::Arc, vec::Vec};
use core::ops::{Deref, DerefMut};

pub trait VfsFS {
    fn root(&self) -> VfsFileRef;
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

pub const OFFSET_TAIL: usize = usize::MAX;

pub trait VfsFile: Send + Sync {
    // 文件操作
    /// 获取文件的各类属性,
    /// 例如文件类型, 文件大小, 文件创建时间等等
    fn attr(&self) -> ASysResult<VfsFileAttr>;
    /// 读取文件内容
    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize>;
    /// 写入文件内容
    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize>;
    /// 获得代表文件 [offset, offset + PAGE_SIZE) 范围内内容的物理页.
    /// offset 必须是 PAGE_SIZE 的倍数.
    fn get_page(&self, offset: usize, kind: MmapKind) -> ASysResult<PhysAddr4K>;

    // 高级文件操作
    /// 要求文件准备好 [offset, offset + len) 范围内的内容以供读取或写入.
    /// 如果没准备好会返回 Pending, 准备好了就会 wake / 直接返回 Ready.
    fn poll_ready(&self, offset: usize, len: usize, kind: PollKind) -> ASysResult<usize>;
    /// 阻塞地读取文件内容, 逻辑上应该只在 poll_ready 返回 Ready 之后调用.
    fn poll_read(&self, offset: usize, buf: &mut [u8]) -> usize;
    /// 阻塞地写入文件内容, 逻辑上应该只在 poll_ready 返回 Ready 之后调用.
    fn poll_write(&self, offset: usize, buf: &[u8]) -> usize;

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

    pub async fn resolve(&self, path: &Path) -> SysResult<Self> {
        let mut cur = self.clone();
        for name in path.iter() {
            cur = cur.lookup(name).await?;
        }
        Ok(cur)
    }
}

#[macro_export]
macro_rules! ensure_offset_is_tail {
    ($offset:expr) => {
        if $offset != $crate::fs::new_vfs::top::OFFSET_TAIL {
            panic!("offset must be OFFSET_TAIL, but: {}", $offset);
        }
    };
}

#[macro_export]
macro_rules! impl_vfs_default_non_dir {
    ($ty:ident) => {
        fn list(
            &self,
        ) -> crate::tools::errors::ASysResult<
            alloc::vec::Vec<(alloc::string::String, crate::fs::new_vfs::top::VfsFileRef)>,
        > {
            unimplemented!(concat!(stringify!($ty), "::list"))
        }
        fn lookup(
            &self,
            _name: &str,
        ) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::lookup"))
        }
        fn create(
            &self,
            _name: &str,
            _kind: crate::fs::new_vfs::VfsFileKind,
        ) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::create"))
        }
        fn remove(&self, _name: &str) -> crate::tools::errors::ASysResult {
            unimplemented!(concat!(stringify!($ty), "::remove"))
        }
        fn detach(
            &self,
            _name: &str,
        ) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
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
        ) -> crate::tools::errors::ASysResult<usize> {
            unimplemented!(concat!(stringify!(ty), "::read_at"))
        }
        fn write_at(&self, _offset: usize, _buf: &[u8]) -> crate::tools::errors::ASysResult<usize> {
            unimplemented!(concat!(stringify!(ty), "::write_at"))
        }
        fn get_page(
            &self,
            _offset: usize,
            _kind: crate::fs::new_vfs::top::MmapKind,
        ) -> crate::tools::errors::ASysResult<crate::memory::address::PhysAddr4K> {
            unimplemented!(concat!(stringify!(ty), "::get_page"))
        }
        fn poll_ready(
            &self,
            offset: usize,
            len: usize,
            kind: crate::fs::new_vfs::top::PollKind,
        ) -> crate::tools::errors::ASysResult<usize> {
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
        fn list(&self) -> crate::tools::errors::ASysResult<alloc::vec::Vec<(alloc::string::String, crate::fs::new_vfs::top::VfsFileRef)>> {
            self.$($e)+.list()
        }
        fn lookup<'a>(&'a self, name: &'a str) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
            self.$($e)+.lookup(name)
        }
        fn create<'a>(&'a self, name: &'a str, kind: crate::fs::new_vfs::VfsFileKind) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
            self.$($e)+.create(name, kind)
        }
        fn remove<'a>(&'a self, name: &'a str) -> crate::tools::errors::ASysResult {
            self.$($e)+.remove(name)
        }
        fn detach<'a>(&'a self, name: &'a str) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
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
        fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> crate::tools::errors::ASysResult<usize> {
            self.$($e)+.read_at(offset, buf)
        }
        fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> crate::tools::errors::ASysResult<usize> {
            self.$($e)+.write_at(offset, buf)
        }
        fn get_page(&self, offset: usize, kind: crate::fs::new_vfs::top::MmapKind) -> crate::tools::errors::ASysResult<crate::memory::address::PhysAddr4K> {
            self.$($e)+.get_page(offset, kind)
        }
        fn poll_ready(
            &self,
            offset: usize,
            len: usize,
            kind: crate::fs::new_vfs::top::PollKind,
        ) -> crate::tools::errors::ASysResult<usize> {
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
