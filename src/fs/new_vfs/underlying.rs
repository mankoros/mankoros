use super::VfsFileKind;
use crate::tools::errors::ASysResult;
use alloc::{string::String, vec::Vec};

pub trait ConcreteFile: Send + Sync + Sized + 'static {
    // 通用操作
    fn kind(&self) -> VfsFileKind;
    fn size(&self) -> usize;
    fn block_count(&self) -> usize;
    fn device_id(&self) -> usize;
    fn delete(&self) -> ASysResult;

    // 文件操作
    fn read_page_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize>;
    fn write_page_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize>;
    fn truncate(&self, new_size: usize) -> ASysResult;

    // 文件夹操作
    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<Self>;
    fn list(&self) -> ASysResult<Vec<(String, Self)>>;
    fn create<'a>(&'a self, name: &'a str, kind: VfsFileKind) -> ASysResult<Self>;
    fn rename<'a>(&'a self, file: &'a Self, new_name: &'a str) -> ASysResult;
    fn detach<'a>(&'a self, file: &'a Self) -> ASysResult;
    fn attach<'a>(&'a self, file: &'a Self, name: &'a str) -> ASysResult;
}

pub trait ConcreteFS: Sized + 'static {
    type FileT: ConcreteFile;
    fn root(&'static self) -> Self::FileT;
}
