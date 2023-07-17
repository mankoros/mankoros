use super::{ClsOffsetT, ClusterID, FATFile, Fat32FS, SectorID};
use crate::{
    fs::{
        disk::BLOCK_SIZE,
        fat32::parse,
        new_vfs::{underlying::ConcreteDEntryRef, VfsFileAttr, VfsFileKind},
    },
    tools::errors::SysResult,
};
use alloc::{boxed::Box, collections::VecDeque, string::String, vec::Vec};
use core::{
    async_iter::AsyncIterator, cell::SyncUnsafeCell, cmp::min, future::Future, pin::pin, slice,
    task::Poll,
};
use futures::Stream;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    pub(super) struct Fat32DEntryAttr : u8 {
        const READ_ONLY = 0x01;
        const HIDDEN = 0x02;
        const SYSTEM = 0x04;
        const VOLUME_ID = 0x08;
        const DIRECTORY = 0x10;
        const ARCHIVE = 0x20;
        const LFN =
            Self::READ_ONLY.bits() | Self::HIDDEN.bits() |
            Self::SYSTEM.bits() | Self::VOLUME_ID.bits();
    }
}

#[derive(Clone)]
pub struct FATDentry {
    // info about the dentry itself
    fs: &'static Fat32FS,
    pos: GroupDEPos,

    // info about the file represented by the dentry
    attr: Fat32DEntryAttr,
    begin_cluster: ClusterID,
    name: String,
    size: u32,
}

impl FATDentry {
    pub(super) fn lfn_needed(&self) -> usize {
        let name_len = self.name.len();
        if name_len <= 8 {
            0
        } else {
            (name_len + 12) / 13
        }
    }
}

impl ConcreteDEntryRef for FATDentry {
    type FileT = FATFile;

    fn name(&self) -> String {
        self.name.clone()
    }

    fn attr(&self) -> VfsFileAttr {
        let kind = if self.attr.contains(Fat32DEntryAttr::DIRECTORY) {
            VfsFileKind::Directory
        } else {
            VfsFileKind::RegularFile
        };

        let byte_size = self.size as usize;
        let block_count = (byte_size / BLOCK_SIZE) + !(byte_size % BLOCK_SIZE == 0) as usize;

        VfsFileAttr {
            kind,
            device_id: self.fs.device_id(),
            self_device_id: 0,
            byte_size,
            block_count,
            access_time: 0,
            modify_time: 0,
            create_time: 0,
        }
    }

    fn file(&self) -> Self::FileT {
        FATFile {
            fs: self.fs,
            begin_cluster: self.begin_cluster,
            last_cluster: None,
        }
    }
}

// 命名约定:
// 1. AtomDEntry 指 fat32 中的一个 32 byte 长的 DEntry,
//    它可以是一个 LFN Entry, 或是一个 Standard 8.3 DEntry.
//    由于 8.3 叫起来顺口, 注释中一般会以 8.3 指代 Standard 8.3 DEntry.
//    但是 8.3 不是合法标识符, 所以代码中一般会用 std 或是 8p3 指代.
// 2. GroupDEntry 指若干个 LFN + 一个 8.3 组成的一团 DEntry,
//    在逻辑上代表了目录中的一项文件

const DENTRY_SIZE: ClsOffsetT = 32;

#[repr(C, packed)]
pub(super) struct Standard8p3EntryRepr {
    name: [u8; 8],
    ext: [u8; 3],
    attr: u8,
    _reserved: u8,
    ctime_ts: u8,
    ctime: u16,
    cdate: u16,
    adate: u16,
    cluster_high: u16,
    mtime: u16,
    mdate: u16,
    cluster_low: u16,
    size: u32,
}

#[repr(C, packed)]
pub(super) struct LongFileNameEntryRepr {
    order: u8,
    name1: [u16; 5],
    attr: u8,
    _type: u8,
    checksum: u8,
    name2: [u16; 6],
    _reserved: u16,
    name3: [u16; 2],
}

pub(super) struct AtomDEntryView<'a>(&'a [u8]);
impl<'a> AtomDEntryView<'a> {
    pub fn new(raw: &'a [u8]) -> Self {
        debug_assert!(raw.len() == DENTRY_SIZE as usize);
        Self(raw)
    }

