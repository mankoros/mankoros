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
    async_iter::AsyncIterator, cell::SyncUnsafeCell, future::Future, pin::pin, slice, task::Poll,
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
    cluster_id: ClusterID,
    cluster_offset: ClsOffsetT,

    // info about the file represented by the dentry
    attr: Fat32DEntryAttr,
    begin_cluster: ClusterID,
    name: String,
    size: u32,
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

pub struct DEntryIter {
    fs: &'static Fat32FS,
    buf: Box<[u8]>,
    cur_cluster: ClusterID,
    cur_offset: ClsOffsetT,
    is_end: bool,
    is_uninit: bool,
}

impl DEntryIter {
    pub fn new(fs: &'static Fat32FS, begin_cluster: ClusterID) -> Self {
        let buf = unsafe { Box::new_uninit_slice(fs.cluster_size_byte as usize).assume_init() };
        Self {
            fs,
            buf,
            cur_cluster: begin_cluster,
            cur_offset: 0,
            is_end: false,
            is_uninit: true,
        }
    }

    fn at_cluster_end(&self) -> bool {
        self.cur_offset == self.buf.len() as u16
    }
}

impl Stream for DEntryIter {
    type Item = SysResult<FATDentry>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };

        // check if it has been ended
        if this.is_end {
            return Poll::Ready(None);
        }

        // record begin cluster and offset for DEntryRef
        let (begin_cls, begin_offset) = if this.at_cluster_end() {
            (this.cur_cluster + 1, 0)
        } else {
            (this.cur_cluster, this.cur_offset)
        };

        let mut lfn_names = Vec::<(u8, [u16; 13])>::new();
        loop {
            log::trace!(
                "DEntryIter: (cluster, cur_offset): {}, {}",
                this.cur_cluster,
                this.cur_offset
            );

            // check if need to move to next cluster
            if this.at_cluster_end() {
                let next_cls =
                    this.fs.with_fat(|fat_table_mgr| fat_table_mgr.next(this.cur_cluster));
                // if not next, end
                let next_cls = match next_cls {
                    Some(next_cls) => next_cls,
                    None => {
                        this.is_end = true;
                        return Poll::Ready(None);
                    }
                };
                this.is_uninit = true;
                // update cur_cluster and cur_offset
                this.cur_cluster = next_cls;
                this.cur_offset = 0;
            }

            // check if need to read the current cluster to buf
            if this.is_uninit {
                // if read pending, return pending
                // if read error, return error
                match pin!(this.fs.read_cluster(this.cur_cluster, &mut this.buf)).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                    _ => {}
                }

                this.is_uninit = false;
            }

            // read a dentry
            let dentry_raw = {
                let offset = this.cur_offset as usize;
                &this.buf[offset..(offset + 32)]
            };
            this.cur_offset += 32;

            // read the first byte to determine whether it is a valid dentry
            let first_byte = parse!(u8, dentry_raw, 0);
            if first_byte == 0 {
                // an empty entry, indicating the end of dentries
                this.is_end = true;
                return Poll::Ready(None);
            } else if first_byte == 0xE5 {
                // deleted entry, indicating an unused slot
                continue;
            }

            // read the attribute byte to determine whether it is a LFN entry
            let attr = parse!(u8, dentry_raw, 11);
            if attr == 0x0F {
                // LFN
                let ord = parse!(u8, dentry_raw, 0);
                lfn_names.push((ord, [0; 13]));
                let lfn_name = &mut lfn_names.last_mut().unwrap().1;
                // copy the first 5 * 2char, then 6 * 2char, finanlly 2 * 2char
                lfn_name[0..5].copy_from_slice(unsafe {
                    slice::from_raw_parts(dentry_raw[1..11].as_ptr() as *const u16, 5)
                });
                lfn_name[5..11].copy_from_slice(unsafe {
                    slice::from_raw_parts(dentry_raw[14..26].as_ptr() as *const u16, 6)
                });
                lfn_name[11..13].copy_from_slice(unsafe {
                    slice::from_raw_parts(dentry_raw[28..32].as_ptr() as *const u16, 2)
                });
            } else {
                // standard 8.3, or 8.3 with LFN
                // collect all the information to build a result dentry

                // if not LFN, the name is in 8.3
                let name = if lfn_names.len() == 0 {
                    // 8.3 name use space (0x20) for "no char"
                    let mut name_chars = Vec::<u8>::with_capacity(8 + 1 + 3);
                    for i in 0..8 {
                        let c = parse!(u8, dentry_raw, i);
                        if c == 0x20 {
                            break;
                        }
                        name_chars.push(c);
                    }
                    // if no extension, no dot
                    if parse!(u8, dentry_raw, 8) != b' ' {
                        name_chars.push(b'.');
                        for i in 8..11 {
                            let c = parse!(u8, dentry_raw, i);
                            if c == 0x20 {
                                break;
                            }
                            name_chars.push(c);
                        }
                    }
                    // use from_utf8 to parse ASCII name
                    String::from_utf8(name_chars).unwrap()
                } else {
                    // sort the LFN entries we have found
                    // the LFS char is 2-byte, usually UTF-16
                    let mut name_chars = Vec::<u16>::with_capacity(13 * lfn_names.len());
                    lfn_names.sort_by_cached_key(|(ord, _)| *ord);
                    'outer: for (_, lfn_name) in lfn_names.iter() {
                        // then collect all the valid chars (not '\0\0')
                        for c in lfn_name.iter() {
                            if *c == 0 {
                                break 'outer;
                            }
                            name_chars.push(*c);
                        }
                    }
                    String::from_utf16(&name_chars).unwrap()
                };

                // then collect other information

                let attr = Fat32DEntryAttr::from_bits(attr).unwrap();

                let cluster_high = parse!(u16, dentry_raw, 20);
                let cluster_low = parse!(u16, dentry_raw, 26);
                let begin_cluster = ((cluster_high as u32) << 16) | (cluster_low as u32);

                let size = parse!(u32, dentry_raw, 28);

                let dentry = FATDentry {
                    fs: this.fs,
                    cluster_id: begin_cls,
                    cluster_offset: begin_offset,

                    name,
                    attr,
                    begin_cluster,
                    size,
                };

                return Poll::Ready(Some(Ok(dentry)));
            }
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
        Self(raw)
    }

    pub fn is_std(&self) -> bool {
        parse!(u8, self.0, 11) != 0x0F
    }
    pub fn is_lfn(&self) -> bool {
        parse!(u8, self.0, 11) == 0x0F
    }
    pub fn is_empty(&self) -> bool {
        parse!(u8, self.0, 0) == 0
    }

    pub fn as_std(&self) -> &'a Standard8p3EntryRepr {
        unsafe { &*(self.0.as_ptr() as *const Standard8p3EntryRepr) }
    }
    pub fn as_lfn(&self) -> &'a LongFileNameEntryRepr {
        unsafe { &*(self.0.as_ptr() as *const LongFileNameEntryRepr) }
    }

    pub unsafe fn as_std_mut(&mut self) -> &'a mut Standard8p3EntryRepr {
        unsafe { &mut *(self.0.as_ptr() as *mut Standard8p3EntryRepr) }
    }
    pub unsafe fn as_lfn_mut(&mut self) -> &'a mut LongFileNameEntryRepr {
        unsafe { &mut *(self.0.as_ptr() as *mut LongFileNameEntryRepr) }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, PartialOrd, Ord)]
