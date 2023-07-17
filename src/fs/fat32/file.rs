use super::{dir::GroupDEntryIter, ClusterID, FATDentry, Fat32FS};
use crate::{
    fs::new_vfs::{
        underlying::{ConcreteDEntryRefModification, ConcreteFile},
        VfsFileAttr, VfsFileKind,
    },
    tools::errors::{dyn_future, SysError, SysResult},
};
use alloc::{boxed::Box, vec::Vec};
use core::pin::Pin;
use futures::Stream;

pub struct FATFile {
    pub(super) fs: &'static Fat32FS,
    pub(super) begin_cluster: ClusterID,
    pub(super) last_cluster: Option<ClusterID>,
}

fn round_up(x: usize, y: usize) -> usize {
    (x + y - 1) / y
}

impl FATFile {
    pub fn dentry_iter(&self) -> Pin<Box<dyn Stream<Item = SysResult<FATDentry>>>> {
        Box::pin(GroupDEntryIter::new(self.fs, self.begin_cluster))
    }
}

impl ConcreteFile for FATFile {
    type DEntryRefT = FATDentry;

    fn read_at<'a>(
        &'a self,
        offset: usize,
        buf: &'a mut [u8],
    ) -> crate::tools::errors::ASysResult<usize> {
        let cls_size_byte = self.fs.cluster_size_byte as usize;
        dyn_future(async move {
            // assume that all read from VFS to FAT32 is aligned to a whole page
            let (skip_cls, offset) = self.fs.offset_cls(offset);
            debug_assert!(offset == 0);
            let cnt_cls = round_up(buf.len(), cls_size_byte);

            let range_clss = self.fs.with_fat(|fat_table_mgr| {
                fat_table_mgr.find_range(self.begin_cluster, skip_cls, cnt_cls)
            });
            if let Some(clss) = range_clss {
                let read_len = clss.len() * cls_size_byte;
                let mut buf = buf;
                for cls in clss {
                    let slice = &mut buf[..cls_size_byte];
                    self.fs.read_cluster(cls, slice).await?;
                    buf = &mut buf[cls_size_byte..];
                }
                Ok(read_len)
            } else {
                // read out of range
                Err(SysError::EINVAL)
            }
        })
    }

    fn write_at<'a>(
        &'a self,
        offset: usize,
        buf: &'a [u8],
    ) -> crate::tools::errors::ASysResult<usize> {
        let cls_size_byte = self.fs.cluster_size_byte as usize;
        dyn_future(async move {
            let (skip_cls, offset) = self.fs.offset_cls(offset);
            // assume that all write from VFS to FAT32 is a whole page
            debug_assert!(offset == 0);
            let cnt_cls = round_up(buf.len(), cls_size_byte);

            let range_clss = self.fs.with_fat(|fat_table_mgr| {
                fat_table_mgr.find_range(self.begin_cluster, skip_cls, cnt_cls)
            });
            if let Some(clss) = range_clss {
                let write_len = clss.len() * cls_size_byte;
                let mut buf = buf;
                for cls in clss {
                    let slice = &buf[..cls_size_byte];
                    self.fs.write_cluster(cls, slice).await?;
                    buf = &buf[cls_size_byte..];
                }
                Ok(write_len)
            } else {
                // write out of range
                Err(SysError::EINVAL)
            }
        })
    }

    fn lookup_batch(
        &self,
        _skip_n: usize,
        _name: Option<&str>,
    ) -> crate::tools::errors::ASysResult<(bool, Vec<Self::DEntryRefT>)> {
        todo!()
    }

    fn set_attr(
        &self,
        _dentry_ref: Self::DEntryRefT,
        _attr: VfsFileAttr,
    ) -> crate::tools::errors::ASysResult {
        todo!()
    }

    fn create(
        &self,
        _name: &str,
        _kind: VfsFileKind,
    ) -> crate::tools::errors::ASysResult<Self::DEntryRefT> {
        todo!()
    }

    fn remove(&self, _dentry_ref: Self::DEntryRefT) -> crate::tools::errors::ASysResult {
        todo!()
    }

    fn detach(&self, _dentry_ref: Self::DEntryRefT) -> crate::tools::errors::ASysResult<Self> {
        todo!()
    }

    fn sync_batch<'a, Iter>(&'a self, mod_iter: Iter) -> crate::tools::errors::ASysResult
    where
        Iter: IntoIterator<
                Item = crate::fs::new_vfs::underlying::ConcreteDEntryRefModification<
                    Self::DEntryRefT,
                >,
            > + Send
            + 'a,
    {
        dyn_future(async move {
            let mods: Vec<ConcreteDEntryRefModification<Self::DEntryRefT>> =
                mod_iter.into_iter().collect();
            // 先排序 Truncate 和 Rename
            Ok(())
        })
    }
}
