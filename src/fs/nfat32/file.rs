use super::{
    dir::{Fat32DEntryAttr, GroupDEPos, GroupDEntryIter, Standard8p3EntryRepr},
    tools::{ClusterChain, WithDirty},
    ClusterID, Fat32FS, FatDEntryData, SectorID,
};
use crate::{
    executor::block_on,
    fs::{
        disk::{BLOCK_SIZE, LOG2_BLOCK_SIZE},
        new_vfs::{underlying::ConcreteFile, VfsFileKind},
    },
    tools::errors::{dyn_future, ASysResult, SysError, SysResult},
};
use alloc::vec::Vec;
use core::cell::SyncUnsafeCell;

pub(super) struct DEntryPosInfo {
    /// 整个 GroupDE 的位置
    gde_pos: GroupDEPos,
    /// Standard Entry 的位置
    sector: SectorID,
    /// Standard Entry 的偏移
    offset: u16,
}

pub(super) struct StdEntryEditor {
    pub(super) pos: SyncUnsafeCell<DEntryPosInfo>,
    pub(super) std: WithDirty<Standard8p3EntryRepr>,
}

impl StdEntryEditor {
    pub fn new_normal(pos: DEntryPosInfo, std: Standard8p3EntryRepr) -> Self {
        Self {
            pos: SyncUnsafeCell::new(pos),
            std: WithDirty::new(std),
        }
    }

    pub fn new_free() -> Self {
        Self {
            pos: SyncUnsafeCell::new(DEntryPosInfo {
                gde_pos: GroupDEPos::null(),
                sector: 0,
                offset: 0,
            }),
            std: WithDirty::new(Standard8p3EntryRepr::new_empty()),
        }
    }

    fn pos(&self) -> &mut DEntryPosInfo {
        unsafe { &mut *self.pos.get() }
    }

    pub fn is_free(&self) -> bool {
        self.pos().gde_pos == GroupDEPos::null()
    }
    pub fn set_free(&self) {
        self.pos().gde_pos = GroupDEPos::null();
    }
    pub fn set_pos(&self, pos: DEntryPosInfo) {
        *(self.pos()) = pos;
    }

    pub fn gde_pos(&self) -> GroupDEPos {
        self.pos().gde_pos
    }
    pub fn sector(&self) -> SectorID {
        self.pos().sector
    }
    pub fn offset(&self) -> u16 {
        self.pos().offset
    }

    pub fn std(&self) -> &Standard8p3EntryRepr {
        self.std.as_ref()
    }
    pub fn std_mut(&self) -> &mut Standard8p3EntryRepr {
        self.std.as_mut()
    }

    pub async fn sync(&self, fs: &'static Fat32FS) -> SysResult<()> {
        if self.std.dirty() && !self.is_free() {
            let bc = fs.block_dev().get(self.sector()).await?;
            let ptr = &bc.as_mut_slice()[self.offset() as usize..][..32] as *const _ as *mut [u8]
                as *mut Standard8p3EntryRepr;
            unsafe { *ptr = self.std().clone() };
            self.std.clear();
        }
        Ok(())
    }
}

pub struct FATFile {
    pub(super) fs: &'static Fat32FS,
    pub(super) editor: StdEntryEditor,
    pub(super) chain: ClusterChain,
}

fn round_up(x: usize, y: usize) -> usize {
    (x + y - 1) / y
}

impl FATFile {
    pub fn new_free(fs: &'static Fat32FS, begin_cluster: ClusterID) -> Self {
        Self {
            fs,
            editor: StdEntryEditor::new_free(),
            chain: ClusterChain::new(fs, begin_cluster),
        }
    }

    // 真的要在具体 FS 层支持递归删除吗? 感觉似乎可以放上 VFS 做
    fn delete_recursive(&self) -> ASysResult {
        // 递归的 async 函数必须 Box
        dyn_future(async {
            match self.kind() {
                VfsFileKind::RegularFile => self.delete_self(),
                VfsFileKind::Directory => {
                    let mut it = self.gde_iter();
                    while it.mark_next().await?.is_some() {
                        let file = self.into_file(&it);
                        file.delete_recursive().await?;
                        it.leave_next().await?;
                    }
                    self.delete_self()
                }
                _ => panic!("unsupported file kind"),
            }
        })
    }
    fn delete_self(&self) -> SysResult {
        // TODO: debug mode 下给块里写满 0xdeadbeef, 用于 debug
        self.chain.free_all(self.fs);
        Ok(())
    }

