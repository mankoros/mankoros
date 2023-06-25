//! 给予进程使用的顶层模块

use alloc::{sync::Arc, boxed::Box};
use super::{inode::VfsNode, underlying::{FsNode, FsFileSystem}, dentry::DirEntry};

struct Vfs {
    fs: Box<dyn FsFileSystem>,
    root: VfsFile,
}

impl Vfs {
    pub fn new(fs: Box<dyn FsFileSystem>) -> Self {
        let fs_node = fs.root();
        let vfs_node = Arc::new(VfsNode::new(fs_node));
        let vfs_dentry = DirEntry::new_root(vfs_node.clone());
        let vfs_file = VfsFile::new(vfs_node, vfs_dentry);

        Self {
            fs,
            root: vfs_file,
        }
    }

    pub fn root(&self) -> Arc<VfsNode> {
        self.root.node.clone()
    }
}


struct VfsFile {
    node: Arc<VfsNode>,
    dentry: Arc<DirEntry>,
}

impl VfsFile {
    fn new(node: Arc<VfsNode>, dentry: Arc<DirEntry>) -> Self {
        Self {
            node,
            dentry,
        }
    }
}