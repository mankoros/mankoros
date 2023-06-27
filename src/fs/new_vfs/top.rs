//! 给予进程使用的顶层模块

use alloc::{sync::Arc, boxed::Box, string::String, vec::Vec};
use super::{inode::VfsNode, underlying::{FsNode, FsFileSystem}, dentry::DirEntry, info::NodeStat, path::Path};
use crate::tools::errors::{ASysResult, SysResult};

pub struct Vfs {
    fs: Box<dyn FsFileSystem>,
    root: Arc<VfsFile>,
}

impl Vfs {
    pub fn new(fs: Box<dyn FsFileSystem>) -> Self {
        let fs_node = fs.root();
        let vfs_node = Arc::new(VfsNode::new(fs_node));
        let vfs_dentry = DirEntry::new_root(vfs_node);
        let vfs_file = VfsFile::new(vfs_dentry);

        Self {
            fs,
            root: vfs_file,
        }
    }

    pub fn mount(&self, mount_point: Arc<VfsFile>) {
        // replace parent
        let name = mount_point.dentry.name();
        if let Some(parent) = mount_point.dentry.parent() {
            parent.link(name, mount_point.dentry.inode());
        }
    }

    pub fn root(&self) -> Arc<VfsFile> {
        self.root.clone()
    }

    pub async fn resolve(&self, path: &Path) -> SysResult<Arc<VfsFile>> {
        self.root.resolve(path).await
    }
}

#[derive(Clone)]
pub struct VfsFile {
    dentry: Arc<DirEntry>,
}

impl VfsFile {
    fn new(dentry: Arc<DirEntry>) -> Self {
        Self { dentry }
    }

    fn node(&self) -> Arc<VfsNode> {
        self.dentry.inode()
    }

    // 文件操作
    /// 获取文件的各类属性,
    /// 例如文件类型, 文件大小, 文件创建时间等等
    pub async fn stat(&self) -> SysResult<NodeStat> {
        self.node().stat().await
    }
    /// 读取文件内容
    pub async fn read_at(&self, offset: usize, buf: &mut [u8]) -> SysResult<usize> {
        self.node().read_at(offset, buf).await
    }
    /// 写入文件内容
    pub async fn write_at(&self, offset: usize, buf: &[u8]) -> SysResult<usize> {
        self.node().write_at(offset, buf).await
    }

    pub async fn link_raw<T: FsNode>(self: &Arc<Self>, name: &str, fs_node: T) -> SysResult<Arc<VfsFile>> {
        let vfs_node = Arc::new(VfsNode::new(fs_node));
        self.dentry.link(name, vfs_node).await.map(Self::new)
    }

    // 文件夹操作
    /// 列出文件夹中的所有文件的名字
    pub async fn list(&self) -> SysResult<Vec<(String, VfsFile)>> {
        self.dentry.list().await.map(
            |l| { l.into_iter().map(|(name, node)| (name, Self::new(node))).collect() })
    }
    /// 根据名字查找文件夹中的文件, 不会递归查找
    pub async fn lookup(&self, name: &str) -> SysResult<Self> {
        self.dentry.lookup(name).await.map(Self::new)
    }
    /// 新建一个文件, 并在当前文件夹中创建一个 名字->新建文件 的映射
    pub async fn create(&self, name: &str, is_dir: bool) -> SysResult<Self> {
        self.dentry.create(name, is_dir).await.map(Self::new)
    }
    /// 在当前文件夹创建一个 名字->文件 的映射
    pub async fn link(&self, name: &str, file: Arc<VfsFile>) -> SysResult<Self> {
        self.dentry.link(name, file.node()).await.map(Self::new)
    }
    /// 在当前文件夹删除一个 名字->文件 的映射
    pub async fn unlink(&self, name: &str) -> SysResult {
        self.dentry.unlink(name).await
    }

    /// 以当前文件夹为根, 递归解析路径
    pub async fn resolve(&self, path: &Path) -> SysResult<Self> {
        let mut dir = self.dentry.clone();
        for name in path.iter() {
            dir = dir.lookup(name).await?;
        }
        return Ok(Self::new(dir));
    }
}