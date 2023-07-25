use super::tools::BlockCacheEntryRef;
use super::{ClsOffsetT, ClusterID, Fat32FS, SectorID};

use crate::{
    fs::{disk::BLOCK_SIZE, nfat32::parse},
    tools::errors::SysResult,
};
use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::fmt::Display;
use core::{cmp::min, usize};

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
pub struct FatDEntryData<'a> {
    // info about the file represented by the dentry
    pub(super) attr: Fat32DEntryAttr,
    pub(super) begin_cluster: ClusterID,
    pub(super) name: &'a str,
    pub(super) size: u32,
}

impl FatDEntryData<'_> {
    pub(super) fn lfn_needed(&self) -> usize {
        let name_len = self.name.len();
        if name_len <= 8 {
            0
        } else {
            (name_len + 12) / 13
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
#[derive(Clone)]
pub(super) struct Standard8p3EntryRepr {
    pub name: [u8; 8],
    pub ext: [u8; 3],
    pub attr: u8,
    pub _reserved: u8,
    pub ctime_ts: u8,
    pub ctime: u16,
    pub cdate: u16,
    pub adate: u16,
    pub cluster_high: u16,
    pub mtime: u16,
    pub mdate: u16,
    pub cluster_low: u16,
    pub size: u32,
}

impl Standard8p3EntryRepr {
    pub fn attr(&self) -> Fat32DEntryAttr {
        Fat32DEntryAttr::from_bits(self.attr).unwrap()
    }
}

#[repr(C, packed)]
#[derive(Clone)]
pub(super) struct LongFileNameEntryRepr {
    pub order: u8,
    pub name1: [u16; 5],
    pub attr: u8,
    pub _type: u8,
    pub checksum: u8,
    pub name2: [u16; 6],
    pub _reserved: u16,
    pub name3: [u16; 2],
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

    pub unsafe fn mark_end(&self) {
        unsafe { self.as_std_mut().name[0] = 0x00 }
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AtomDEPos(usize);
/// 一个 GroupDE 在目录中的位置
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupDEPos(usize);

impl AtomDEPos {
    /// 这个 AtomDE 的开始在目录中的字节偏移量
    pub const fn new(pos: usize) -> Self {
        Self(pos)
    }
    /// 不在任何一个目录中的 AtomDE
    pub const fn null() -> Self {
        Self(usize::MAX)
    }
    pub fn is_null(&self) -> bool {
        self.0 == usize::MAX
    }

    #[inline(always)]
    fn assume_non_null(&self) {
        debug_assert!(!self.is_null())
    }

    pub fn as_gdp(&self) -> GroupDEPos {
        self.assume_non_null();
        GroupDEPos(self.0)
    }

    pub fn offset_ade(&self, n: isize) -> AtomDEPos {
        self.offset_byte(n * DENTRY_SIZE as isize)
    }
    pub fn offset_byte(&self, byte: isize) -> AtomDEPos {
        self.assume_non_null();
        debug_assert!(byte % DENTRY_SIZE as isize == 0);
        debug_assert!(self.0 as isize + byte >= 0);
        AtomDEPos((self.0 as isize + byte) as usize)
    }

    pub fn as_byte_offset(&self) -> usize {
        self.assume_non_null();
        self.0
    }
    pub fn as_ade_offset(&self) -> usize {
        self.as_byte_offset() / DENTRY_SIZE as usize
    }
}
impl GroupDEPos {
    /// 这个 GroupDE 的开始在目录中的字节偏移量
    pub const fn new(pos: usize) -> Self {
        Self(pos)
    }
    /// 不在任何一个目录中的 GroupDE
    pub const fn null() -> Self {
        Self(usize::MAX)
    }
    pub fn is_null(&self) -> bool {
        self.0 == usize::MAX
    }

    #[inline(always)]
    fn assume_non_null(&self) {
        debug_assert!(!self.is_null())
    }

    pub fn begin_ade(&self) -> AtomDEPos {
        self.assume_non_null();
        AtomDEPos(self.0)
    }
    pub fn round_down_sct(&self) -> AtomDEPos {
        self.assume_non_null();
        AtomDEPos((self.0 / BLK_ADE_CNT) * BLK_ADE_CNT)
    }

    pub fn offset_byte(&self, byte: usize) -> GroupDEPos {
        self.assume_non_null();
        debug_assert!(byte % DENTRY_SIZE as usize == 0);
        GroupDEPos(self.0 + byte)
    }

    pub fn as_byte_offset(&self) -> usize {
        self.assume_non_null();
        self.0
    }
}
impl Display for GroupDEPos {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}
impl Display for AtomDEPos {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self)
    }
}

pub struct GroupDEntryIter {
    fs: &'static Fat32FS,
    window: AtomDEntryWindow,
    is_deleted: bool,
}

impl GroupDEntryIter {
    pub fn new(fs: &'static Fat32FS, begin_cluster: ClusterID) -> Self {
        Self {
            fs,
            window: AtomDEntryWindow::new(fs, begin_cluster),
            is_deleted: false,
        }
    }

    pub fn new_middle(fs: &'static Fat32FS, sector: SectorID, gde_pos: GroupDEPos) -> Self {
        Self {
            fs,
            window: AtomDEntryWindow::new_middle(fs, sector, gde_pos),
            is_deleted: false,
        }
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

    pub(super) fn gde_pos(&self) -> GroupDEPos {
        self.window.left_pos().as_gdp()
    }
    pub(super) fn std_pos(&self) -> AtomDEPos {
        self.window.right_pos().offset_ade(-1)
    }
    pub(super) fn std_clone(&self) -> Standard8p3EntryRepr {
        self.get_std_entry().as_std().clone()
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

    pub(super) fn change_size(&mut self, new_size: u32) {
        unsafe { self.get_std_entry().as_std_mut().size = new_size }
    }
    pub(super) fn delete_entry(&mut self) {
        // 要将所有的 ADE 的第一个字节设置为 0xE5, 并且标记全部 dirty
        self.is_deleted = true;
        for atom_entry in self.atom_iter() {
            unsafe { atom_entry.as_std_mut().name[0] = 0xE5 };
        }
    }
    /// 是否代表一个有效的 GDE.
    /// 只有在它返回 true 时, collect_* 和 get_* 方法才能使用
    pub(super) fn is_valid(&self) -> bool {
        !self.can_create_any()
    }
    /// 是否代表一个能放东西的空缺
    pub(super) fn can_create_any(&self) -> bool {
        self.window.get_in_buf(0).is_unused() || self.is_deleted
    }
    pub(super) fn can_create(&self, dentry: &FatDEntryData) -> bool {
        self.can_create_any() && dentry.lfn_needed() + 1 <= self.window.len()
    }

    pub(super) async fn create_entry<'a>(
        &'a mut self,
        dentry: &FatDEntryData<'a>,
    ) -> SysResult<()> {
        // 从当前窗口切一块出来存放新的 GDE
        debug_assert!(self.can_create(dentry));

        // 写入名字
        let name = dentry.name;
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
        self.window.move_left(lfn_needed + 1).await
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
        self.window.move_left(self.window.len()).await
    }

    pub(super) fn append_enter(&mut self) {
        self.window.in_append = true;
    }
    pub(super) async fn append<'a>(&'a mut self, dentry: &FatDEntryData<'a>) -> SysResult<()> {
        let ade_needed = dentry.lfn_needed() + 1;
        for _ in 0..ade_needed {
            self.window.move_right_one().await?;
        }
        self.create_entry(dentry).await
    }
    pub(super) async fn append_end(&mut self) -> SysResult<()> {
        // write an empty GDE
        self.window.move_right_one().await?;
        unsafe { self.window.last().mark_end() };

        // sync
        self.sync_all().await?;
        self.window.in_append = false;
        Ok(())
    }

    pub(super) async fn sync_all(&mut self) -> SysResult<()> {
        self.window.move_left(self.window.len()).await
    }
}

struct SectorBuf {
    data: Option<BlockCacheEntryRef>,
    pub(super) id: SectorID,
}

impl SectorBuf {
    pub fn new(id: SectorID) -> Self {
        SectorBuf { data: None, id }
    }
}

struct AtomDEntryWindow {
    fs: &'static Fat32FS,
    ///  |----|++++++++++++++++++|----|
    /// beg   left(r)     right(r)   end
    sector_bufs: VecDeque<SectorBuf>,
    in_append: bool,
    next_sector: Option<SectorID>,
    /// `sector_bufs[0]` 中的第一个 ADE 的绝对编号
    begin_pos: AtomDEPos,
    left_pos: AtomDEPos,
    right_pos: AtomDEPos,
}

const BLK_ADE_CNT: usize = BLOCK_SIZE / DENTRY_SIZE as usize;

impl<'a> AtomDEntryWindow {
    const INIT_BUF_CNT: usize = 2;
    pub fn new(fs: &'static Fat32FS, begin_cluster: ClusterID) -> Self {
        AtomDEntryWindow {
            fs,
            sector_bufs: VecDeque::with_capacity(4),
            in_append: false,
            next_sector: Some(fs.first_sector(begin_cluster)),
            begin_pos: AtomDEPos::new(0),
            left_pos: AtomDEPos::new(0),
            right_pos: AtomDEPos::new(0),
        }
    }
    pub fn new_middle(fs: &'static Fat32FS, sector: SectorID, gde_pos: GroupDEPos) -> Self {
        AtomDEntryWindow {
            fs,
            sector_bufs: VecDeque::with_capacity(3),
            in_append: false,
            next_sector: Some(sector),
            begin_pos: gde_pos.round_down_sct(),
            left_pos: gde_pos.begin_ade(),
            right_pos: gde_pos.begin_ade(),
        }
    }
    async fn load(&mut self, sc: &mut SectorBuf) -> SysResult<()> {
        if sc.data.is_none() {
            let block_cache = self.fs.block_dev().get(sc.id).await?;
            sc.data = Some(block_cache);
        }
        Ok(())
    }
    fn buf_idx(&self, pos: AtomDEPos) -> usize {
        (pos.as_byte_offset() - self.begin_pos.as_byte_offset()) / BLOCK_SIZE
    }

    /// move the left bound of current windows by N ADEs,
    /// if write_back is true, the passed by buffers and
    /// the buffer where the new left in will be marked as dirty.
    pub async fn move_left(&mut self, n: usize) -> SysResult<()> {
        let new_left = self.left_pos.offset_ade(n as isize);
        if new_left > self.right_pos {
            panic!("move_left: out of range");
        }

        // check how many sectors has been passed by
        // sync the passed by buffers and re-use them
        let buf_idx_where_new_left_in = self.buf_idx(new_left);
        for _i in 0..buf_idx_where_new_left_in {
            self.sector_bufs.pop_front();
        }

        log::trace!("move_left: {} -> {}", self.left_pos, new_left);
        self.left_pos = new_left;
        self.begin_pos =
            self.begin_pos.offset_byte((buf_idx_where_new_left_in * BLOCK_SIZE) as isize);
        Ok(())
    }

    pub async fn move_right_one(&mut self) -> SysResult<Option<AtomDEntryView>> {
        let current = self.right_pos;
        log::trace!(
            "move_right_one enter: (right_pos: {}, buf_in_use: {})",
            self.right_pos,
            self.sector_bufs.len()
        );

        // 1. 检查是否需要加载新的 sector
        let cur_buf_idx = self.buf_idx(self.right_pos);
        if cur_buf_idx >= self.sector_bufs.len() {
            debug_assert!(cur_buf_idx == self.sector_bufs.len());
            // 要载入新的 sector
            if let Some(next_sct) = self.next_sector {
                let mut new_buf = SectorBuf::new(next_sct);
                self.load(&mut new_buf).await?;
                self.sector_bufs.push_back(new_buf);
                // 更新 next_sector
                self.next_sector = self.fs.next_sector(next_sct);
            } else {
                // 不存在新的 sector 了
                return Ok(None);
            }
        }
        debug_assert!(cur_buf_idx < self.sector_bufs.len());

        // 2. update the right_pos
        self.right_pos.offset_ade(1);

        // 3. check whether the new ADE is valid.
        let last_ade = self.get_in_dir(current);
        if last_ade.is_end() && !self.in_append {
            log::debug!("move_right_one: reach end ({})", current);
            return Ok(None);
        } else {
            let ade_kind = if last_ade.is_lfn() {
                "lfn"
            } else if last_ade.is_std() {
                "std"
            } else {
                "unused"
            };
            log::debug!("move_right_one: reach {} ({})", ade_kind, current);
            Ok(Some(last_ade))
        }
    }

    /// get the ADE with its relative AtomDEPos in this window
    pub fn get_in_window(&self, idx: usize) -> AtomDEntryView {
        self.get_in_dir(self.left_pos.offset_ade(idx as isize))
    }

    /// get the ADE with its AtomDEPos in current directory
    pub fn get_in_dir(&self, pos: AtomDEPos) -> AtomDEntryView {
        let delta_ade = pos.as_ade_offset() - self.begin_pos.as_ade_offset();
        self.get_in_buf(delta_ade)
    }

    /// get the ADE with its index in curr buffer
    fn get_in_buf(&self, buf_ade_idx: usize) -> AtomDEntryView {
        let buf_idx = buf_ade_idx / BLK_ADE_CNT;
        let buf_off = buf_ade_idx % BLK_ADE_CNT;

        let buf = &self.sector_bufs[buf_idx];
        let buf_off_byte = buf_off * DENTRY_SIZE as usize;
        let bc_ref = buf.data.as_ref().unwrap();
        // TODO: 想办法在这里合并一些锁的使用, 比如说在准备大量解析同一个块时只给它上一个锁
        let block_slice = bc_ref.as_slice();
        let entry_slice = &block_slice[buf_off_byte..(buf_off_byte + DENTRY_SIZE as usize)];
        let entry = AtomDEntryView::new(entry_slice);

        // log::debug!("get_idx: {}", buf_ade_idx);
        // entry.debug();
        entry
    }

    /// how many ADE this windows holding now
    pub fn len(&self) -> usize {
        let delta_byte = self.right_pos.as_byte_offset() - self.left_pos.as_byte_offset();
        (delta_byte as usize) / (DENTRY_SIZE as usize)
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
        self.get_in_dir(self.right_pos.offset_ade(-1))
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
        if self.cur.as_ade_offset() >= self.this.len() {
            None
        } else {
            let res = self.this.get_in_dir(self.cur);
            self.cur.offset_ade(1);
            Some(res)
        }
    }
}
