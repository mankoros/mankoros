use alloc::{sync::Arc, string::String, vec::Vec};
use super::{VfsFileAttr, VfsFileKind};
use crate::{tools::errors::ASysResult, memory::address::PhysAddr4K};

pub type VfsFileRef = Arc<dyn VfsFile>;

pub trait FsFileSystem {
    fn root(&self) -> VfsFileRef;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmapKind {
    Shared,
    Private,
}

pub trait VfsFile : Send + Sync {
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

#[macro_export]
macro_rules! impl_vfs_default_non_dir {
    ($ty:ident) => {
        fn list(&self) -> crate::tools::errors::ASysResult<alloc::vec::Vec<(alloc::string::String, crate::fs::new_vfs::top::VfsFileRef)>> {
            unimplemented!(concat!(stringify!($ty), "::list"))
        }
        fn lookup(&self, _name: &str) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::lookup"))
        }
        fn create(&self, _name: &str, _kind: crate::fs::new_vfs::VfsFileKind) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::create"))
        }
        fn remove(&self, _name: &str) -> crate::tools::errors::ASysResult {
            unimplemented!(concat!(stringify!($ty), "::remove"))
        }
        fn detach(&self, _name: &str) -> crate::tools::errors::ASysResult<crate::fs::new_vfs::top::VfsFileRef> {
            unimplemented!(concat!(stringify!($ty), "::detach"))
        }
        fn attach(&self, _name: &str, _node: crate::fs::new_vfs::top::VfsFileRef) -> crate::tools::errors::ASysResult {
            unimplemented!(concat!(stringify!($ty), "::attach"))
        }
    };
}

#[macro_export]
macro_rules! impl_vfs_default_non_file {
    ($ty:ident) => {
        fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> crate::tools::errors::ASysResult<usize> {
            unimplemented!(concat!(stringify!(ty), "::read_at"))
        }
        fn write_at(&self, _offset: usize, _buf: &[u8]) -> crate::tools::errors::ASysResult<usize> {
            unimplemented!(concat!(stringify!(ty), "::write_at"))
        }
        fn get_page(&self, _offset: usize, _kind: crate::fs::new_vfs::top::MmapKind) -> crate::tools::errors::ASysResult<crate::memory::address::PhysAddr4K> {
            unimplemented!(concat!(stringify!(ty), "::get_page"))
        }
    };
}