    pub fn is_std(&self) -> bool {
        parse!(u8, self.0, 11) != 0x0F
    }
    pub fn is_lfn(&self) -> bool {
        parse!(u8, self.0, 11) == 0x0F
    }
    pub fn is_unused(&self) -> bool {
        parse!(u8, self.0, 0) == 0xE5
    }
    pub fn is_end(&self) -> bool {
        parse!(u8, self.0, 0) == 0x00
    }

    pub fn as_std(&self) -> &'a Standard8p3EntryRepr {
        unsafe { &*(self.0.as_ptr() as *const Standard8p3EntryRepr) }
    }
    pub fn as_lfn(&self) -> &'a LongFileNameEntryRepr {
        unsafe { &*(self.0.as_ptr() as *const LongFileNameEntryRepr) }
    }

    pub unsafe fn as_std_mut(&self) -> &'a mut Standard8p3EntryRepr {
        unsafe { &mut *(self.0.as_ptr() as *mut Standard8p3EntryRepr) }
    }
    pub unsafe fn as_lfn_mut(&self) -> &'a mut LongFileNameEntryRepr {
        unsafe { &mut *(self.0.as_ptr() as *mut LongFileNameEntryRepr) }
    }

    fn debug(&self) -> &Self {
        if self.is_end() {
            log::debug!("Type: End");
        } else if self.is_unused() {
            log::debug!("Type: Unused");
        } else if self.is_std() {
            log::debug!("Type: Standard 8.3");
        } else if self.is_lfn() {
            log::debug!("Type: Long File Name");
        } else {
            log::debug!("Type: Unknown");
        }

        log::debug!("Binary:");
        // print 32 byte, in 2 line
        log::debug!(
            " {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
            self.0[0],
            self.0[1],
            self.0[2],
            self.0[3],
            self.0[4],
            self.0[5],
            self.0[6],
            self.0[7],
            self.0[8],
            self.0[9],
            self.0[10],
            self.0[11],
            self.0[12],
            self.0[13],
            self.0[14],
            self.0[15]
        );
        log::debug!(
            " {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x} {:02x}",
            self.0[16],
            self.0[17],
            self.0[18],
            self.0[19],
            self.0[20],
            self.0[21],
            self.0[22],
            self.0[23],
            self.0[24],
            self.0[25],
            self.0[26],
            self.0[27],
            self.0[28],
            self.0[29],
            self.0[30],
            self.0[31]
        );
        self
    }
}

/// 一个 AtomDE 在目录中的位置
type AtomDEPos = u32;
/// 一个 GroupDE 在目录中的位置
type GroupDEPos = u32;

pub struct GroupDEntryIter {
    fs: &'static Fat32FS,
    window: AtomDEntryWindow,
    is_deleted: bool,
    is_dirty: bool,
}

impl GroupDEntryIter {
    pub fn new(fs: &'static Fat32FS, begin_cluster: ClusterID) -> Self {
        Self {
            fs,
            window: AtomDEntryWindow::new(fs, begin_cluster),
            is_deleted: false,
            is_dirty: false,
        }
    }
}

impl Stream for GroupDEntryIter {
    type Item = SysResult<FATDentry>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };
        macro_rules! p_await {
            ($e:expr) => {
                match core::pin::pin!($e).poll(cx) {
                    Poll::Ready(v) => v,
                    Poll::Pending => return Poll::Pending,
                }
            };
        }

        match p_await!(this.mark_next()) {
            Ok(Some(())) => {}
            Ok(None) => return Poll::Ready(None),
            Err(e) => return Poll::Ready(Some(Err(e))),
        };

        let result = this.collect_dentry();

        if let Err(e) = p_await!(this.leave_next()) {
            return Poll::Ready(Some(Err(e)));
        }

        Poll::Ready(Some(Ok(result)))
    }
}

impl GroupDEntryIter {
    fn atom_iter(&self) -> impl Iterator<Item = AtomDEntryView> {
        self.window.iter()
    }
    fn get_std_entry(&self) -> AtomDEntryView {
        log::trace!("get_std_entry");
        self.window.last()
    }
    fn is_std_only(&self) -> bool {
        // 只有一个 AtomDEntry
        self.window.len() == 1
    }

