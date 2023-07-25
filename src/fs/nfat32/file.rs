use super::{
    dir::{Fat32DEntryAttr, GroupDEPos, GroupDEntryIter},
    ClusterID, FATDentry, Fat32FS,
};
use crate::{
    fs::new_vfs::{
        underlying::{
            ConcreteDEntryRef, ConcreteDEntryRefModification, ConcreteDEntryRefModificationKind,
            ConcreteFile,
        },
        VfsFileAttr, VfsFileKind,
    },
    tools::errors::{dyn_future, SysError, SysResult},
};
use alloc::{boxed::Box, string::ToString, vec::Vec};
use core::{cmp::Reverse, pin::Pin};
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

            let cluster_chain = self.fs.with_fat(|fat_table_mgr| {
                fat_table_mgr.find_range(self.begin_cluster, skip_cls, cnt_cls)
            });
            if let Some(cluster_chain) = cluster_chain {
                let read_len = cluster_chain.len() * cls_size_byte;
                let mut buf = buf;
                for cls in cluster_chain {
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

    fn lookup_batch<'a>(
        &'a self,
        skip_n: usize,
        name: Option<&'a str>,
    ) -> crate::tools::errors::ASysResult<(bool, Vec<Self::DEntryRefT>)> {
        dyn_future(async move {
            let mut result = Vec::new();

            let mut skip_idx = 0;
            let mut gde_iter = GroupDEntryIter::new(self.fs, self.begin_cluster);
            while let Some(_) = gde_iter.mark_next().await? {
                if skip_idx < skip_n {
                    skip_idx += 1;
                    continue;
                }

                result.push(gde_iter.collect_dentry());
                let gde_name = &result.last().unwrap().name;
                if let Some(name) = name && gde_name == name {
                    // we have found the target & we havn't conusme all the entries
                    return Ok((false, result));
                }
                gde_iter.leave_next().await?;
            }
            // we have consumed all the entries
            Ok((true, result))
        })
    }

    fn set_attr(
        &self,
        _dentry_ref: Self::DEntryRefT,
        _attr: VfsFileAttr,
    ) -> crate::tools::errors::ASysResult {
        todo!()
    }

    fn create<'a>(
        &'a self,
        name: &'a str,
        kind: VfsFileKind,
    ) -> crate::tools::errors::ASysResult<Self::DEntryRefT> {
        dyn_future(async move {
            let begin_cluster = self.fs.with_fat(|f| f.alloc());
            let attr = match kind {
                VfsFileKind::RegularFile => Fat32DEntryAttr::from_bits(0).unwrap(),
                VfsFileKind::Directory => Fat32DEntryAttr::DIRECTORY,
                _ => panic!("unsupported file kind"),
            };
            Ok(FATDentry {
                fs: self.fs,
                pos: GroupDEPos::null(),
                attr,
                begin_cluster,
                name: name.to_string(),
                size: 0,
            })
        })
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
            // 先排序 Truncate 和 Rename
            let mut std_change_or_delete = Vec::new();
            let mut creations = Vec::new();

            for modi in mod_iter {
                use ConcreteDEntryRefModificationKind::*;
                match modi.kind {
                    Rename(_) => {
                        let (d, c) = modi.split_rename();
                        std_change_or_delete.push(d);
                        creations.push((true, c));
                    }
                    Truncate(_) | Detach | Delete => {
                        std_change_or_delete.push(modi);
                    }
                    Create => {
                        creations.push((true, modi));
                    }
                }
            }

            std_change_or_delete.sort_by_cached_key(|f| {
                f.dentry_ref.pos().expect("DEntry ref in Truncate/Delete must have valid pos")
            });
            creations.sort_by_cached_key(|(_, f)| Reverse(f.dentry_ref.name().len()));

            // 依次遍历 GDE, 由于 std_change_or_delete 是有序的,
            // 所以我们可以保证只看最前面即可

            let mut gde_iter = GroupDEntryIter::new(self.fs, self.begin_cluster);
            while let Some(_) = gde_iter.mark_next().await? {
                if let Some(first) = std_change_or_delete.first() {
                    use ConcreteDEntryRefModificationKind::*;
                    let first_pos = first.dentry_ref.pos().unwrap();
                    if first_pos == gde_iter.pos() {
                        // 该 GDE 有变化
                        match first.kind {
                            Truncate(new_size) => gde_iter.change_size(new_size as u32),
                            Delete => {
                                gde_iter.delete_entry();
                                // TODO: delete the files
                            }
                            Detach => gde_iter.delete_entry(),
                            Create | Rename(_) => unreachable!(),
                        }
                    }
                    // 如果该 GDE 为 unused, 或者在上边被 delete 了,
                    // 就可以尝试放置 creation
                    if gde_iter.can_create_any() {
                        // 由大到小尝试放置 creation
                        for (valid, f) in creations.iter_mut() {
                            debug_assert!(matches!(f.kind, Create));
                            if gde_iter.can_create(&f.dentry_ref) {
                                gde_iter.create_entry(&f.dentry_ref).await?;
                            }
                            *valid = false;
                        }
                        creations.retain(|(v, _)| *v);
                    }
                } else {
                    // std_change_or_delete is empty
                    if creations.is_empty() {
                        // if creation is empty too, we have nothing to do next
                        // just break
                        break;
                    }
                }

                gde_iter.leave_next().await?;
            }

            // 如果遍历完了所有已经存在的 GDE, 但是还有 creation 没有放置,
            // 那么就需要进入追加模式新建 GDE.
            if !creations.is_empty() {
                gde_iter.append_enter();
                for (_, f) in creations {
                    gde_iter.append(&f.dentry_ref).await?;
                }
                gde_iter.append_end().await?;
            }

            gde_iter.sync_all().await
        })
    }
}

impl ConcreteDEntryRefModification<FATDentry> {
    pub fn split_rename(self) -> (Self, Self) {
        let new_name = match self.kind {
            ConcreteDEntryRefModificationKind::Rename(new_name) => new_name,
            _ => panic!("not a rename"),
        };

        let detach = Self::new_detach(self.dentry_ref.clone());
        let new_dentry_ref = FATDentry {
            fs: self.dentry_ref.fs,
            pos: GroupDEPos::null(),
            attr: self.dentry_ref.attr,
            begin_cluster: self.dentry_ref.begin_cluster,
            name: new_name,
            size: self.dentry_ref.size,
        };
        let create = Self::new_create(new_dentry_ref);

        (detach, create)
    }
}