    async fn attach_impl<'a>(&'a self, data: &FatDEntryData<'a>, file: &FATFile) -> SysResult {
        debug_assert!(file.editor.is_free());
        // 遍历 GroupDE, 寻找 unused entry, 然后把 name 和 kind 写进去
        // 如果找不到, 就开 append mode 要求新写入
        let mut it = self.gde_iter();
        while it.mark_next().await?.is_some() {
            if it.can_create(data) {
                it.create_entry(data).await?;
                file.editor.set_pos(self.into_de_pos(&it));
            }
            it.leave_next().await?;
        }
        it.append_enter();
        // TODO: 这对吗? 看起来不太对劲
        it.append(data).await?;
        file.editor.set_pos(self.into_de_pos(&it));
        it.append_end().await?;
        Ok(())
    }

    async fn detach_impl<'a>(&'a self, file: &'a Self) -> SysResult<GroupDEntryIter> {
        let mut it =
            GroupDEntryIter::new_middle(self.fs, file.editor.sector(), file.editor.gde_pos());
        it.mark_next().await?;
        it.delete_entry();
        file.editor.set_free();
        Ok(it)
    }

    fn gde_iter(&self) -> GroupDEntryIter {
        GroupDEntryIter::new(self.fs, self.chain.first())
    }

    fn into_de_pos(&self, it: &GroupDEntryIter) -> DEntryPosInfo {
        let (sector, offset) = self.chain.offset_sct(self.fs, it.std_pos().as_byte_offset());
        DEntryPosInfo {
            gde_pos: it.gde_pos(),
            sector,
            offset,
        }
    }

    fn into_de_editor(&self, it: &GroupDEntryIter) -> StdEntryEditor {
        StdEntryEditor::new_normal(self.into_de_pos(it), it.std_clone())
    }

    fn into_file(&self, it: &GroupDEntryIter) -> Self {
        let editor = self.into_de_editor(it);
        let begin_cluster = it.get_begin_cluster();
        let chain = ClusterChain::new(self.fs, begin_cluster);
        Self {
            fs: self.fs,
            editor,
            chain,
        }
    }

    async fn sync_metadata(&self) {
        self.editor.sync(self.fs).await.unwrap();
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
    fn delete(&self) -> ASysResult {
        dyn_future(async move {
            self.delete_self()?;
            Ok(())
        })
    }

    fn read_page_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        // 假设 VFS 上层都是按页读取的, 那么这意味着 offset 一定是 sector 对齐的,
        // 并且 buf 的长度一定是 sector 的整数.
        debug_assert!(offset % BLOCK_SIZE == 0);
        debug_assert!(buf.len() % BLOCK_SIZE == 0);

        dyn_future(async move {
            let (sct_id, sct_off) = self.chain.offset_sct(self.fs, offset);
            debug_assert!(sct_off == 0);

            let mut buf = buf;
            let mut sid = sct_id;
            let mut read_len = 0;
            while !buf.is_empty() {
                self.fs.read_sector(sid, buf).await?;
                let len = core::cmp::min(buf.len(), BLOCK_SIZE);
                buf = &mut buf[len..];
                read_len += BLOCK_SIZE;
                if let Some(next_sid) = self.fs.next_sector(sid) {
                    sid = next_sid;
                } else {
                    break;
                }
            }

            Ok(read_len)
        })
    }

    fn write_page_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        // 大体上与 read_at 相同
        debug_assert!(offset % BLOCK_SIZE == 0);
        debug_assert!(buf.len() % BLOCK_SIZE == 0);
        dyn_future(async move {
            let (sct_id, sct_off) = self.chain.offset_sct(self.fs, offset);
            debug_assert!(sct_off == 0);

            let mut buf = buf;
            let mut sid = sct_id;
            let mut write_len = 0;
            while !buf.is_empty() {
                self.fs.write_sector(sid, buf).await?;
                let len = core::cmp::min(buf.len(), BLOCK_SIZE);
                buf = &buf[len..];
                write_len += BLOCK_SIZE;
                if let Some(next_sid) = self.fs.next_sector(sid) {
                    sid = next_sid;
                } else {
                    break;
                }
            }

            Ok(write_len)
        })
    }

    fn truncate<'a>(&'a self, new_size: usize) -> ASysResult {
        // 如果是文件夹，不允许 truncate
        // 如果是文件, 那么根据 new_size 是大是小决定
        // 如果小, 则调整 chain, 把多出来的块还给 fs
        // 如果大, 则向 fs 要新的块并更新 chain
        dyn_future(async move {
            let new_size_cls = new_size >> (self.fs.log_cls_size_sct as usize + LOG2_BLOCK_SIZE);
            let old_size_cls = self.chain.len();

            // alloc/free cluster
            if new_size_cls > old_size_cls {
                self.chain.alloc_push(new_size_cls - old_size_cls, self.fs);
            } else if new_size_cls < self.chain.len() {
                self.chain.free_pop(old_size_cls - new_size_cls, self.fs);
            } else {
                debug_assert!(new_size_cls == self.chain.len());
                // do nothing
            }

            // update the size in DEntry
            self.editor.std_mut().size = new_size as u32;
            Ok(())
        })
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
            let file = FATFile {
                fs: self.fs,
                editor: StdEntryEditor::new_free(),
                chain: ClusterChain::new(self.fs, begin_cluster),
            };
            self.attach_impl(&data, &file).await?;
            Ok(file)
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
                file.editor.set_pos(self.into_de_pos(&it));
            } else {
                // 从头开始重新查找空闲位置, 或者在最末尾写入
                self.attach_impl(&data, file).await?;
            }
            Ok(())
        })
    }

    fn attach<'a>(&'a self, file: &'a Self, name: &'a str) -> ASysResult {
        dyn_future(async move {
            let data = FatDEntryData {
                attr: file.editor.std().attr(),
                begin_cluster: file.chain.first(),
                size: file.editor.std().size,
                name,
            };
            self.attach_impl(&data, file).await?;
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

impl Drop for FATFile {
    fn drop(&mut self) {
        // TODO: 也许可以用 spawn 直接把它加入到调度器就完事了?
        block_on(self.sync_metadata());
    }
}