    pub(super) fn collect_name(&self) -> String {
        if self.is_std_only() {
            let std = self.get_std_entry().as_std();
            let mut name: Vec<_> = std.name.into_iter().filter(|&c| c != 0x20).collect();
            let ext: Vec<_> = std.ext.into_iter().filter(|&c| c != 0x20).collect();
            if !ext.is_empty() {
                name.push(b'.');
                name.extend(ext);
            }
            String::from_utf8(name).unwrap()
        } else {
            let mut name = Vec::<u16>::new();
            for atom_entry in self.atom_iter() {
                if atom_entry.is_std() {
                    break;
                }
                let lfn = atom_entry.as_lfn();
                name.extend(lfn.name1);
                name.extend(lfn.name2);
                name.extend(lfn.name3);
            }
            name.retain(|&x| x != 0xFFFF);
            String::from_utf16(&name).unwrap()
        }
    }
    pub(super) fn get_attr(&self) -> Fat32DEntryAttr {
        Fat32DEntryAttr::from_bits(self.get_std_entry().as_std().attr).unwrap()
    }
    pub(super) fn get_begin_cluster(&self) -> ClusterID {
        let std = self.get_std_entry().as_std();
        ((std.cluster_high as ClusterID) << 16) | (std.cluster_low as ClusterID)
    }
    pub(super) fn get_size(&self) -> u32 {
        self.get_std_entry().as_std().size
    }
    pub(super) fn collect_dentry(&self) -> FATDentry {
        FATDentry {
            fs: self.fs,
            pos: self.window.left_pos(),
            name: self.collect_name(),
            attr: self.get_attr(),
            begin_cluster: self.get_begin_cluster(),
            size: self.get_size(),
        }
    }

    pub(super) fn change_size(&mut self, new_size: u32) {
        self.is_dirty = true;
        unsafe { self.get_std_entry().as_std_mut().size = new_size }
    }
    pub(super) fn delete_entry(&mut self) {
        // 要将所有的 ADE 的第一个字节设置为 0xE5, 并且标记全部 dirty
        self.is_deleted = true;
        self.is_dirty = true;
        for atom_entry in self.atom_iter() {
            unsafe { atom_entry.as_std_mut().name[0] = 0xE5 };
        }
    }
    pub(super) fn can_create(&self, dentry: &FATDentry) -> bool {
        (self.window.get_in_buf(0).is_unused() || self.is_deleted)
            && dentry.lfn_needed() + 1 <= self.window.len()
    }

    pub(super) async fn create_entry(&mut self, dentry: &FATDentry) -> SysResult<()> {
        // 从当前窗口切一块出来存放新的 GDE
        debug_assert!(self.can_create(dentry));
        self.is_dirty = true;

        // 写入名字
        let name = dentry.name.as_str();
        let lfn_needed = dentry.lfn_needed();
        for i in 0..lfn_needed {
            let mut lfn_buf: [u16; 13] = [0; 13];
            let end = min(name.len(), (i + 1) * 13);
            for (j, c) in name[i * 13..end].chars().enumerate() {
                lfn_buf[j] = c as u16;
            }

            let lfn = unsafe { self.window.get_in_buf(i).as_lfn_mut() };
            lfn.order = i as u8 + 1;
            // non-aligned u16 here, don't know how to do better
            for j in 0..5 {
                lfn.name1[j] = lfn_buf[j];
            }
            lfn.attr = 0x0F;
            lfn._type = 0;
            lfn.checksum = 0;
            for j in 0..6 {
                lfn.name2[j] = lfn_buf[j + 5];
            }
            lfn._reserved = 0;
            for j in 0..2 {
                lfn.name3[j] = lfn_buf[j + 11];
            }
        }

        // 写入 8.3
        let std = unsafe { self.window.get_in_buf(lfn_needed).as_std_mut() };
        if lfn_needed == 0 {
            for (j, c) in name.chars().enumerate() {
                std.name[j] = c as u8;
            }
        } else {
            std.name.fill(b' ');
        }
        std.ext.fill(b' ');
        std.attr = dentry.attr.bits();
        std._reserved = 0;
        std.ctime_ts = 0;
        std.ctime = 0;
        std.cdate = 0;
        std.adate = 0;
        std.cluster_high = (dentry.begin_cluster >> 16) as u16;
        std.mtime = 0;
        std.mdate = 0;
        std.cluster_low = (dentry.begin_cluster & 0xFFFF) as u16;
        std.size = dentry.size;

        // 更新窗口
        self.window.move_left(lfn_needed + 1, true).await
    }

