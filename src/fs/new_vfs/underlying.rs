use super::{VfsFileAttr, VfsFileKind};
use crate::tools::errors::ASysResult;
use alloc::{string::String, vec::Vec};

pub trait DEntryRef: Clone + Send + Sync + Sized {
    type FileT: ConcreteFile;
    fn name(&self) -> String;
    fn attr(&self) -> VfsFileAttr;
    fn file(&self) -> Self::FileT;
}

pub trait ConcreteFile: Send + Sync + Sized + 'static {
    type DEntryRefT: DEntryRef<FileT = Self>;

    // 文件操作
    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize>;
    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize>;

    // 文件夹操作
    fn lookup_batch(
        &self,
        skip_n: usize,
        name: Option<&str>,
    ) -> ASysResult<(bool, Vec<Self::DEntryRefT>)>;
    fn set_attr(&self, dentry_ref: Self::DEntryRefT, attr: VfsFileAttr) -> ASysResult;
    fn create(&self, name: &str, kind: VfsFileKind) -> ASysResult<Self::DEntryRefT>;
    fn remove(&self, dentry_ref: Self::DEntryRefT) -> ASysResult;
    fn detach(&self, dentry_ref: Self::DEntryRefT) -> ASysResult<Self>;
}

pub trait ConcreteFS: Sized {
    type FileT: ConcreteFile;
    fn root(&self) -> Self::FileT;
}
