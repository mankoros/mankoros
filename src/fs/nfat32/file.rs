use super::{
    dir::{
        AtomDEPos, AtomDEntryView, Fat32DEntryAttr, GroupDEPos, GroupDEntryIter,
        Standard8p3EntryRepr,
    },
    tools::{ClusterChain, WithDirty},
    ClusterID, Fat32FS, FatDEntryData, SectorID,
};
use crate::{
    fs::new_vfs::{underlying::ConcreteFile, VfsFileAttr, VfsFileKind},
    panic,
    tools::errors::{dyn_future, ASysResult, SysError, SysResult},
};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    vec::Vec,
};
use core::{cmp::Reverse, pin::Pin};
use futures::Stream;
use ringbuffer::RingBufferExt;

pub(super) struct StdEntryEditor {
    pub(super) sector: SectorID,
    pub(super) offset: u16,
    pub(super) std: WithDirty<Standard8p3EntryRepr>,
}

impl StdEntryEditor {
    pub fn std(&self) -> &Standard8p3EntryRepr {
        self.std.as_ref()
    }
    pub fn std_mut(&self) -> &mut Standard8p3EntryRepr {
        self.std.as_mut()
    }

    pub async fn sync(&self, fs: &'static Fat32FS) -> SysResult<()> {
        let bc = fs.block_dev().get(self.sector).await?;
        let ptr = &bc.as_mut_slice()[self.offset as usize..][..32] as *const _ as *mut [u8]
            as *mut Standard8p3EntryRepr;
        unsafe { *ptr = self.std().clone() };
        self.std.clear();
        Ok(())
    }
}

pub struct FATFile {
    fs: &'static Fat32FS,
    editor: StdEntryEditor,
    chain: ClusterChain,
    gde_pos: GroupDEPos,
}

fn round_up(x: usize, y: usize) -> usize {
    (x + y - 1) / y
}

impl FATFile {
    async fn delete_self(&self) -> SysResult {
        todo!()
    }

    async fn attach<'a>(&'a self, data: &FatDEntryData<'a>) -> SysResult<Self> {
        // 遍历 GroupDE, 寻找 unused entry, 然后把 name 和 kind 写进去
        // 如果找不到, 就开 append mode 要求新写入
        todo!()
    }

    async fn detach_impl<'a>(&'a self, file: &'a Self) -> SysResult<GroupDEntryIter> {
        let mut it = GroupDEntryIter::new_middle(self.fs, file.editor.sector, file.gde_pos);
        it.mark_next().await?;
        it.delete_entry();
        Ok(it)
    }

    fn gde_iter(&self) -> GroupDEntryIter {
        GroupDEntryIter::new(self.fs, self.chain.first())
    }

    fn into_file(&self, it: &GroupDEntryIter) -> Self {
        let std_pos = it.std_pos();
        let (sct_id, sct_off) = self.chain.offset_sct(self.fs, std_pos.as_byte_offset());
        let editor = StdEntryEditor {
            sector: sct_id,
            offset: sct_off,
            std: WithDirty::new(it.std_clone()),
        };
        let begin_cluster = it.get_begin_cluster();
        let chain = ClusterChain::new(self.fs, begin_cluster);
        let gde_pos = it.gde_pos();
        Self {
            fs: self.fs,
            editor,
            chain,
            gde_pos,
        }
    }
}

impl ConcreteFile for FATFile {
    fn kind(&self) -> VfsFileKind {
        let kind = self.editor.std().attr();
        if kind.contains(Fat32DEntryAttr::DIRECTORY) {
            VfsFileKind::Directory
        } else {
            VfsFileKind::RegularFile
        }
    }
    fn size(&self) -> usize {
        self.editor.std().size as usize
    }
    fn block_count(&self) -> usize {
        self.chain.len() * (self.fs.cluster_size_sct as usize)
    }
    fn device_id(&self) -> usize {
        self.fs.device_id()
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        // 先从 offset 找到 sector, 然后逐个 sector 去无缓存地读
        // 读的过程中与 buf.len 取个 min, 到 0 了或者没 sector 了就结束
        todo!()
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        todo!()
    }

    fn truncate<'a>(&'a self, new_size: usize) -> ASysResult {
        // 如果是文件夹，不允许 truncate
        // 如果是文件, 那么根据 new_size 是大是小决定
        // 如果小, 则调整 chain, 把多出来的块还给 fs
        // 如果大, 则向 fs 要新的块并更新 chain
        todo!()
    }

    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<Self> {
        // 通过一个 GroupDE Iter 遍历, 然后寻找 name 相同的
        dyn_future(async move {
            let mut it = self.gde_iter();
            while it.mark_next().await?.is_some() {
                if it.collect_name() == name {
                    return Ok(self.into_file(&it));
                }
                it.leave_next().await?;
            }
            Err(SysError::ENOENT)
        })
    }

    fn list(&self) -> ASysResult<Vec<(alloc::string::String, Self)>> {
        // 遍历 GroupDE, 然后把每个 entry 的 name 和对应的 file 构造出来
        dyn_future(async move {
            let mut it = self.gde_iter();
            let mut res = Vec::new();
            while it.mark_next().await?.is_some() {
                let name = it.collect_name();
                let file = self.into_file(&it);
                res.push((name, file));
                it.leave_next().await?;
            }
            Ok(res)
        })
    }

    fn create<'a>(&'a self, name: &'a str, kind: VfsFileKind) -> ASysResult<Self> {
        // 先向 fs 申请新创文件, 然后 attach 上去
        dyn_future(async move {
            let begin_cluster = self.fs.with_fat(|f| f.alloc());
            let attr = match kind {
                VfsFileKind::Directory => Fat32DEntryAttr::DIRECTORY,
                VfsFileKind::RegularFile => Fat32DEntryAttr::ARCHIVE,
                _ => panic!("unsupported file kind"),
            };
            let data = FatDEntryData {
                name,
                attr,
                begin_cluster,
                size: 0,
            };
            self.attach(&data).await
        })
    }

    fn remove<'a>(&'a self, file: &'a Self) -> ASysResult {
        // detach, 然后通知 fs 递归回收后边文件的 cluster
        dyn_future(async move {
            self.detach(file).await?;
            file.delete_self().await
        })
    }

    fn rename<'a>(&'a self, file: &'a Self, new_name: &'a str) -> ASysResult {
        // detach, 然后 attach
        dyn_future(async move {
            let mut it = self.detach_impl(file).await?;
            let data = FatDEntryData {
                attr: file.editor.std().attr(),
                begin_cluster: file.chain.first(),
                size: file.editor.std().size,
                name: new_name,
            };
            if it.can_create(&data) {
                // 如果这个名字足够小, 则可以直接写回原来的地方
                it.create_entry(&data).await?;
            } else {
                // 从头开始重新查找空闲位置, 或者在最末尾写入
                self.attach(&data).await?;
            }
            Ok(())
        })
    }

    fn detach<'a>(&'a self, file: &'a Self) -> ASysResult {
        // file 中包含 GDEPos, 所以可以直接定位到具体的 Sector, 使用 GDEIter 写入之即可
        dyn_future(async move {
            let mut it = self.detach_impl(file).await?;
            it.leave_next().await?;
            Ok(())
        })
    }
}