    /// 从当前位置开始，向后查找直到找到一个完整的 GroupDE.
    /// 随后停止在这个 GDE, 可以通过诸如 `Self::get_xxx` 或 `Self::delete_dentry`
    /// 之类的方法访问或修改当前 GDE 的信息
    pub(super) async fn mark_next(&mut self) -> SysResult<Option<()>> {
        loop {
            let next = match self.window.move_right_one().await? {
                Some(next) => next,
                None => return Ok(None),
            };

            if next.is_std() {
                return Ok(Some(()));
            }
        }
    }
    /// 通知已完成了对当前 GroupDE 的需求, 可以准备开始寻找下一个 GDE 了
    pub(super) async fn leave_next(&mut self) -> SysResult<()> {
        // TODO: 识别并记录对当前 GDE 的修改, 有时候只针对 8.3 的修改可以不用写回前边的一堆 LFN
        self.window.move_left(self.window.len(), self.is_dirty).await
    }
}

struct SectorBuf {
    pub(super) data: Box<[u8]>,
    pub(super) id: SectorID,
    pub(super) dirty: bool,
}

impl SectorBuf {
    pub fn alloc() -> SectorBuf {
        SectorBuf {
            data: unsafe { Box::new_uninit_slice(BLOCK_SIZE).assume_init() },
            id: 0,
            dirty: false,
        }
    }
    pub fn reassign(&mut self, id: SectorID) {
        self.id = id;
        self.dirty = false;
    }
}

struct AtomDEntryWindow {
    fs: &'static Fat32FS,
    ///  |----|++++++++++++++++++|----|
    /// beg   left(r)     right(r)   end
    sector_bufs: VecDeque<SectorBuf>,
    in_init: bool,
    last_sector: SectorID,
    buf_in_use: u32,
    /// sector_bufs[0] 中的第一个 ADE 的绝对编号
    begin_pos: AtomDEPos,
    left_pos: AtomDEPos,
    right_pos: AtomDEPos,
}

const BLK_ADE_CNT: usize = BLOCK_SIZE / DENTRY_SIZE as usize;

impl AtomDEntryWindow {
    const INIT_BUF_CNT: usize = 2;
    pub fn new(fs: &'static Fat32FS, begin_cluster: ClusterID) -> AtomDEntryWindow {
        let mut sector_bufs = VecDeque::with_capacity(4);
        for _ in 0..Self::INIT_BUF_CNT {
            sector_bufs.push_back(SectorBuf::alloc());
        }

        AtomDEntryWindow {
            fs,
            sector_bufs,
            in_init: true,
            last_sector: fs.first_sector(begin_cluster),
            begin_pos: 0,
            left_pos: 0,
            right_pos: 0,
            buf_in_use: 0,
        }
    }
    async fn load(&mut self, buf_idx: usize) -> SysResult<()> {
        let sc = &mut self.sector_bufs[buf_idx];
        self.fs.read_sector(sc.id, &mut sc.data).await
    }
    async fn sync(&mut self, buf_idx: usize) -> SysResult<()> {
        let sc = &mut self.sector_bufs[buf_idx];
        if sc.dirty {
            sc.dirty = false;
            self.fs.write_sector(sc.id, &sc.data).await
        } else {
            Ok(())
        }
    }

    /// move the left bound of current windows by N ADEs,
    /// if write_back is true, the passed by buffers and
    /// the buffer where the new left in will be marked as dirty.
    pub async fn move_left(&mut self, n: usize, write_back: bool) -> SysResult<()> {
        let n = n as AtomDEPos;
        if self.left_pos + n > self.right_pos {
            panic!("move_left: out of range");
        }

        // check how many buffers has been passed by
        let new_left = self.left_pos + n;

        // sync the passed by buffers and re-use them
        let buf_idx_where_new_left_in = new_left / DENTRY_SIZE as AtomDEPos;
        for i in 0..buf_idx_where_new_left_in {
            let mut sc = self.sector_bufs.pop_front().unwrap();
            sc.dirty |= write_back;
            self.sync(i as usize).await?;
            self.sector_bufs.push_back(sc);
        }

        // mark current buffer with respect to write_back
        self.sector_bufs[buf_idx_where_new_left_in as usize].dirty |= write_back;

        log::trace!("move_left: {} -> {}", self.left_pos, new_left);
        self.left_pos = new_left;
        self.begin_pos += buf_idx_where_new_left_in * BLK_ADE_CNT as AtomDEPos;
        Ok(())
    }