pub struct VolumePos {
    pub(super) cluster: ClusterID,
    pub(super) offset: ClsOffsetT,
}

impl core::fmt::Debug for VolumePos {
    fn fmt(&self, f: &mut alloc::fmt::Formatter<'_>) -> alloc::fmt::Result {
        write!(f, "VolPos({:?}, {:?})", self.cluster, self.offset)
    }
}

impl VolumePos {
    pub const fn zero() -> Self {
        Self {
            cluster: 0,
            offset: 0,
        }
    }

    pub fn try_to_next(&mut self, cls_size: ClsOffsetT) -> bool {
        if self.offset >= cls_size {
            self.offset -= cls_size;
            self.cluster += 1;
            true
        } else {
            false
        }
    }

    pub fn add(&mut self, offset: ClsOffsetT) {
        self.offset += offset;
    }
}

/// 一个 AtomDE 在目录中的位置
type AtomDEPos = u32;

pub struct GroupDEntryIter {
    fs: &'static Fat32FS,
    // TODO: 看看改为以 sector 而不是 cluster 为单位是否会更好
    bufs: Vec<Box<[u8]>>,
    last: VolumePos,
    curr: VolumePos,
    is_end: bool,
    /// 延迟清理标志
    /// 在删除了一个 GroupDEntry 之后, 往往会紧接着对它进行写入.
    /// 所以我们不在删除后立即清零, 而是等待所有的写入都完成之后,
    /// 再清空剩余的 AtomDEntries.
    delay_clear: bool,
    /// 所有的 cluster 都 dirty 了
    /// 这是一般删除文件或者创建新文件的情况
    dirty: bool,
    /// 仅仅只是最后一个 cluster dirty 了,
    /// 一般出现于甚长文件名但是只修改了 8.3 部分 (比如 size 的情况)
    last_cls_dirty: bool,
    // 反正要放个 bool, 后边的位置空着也是空着,
    // 不如拿来缓存一下 cluster 大小
    cls_size_byte: ClsOffsetT,
}

