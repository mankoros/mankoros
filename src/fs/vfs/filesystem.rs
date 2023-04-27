use alloc::sync::Arc;

use crate::ax_err;

use super::node::{VfsDirEntry, VfsNodeAttr, VfsNodeType};
use super::VfsResult;

pub struct FileSystemInfo;
pub type VfsNodeRef = Arc<dyn VfsNode>;
pub trait Vfs: Send + Sync {
    /// mount operation
    fn mount(&self, _path: &str, _mount_point: VfsNodeRef) -> VfsResult {
        Ok(())
    }

    /// unmount operation
    fn unmount(&self) -> VfsResult {
        Ok(())
    }

    /// Format the filesystem
    fn format(&self) -> VfsResult {
        ax_err!(Unsupported)
    }

    /// Get the root node of the filesystem
    fn root_dir(&self) -> VfsNodeRef;

    /// Get the attributes of the filesystem
    fn statfs(&self) -> VfsResult<FileSystemInfo> {
        ax_err!(Unsupported)
    }
}

pub trait VfsNode: Send + Sync {
    /// open operation
    fn open(&self) -> VfsResult {
        Ok(())
    }

    /// close operation
    fn release(&self) -> VfsResult {
        Ok(())
    }

    /// Get the attributes of the node
    fn stat(&self) -> VfsResult<VfsNodeAttr>;

    // file operations

    /// Read data from the file at the given offset.
    fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> VfsResult<usize> {
        ax_err!(InvalidInput)
    }

    /// Write data to the file at the given offset.
    fn write_at(&self, _offset: u64, _buf: &[u8]) -> VfsResult<usize> {
        ax_err!(InvalidInput)
    }

    /// Flush the file, synchronize the data to disk.
    fn fsync(&self) -> VfsResult {
        ax_err!(InvalidInput)
    }

    /// Truncate the file to the given size.
    fn truncate(&self, _size: u64) -> VfsResult {
        ax_err!(InvalidInput)
    }

    // directory operations:

    /// Get the parent directory of this directory.
    ///
    /// Return `None` if the node is a file.
    fn parent(&self) -> Option<VfsNodeRef> {
        None
    }

    /// Lookup the node with given `path` in the directory.
    ///
    /// Return the node if found.
    fn lookup(self: Arc<Self>, _path: &str) -> VfsResult<VfsNodeRef> {
        ax_err!(Unsupported)
    }

    /// Create a new node with the given `path` in the directory
    ///
    /// Return [`Ok(())`](Ok) if it already exists.
    fn create(&self, _path: &str, _ty: VfsNodeType) -> VfsResult {
        ax_err!(Unsupported)
    }

    /// Remove the node with the given `path` in the directory.
    fn remove(&self, _path: &str) -> VfsResult {
        ax_err!(Unsupported)
    }

    /// Read directory entries into `dirents`, starting from `start_idx`.
    fn read_dir(&self, _start_idx: usize, _dirents: &mut [VfsDirEntry]) -> VfsResult<usize> {
        ax_err!(Unsupported)
    }
}

/// When implement [`VfsNodeOps`] on a directory node, add dummy file operations
/// that just returns an error.
#[macro_export]
macro_rules! impl_vfs_dir_default {
    () => {
        fn read_at(&self, _offset: u64, _buf: &mut [u8]) -> crate::fs::vfs::VfsResult<usize> {
            crate::ax_err!(IsADirectory)
        }

        fn write_at(&self, _offset: u64, _buf: &[u8]) -> crate::fs::vfs::VfsResult<usize> {
            crate::ax_err!(IsADirectory)
        }

        fn fsync(&self) -> crate::fs::vfs::VfsResult {
            crate::ax_err!(IsADirectory)
        }

        fn truncate(&self, _size: u64) -> crate::fs::vfs::VfsResult {
            crate::ax_err!(IsADirectory)
        }
    };
}

/// When implement [`VfsNodeOps`] on a non-directory node, add dummy directory
/// operations that just returns an error.
#[macro_export]
macro_rules! impl_vfs_non_dir_default {
    () => {
        fn lookup(
            self: alloc::sync::Arc<Self>,
            _path: &str,
        ) -> crate::fs::vfs::VfsResult<crate::fs::vfs::filesystem::VfsNodeRef> {
            crate::ax_err!(NotADirectory)
        }

        fn create(
            &self,
            _path: &str,
            _ty: crate::fs::vfs::node::VfsNodeType,
        ) -> crate::fs::vfs::VfsResult {
            crate::ax_err!(NotADirectory)
        }

        fn remove(&self, _path: &str) -> crate::fs::vfs::VfsResult {
            crate::ax_err!(NotADirectory)
        }

        fn read_dir(
            &self,
            _start_idx: usize,
            _dirents: &mut [crate::fs::vfs::node::VfsDirEntry],
        ) -> crate::fs::vfs::VfsResult<usize> {
            crate::ax_err!(NotADirectory)
        }
    };
}
