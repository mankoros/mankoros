use alloc::boxed::Box;

use crate::{
    fs::vfs::{
        filesystem::VfsNode,
        node::{VfsNodeAttr, VfsNodePermission, VfsNodeType},
        AVfsResult, VfsResult,
    },
    impl_vfs_non_dir_default,
};

// A zero device behaves like `/dev/zero`.
///
/// It always returns a chunk of `\0` bytes when read, and all writes are discarded.
#[derive(Debug, Clone)]
pub struct ZeroDev;

impl VfsNode for ZeroDev {
    fn stat(&self) -> VfsResult<VfsNodeAttr> {
        Ok(VfsNodeAttr::new(
            VfsNodePermission::default_file(),
            VfsNodeType::CharDevice,
            0,
            0,
        ))
    }

    fn sync_read_at(&self, _offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        buf.fill(0);
        Ok(buf.len())
    }

    fn sync_write_at(&self, _offset: u64, buf: &[u8]) -> VfsResult<usize> {
        Ok(buf.len())
    }
    fn read_at<'a>(&'a self, offset: u64, buf: &'a mut [u8]) -> AVfsResult<usize> {
        Box::pin(async move { self.sync_read_at(offset, buf) })
    }

    fn write_at<'a>(&'a self, offset: u64, buf: &'a [u8]) -> AVfsResult<usize> {
        Box::pin(async move { self.sync_write_at(offset, buf) })
    }

    fn truncate(&self, _size: u64) -> VfsResult {
        Ok(())
    }

    impl_vfs_non_dir_default! {}
}