    pub async fn move_right_one(&mut self) -> SysResult<Option<AtomDEntryView>> {
        let current = self.right_pos;
        log::trace!(
            "move_right_one enter: (right_pos: {}, in_init: {}, buf_in_use: {})",
            self.right_pos,
            self.in_init,
            self.buf_in_use
        );

        // 1. check if the next ADE pass the capacity
        // if so, alloc a new buffer
        let cur_buf_idx = (current as usize) / BLK_ADE_CNT;
        if cur_buf_idx >= self.sector_bufs.len() {
            self.sector_bufs.push_back(SectorBuf::alloc());
        }

        // 2. check if the next ADE pass last buffer
        // if so, load the new buffer & update the buf_in_use
        //      2.1 check for init
        if self.in_init {
            self.sector_bufs[0].reassign(self.last_sector as SectorID);
            self.load(0).await?;
            self.buf_in_use += 1;
            self.in_init = false;
        } else {
            let end_idx = self.buf_in_use as usize;
            if cur_buf_idx >= end_idx {
                debug_assert!(cur_buf_idx == end_idx);
                // find next sector of the last buffer
                let next_sector = match self.fs.next_sector(self.last_sector) {
                    Some(s) => s,
                    None => return Ok(None),
                };

                // re-assign the new buffer and load content
                let new_buf = &mut self.sector_bufs[end_idx];
                new_buf.reassign(next_sector);
                self.load(cur_buf_idx).await?;
                self.buf_in_use += 1;
            }
        }

        // 3. update the right_pos
        self.right_pos += 1;

        // 4. check whether the new ADE is valid.
        let last_ade = self.get_in_dir(current);
        if last_ade.is_end() {
            log::debug!("move_right_one: reach end ({})", current);
            return Ok(None);
        } else {
            log::debug!(
                "move_right_one: reach {} ({})",
                (if last_ade.is_lfn() {
                    "lfn"
                } else if last_ade.is_std() {
                    "std"
                } else {
                    "unused"
                }),
                current
            );
            Ok(Some(last_ade))
        }
    }

    /// get the ADE with its relative AtomDEPos in this window
    pub fn get_in_window(&self, idx: usize) -> AtomDEntryView {
        self.get_in_dir(self.left_pos + idx as AtomDEPos)
    }

    /// get the ADE with its AtomDEPos in current directory
    pub fn get_in_dir(&self, pos: AtomDEPos) -> AtomDEntryView {
        self.get_in_buf((pos - self.begin_pos) as usize)
    }

    /// get the ADE with its index in curr buffer
    fn get_in_buf(&self, buf_ade_idx: usize) -> AtomDEntryView {
        let buf_idx = buf_ade_idx / BLK_ADE_CNT;
        let buf_off = buf_ade_idx % BLK_ADE_CNT;

        let buf = &self.sector_bufs[buf_idx];
        let buf_off_byte = buf_off * DENTRY_SIZE as usize;
        let slice = &buf.data[buf_off_byte..(buf_off_byte + DENTRY_SIZE as usize)];
        let entry = AtomDEntryView::new(slice);

        // log::debug!("get_idx: {}", buf_ade_idx);
        // entry.debug();

        entry
    }

    /// how many ADE this windows holding now
    pub fn len(&self) -> usize {
        (self.right_pos - self.left_pos) as usize
    }
    /// left bound of this windows, in current directory
    pub fn left_pos(&self) -> AtomDEPos {
        self.left_pos
    }
    /// right bound of this windows, in current directory
    pub fn right_pos(&self) -> AtomDEPos {
        self.right_pos
    }

    pub fn last(&self) -> AtomDEntryView {
        self.get_in_dir(self.right_pos - 1)
    }
    pub fn iter(&self) -> AtomDEntryWindowIter {
        AtomDEntryWindowIter {
            this: self,
            cur: self.left_pos,
        }
    }
}

pub(super) struct AtomDEntryWindowIter<'a> {
    this: &'a AtomDEntryWindow,
    cur: AtomDEPos,
}

impl<'a> Iterator for AtomDEntryWindowIter<'a> {
    type Item = AtomDEntryView<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        log::trace!("AtomDEntryWindowIter::cur: {}", self.cur);
        if self.cur as usize >= self.this.len() {
            None
        } else {
            let res = self.this.get_in_dir(self.cur);
            self.cur += 1;
            Some(res)
        }
    }
}
