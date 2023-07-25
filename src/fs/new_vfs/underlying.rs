use super::{VfsFileAttr, VfsFileKind};
use crate::tools::errors::ASysResult;
use alloc::{string::String, vec::Vec};

pub trait ConcreteFile: Send + Sync + Sized + 'static {
    // 通用操作
    fn kind(&self) -> VfsFileKind;
    fn size(&self) -> usize;
    fn block_count(&self) -> usize;
    fn device_id(&self) -> usize;

    // 文件操作
    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize>;
    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize>;
    fn truncate<'a>(&'a self, new_size: usize) -> ASysResult;

    // 文件夹操作
    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<Self>;
    fn list(&self) -> ASysResult<Vec<(String, Self)>>;
    fn create<'a>(&'a self, name: &'a str, kind: VfsFileKind) -> ASysResult<Self>;
    fn remove(&self, file: &Self) -> ASysResult;
    fn rename(&self, file: &Self, new_name: &str) -> ASysResult;
    fn detach(&self, file: &Self) -> ASysResult<Self>;
}

pub trait ConcreteFS: Sized {
    type FileT: ConcreteFile;
    fn root(&self) -> Self::FileT;
}