impl GroupDEntryIter {
    // 这里放用于实现 AsyncIterator 的辅助函数
}

impl AsyncIterator for GroupDEntryIter {
    type Item = SysResult<VolumePos>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };
        let cls_size = this.cls_size_byte;
        macro_rules! read_new_cls {
            ($cid:expr, $buf:expr) => {
                match pin!(this.fs.read_cluster($cid, $buf)).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                    _ => {}
                }
            };
        }

        if this.is_end {
            return Poll::Ready(None);
        }

        // the first time we call this iter
        if this.curr == this.last {
            // alloc the first buf and keep it (never free it)
            debug_assert!(this.bufs.is_empty());
            let mut first_buf =
                unsafe { Box::new_uninit_slice(this.cls_size_byte as usize).assume_init() };
            read_new_cls!(this.curr.cluster, &mut first_buf);
            this.bufs.push(first_buf);
        }

        loop {
            log::trace!("GroupDEntryIter::poll_next: curr = {:?}", this.curr);

            if this.curr.try_to_next(cls_size) {
                // we have read all the entries in this cluster
                // so we should read the next cluster
                let next_cls =
                    this.fs.with_fat(|fat_table_mgr| fat_table_mgr.next(this.curr.cluster));
                let next_cls = match next_cls {
                    Some(next_cls) => next_cls,
                    None => {
                        this.is_end = true;
                        return Poll::Ready(None);
                    }
                };
                // alloc a new buf
                let mut new_buf =
                    unsafe { Box::new_uninit_slice(this.cls_size_byte as usize).assume_init() };
                read_new_cls!(next_cls, &mut new_buf);
            }
        }

        todo!()
    }
}

type RelativeVolumePos = VolumePos;
struct AtomDEntryIterInGroupIter<'a> {
    this: &'a GroupDEntryIter,
    // TODO: 直接保存 &Box<[u8]> 避免每次通过 vec 访问
    /// 为了避免每次都要减去 this.beg, 这里的 cluster 保存相对位置
    /// 即 0 代表 this.beg, 1 代表 this.beg + 1, ...
    curr: RelativeVolumePos,
}

impl<'a> Iterator for AtomDEntryIterInGroupIter<'a> {
    type Item = AtomDEntryView<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        let cluster = self.this.bufs.get(self.curr.cluster as usize)?;
        let entry = cluster
            .get(self.curr.offset as usize..(self.curr.offset + DENTRY_SIZE) as usize)
            .unwrap();
        let entry = AtomDEntryView::new(entry);
        self.curr.add(DENTRY_SIZE);
        self.curr.try_to_next(self.this.cls_size_byte);
        Some(entry)
    }
}

impl GroupDEntryIter {
    fn atom_iter(&self) -> AtomDEntryIterInGroupIter {
        AtomDEntryIterInGroupIter {
            this: self,
            curr: VolumePos::zero(),
        }
    }

    fn get_std_entry(&self) -> AtomDEntryView {
        // curr 的前一个 32 byte 就是 8.3
        let offset = self.curr.offset - DENTRY_SIZE;
        let cluster = self.bufs.last().unwrap();
        let entry = cluster.get(offset as usize..(offset + DENTRY_SIZE) as usize).unwrap();
        let atom_entry = AtomDEntryView::new(entry);
        debug_assert!(atom_entry.is_std());
        atom_entry
    }

    fn is_std_only(&self) -> bool {
        // 只有一个 AtomDEntry
        self.curr.cluster == self.last.cluster
            && (self.curr.offset - self.last.offset) == DENTRY_SIZE
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
            name.retain(|&x| x != 0);
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
        self.last_cls_dirty = true;
    }
    pub(super) fn delete_entry(&mut self) {
        self.delay_clear = true;
        todo!()
    }
    pub(super) fn create_entry(&mut self) {
        todo!()
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

type RelativeAtomDEPos = AtomDEPos;
struct AtomDEntryWindow {
    fs: &'static Fat32FS,
    ///  |----|++++++++++++++++++|----|
    /// beg   left(r)     right(r)   end
    sector_bufs: VecDeque<SectorBuf>,
    buf_in_use: u32,
    begin_pos: AtomDEPos,
    // relative to begin_pos
    left_pos: RelativeAtomDEPos,
    // relative to begin_pos
    right_pos: RelativeAtomDEPos,
}

const BLK_ADE_CNT: usize = BLOCK_SIZE / DENTRY_SIZE as usize;

impl AtomDEntryWindow {
    const INIT_BUF_CNT: usize = 2;
    pub fn new(fs: &'static Fat32FS, begin_pos: AtomDEPos) -> AtomDEntryWindow {
        let mut sector_bufs = VecDeque::with_capacity(4);
        for _ in 0..Self::INIT_BUF_CNT {
            sector_bufs.push_back(SectorBuf::alloc());
        }
        AtomDEntryWindow {
            fs,
            sector_bufs,
            begin_pos,
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

        Ok(())
    }

    pub async fn move_right_one(&mut self) -> SysResult<Option<AtomDEntryView>> {
        let new_right = self.right_pos + 1;

        // 1. check if the next ADE pass the capacity
        // if so, alloc a new buffer
        let new_right_buf_idx = (new_right as usize) / BLK_ADE_CNT;
        if new_right_buf_idx >= self.sector_bufs.len() {
            self.sector_bufs.push_back(SectorBuf::alloc());
        }

        // 2. check if the next ADE pass last buffer
        // if so, load the new buffer & update the buf_in_use
        let last_idx = self.buf_in_use as usize;
        if new_right_buf_idx > last_idx {
            // find next sector of the last buffer
            let last_buf = &self.sector_bufs[last_idx];
            let next_sector = match self.fs.next_sector(last_buf.id) {
                Some(s) => s,
                None => return Ok(None),
            };

            // re-assign the new buffer and load content
            let new_buf = &mut self.sector_bufs[last_idx + 1];
            new_buf.reassign(next_sector);
            self.load(new_right_buf_idx).await?;
            self.buf_in_use += 1;
        }

        // 3. update the right_pos and return result
        self.right_pos = new_right;
        Ok(Some(self.get_pos(new_right)))
    }

    /// get the ADE with its AtomDEPos in current directory
    pub fn get_pos(&self, pos: AtomDEPos) -> AtomDEntryView {
        let relative_pos = pos - self.begin_pos;
        self.get_idx(relative_pos as usize)
    }

    /// get the ADE with its relative AtomDEPos in this window
    pub fn get_relative_pos(&self, pos: RelativeAtomDEPos) -> AtomDEntryView {
        self.get_idx(pos as usize)
    }

    /// get the ADE with its index in current ADE window
    pub fn get_idx(&self, wade_idx: usize) -> AtomDEntryView {
        let buf_idx = wade_idx / BLK_ADE_CNT;
        let buf_off = wade_idx % BLK_ADE_CNT;

        let buf = &self.sector_bufs[buf_idx];
        let buf_off_byte = buf_off * DENTRY_SIZE as usize;
        let slice = &buf.data[buf_off_byte..(buf_off_byte + DENTRY_SIZE as usize)];
        AtomDEntryView::new(slice)
    }

    /// how many ADE this windows holding now
    pub fn windows_len(&self) -> usize {
        (self.right_pos - self.left_pos) as usize
    }
    /// left bound of this windows, in current directory
    pub fn left_pos(&self) -> AtomDEPos {
        self.begin_pos + self.left_pos
    }
    /// right bound of this windows, in current directory
    pub fn right_pos(&self) -> AtomDEPos {
        self.begin_pos + self.right_pos
    }

    pub fn last(&self) -> AtomDEntryView {
        self.get_idx(self.windows_len() - 1)
    }
}

pub(super) struct AtomDEntryWindowIter<'a> {
    this: &'a AtomDEntryWindow,
    cur: RelativeAtomDEPos,
}

impl<'a> Iterator for AtomDEntryWindowIter<'a> {
    type Item = AtomDEntryView<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.cur as usize >= self.this.windows_len() {
            None
        } else {
            let res = self.this.get_relative_pos(self.cur);
            self.cur += 1;
            Some(res)
        }
    }
}
