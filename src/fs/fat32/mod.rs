use super::{
    disk::BLOCK_SIZE,
    new_vfs::{
        underlying::{ConcreteDEntryRef, ConcreteFile},
        VfsFileAttr, VfsFileKind,
    },
};
use crate::{
    consts::PAGE_SIZE,
    drivers::{AsyncBlockDevice, DevError},
    here,
    sync::{SleepLock, SpinNoIrqLock},
    tools::errors::{dyn_future, SysError, SysResult},
};
use alloc::{boxed::Box, collections::BTreeMap, string::String, sync::Arc, vec::Vec};
use core::{
    cmp::min,
    future::Future,
    pin::{pin, Pin},
    slice,
    task::Poll,
};
use futures::Stream;

// https://wiki.osdev.org/FAT

pub type BlkDevRef = Arc<dyn AsyncBlockDevice>;

type BlockID = u64;
type SectorID = u64;
// fat32 ==> cluster id is within u32
type ClusterID = u32;
// byte offset within a cluster
type ClsOffsetT = u16;

pub struct Fat32FS {
    dir_blocks: SpinNoIrqLock<BTreeMap<BlockID, [u8; BLOCK_SIZE]>>,
    block_dev: SleepLock<BlkDevRef>,
    fat_table_mgr: SpinNoIrqLock<FATTableManager>,

    // FS Info
    device_id: usize,
    cluster_size_byte: usize,
    cluster_size_sct: usize,
    data_begin_sct: SectorID,
    root_id_cls: ClusterID,
}

macro_rules! parse {
    (u8, $buf:expr, $beg_idx:expr) => {
        $buf[$beg_idx]
    };
    (u16, $buf:expr, $beg_idx:expr) => {
        u16::from_le_bytes($buf[$beg_idx..($beg_idx + 2)].try_into().unwrap())
    };
    (u32, $buf:expr, $beg_idx:expr) => {
        u32::from_le_bytes($buf[$beg_idx..($beg_idx + 4)].try_into().unwrap())
    };
}

fn cvt_err(dev_err: DevError) -> SysError {
    match dev_err {
        DevError::AlreadyExists => SysError::EEXIST,
        DevError::Again => SysError::EAGAIN,
        DevError::BadState => SysError::EIO,
        DevError::InvalidParam => SysError::EINVAL,
        DevError::IO => SysError::EIO,
        DevError::NoMemory => SysError::ENOMEM,
        DevError::ResourceBusy => SysError::EBUSY,
        DevError::Unsupported => SysError::EINVAL,
    }
}

// abbr: _sct: sector, _byte: byte, _clu: cluster, _cnt: element count (mostly item = dentry)
impl Fat32FS {
    pub async fn new(blk_dev: BlkDevRef) -> SysResult<Self> {
        let mut boot_record: [u8; BLOCK_SIZE] = [0; BLOCK_SIZE];
        blk_dev.read_block(0, &mut boot_record).await.map_err(cvt_err)?;

        let sector_size_byte = parse!(u16, boot_record, 0x0B);
        let cluster_size_sct = parse!(u8, boot_record, 0x0D);
        let cluster_size_byte = (cluster_size_sct as u16) * sector_size_byte;

        if sector_size_byte as usize != BLOCK_SIZE {
            panic!("FAT32: byte per sector is not 512");
        }
        if cluster_size_byte as usize > PAGE_SIZE {
            panic!("FAT32: cluster size is too large (> PAGE_SIZE)");
        }

        log::debug!("FAT BPB: sector_size: {} (byte)", sector_size_byte);
        log::debug!("FAT BPB: cluster_size: {} (byte)", cluster_size_byte);

        // how many sectors are reserved for boot record
        // after `reserved_size_sct` sectors, the first FAT table begins
        let reserved_size_sct = parse!(u16, boot_record, 0x0E);
        let fat_cnt = parse!(u8, boot_record, 0x10);
        let root_dentry_cnt = parse!(u16, boot_record, 0x11);

        let volume_size_16_sct = parse!(u16, boot_record, 0x13);
        let volume_size_sct = if volume_size_16_sct == 0 {
            parse!(u32, boot_record, 0x20)
        } else {
            volume_size_16_sct as u32
        };

        log::debug!("FAT BPB: fat cnt: {}", fat_cnt);
        log::debug!("FAT BPB: root dentry cnt: {}", root_dentry_cnt);
        log::debug!("FAT BPB: sector cnt: {}", volume_size_sct);

        let fat_size_sct = parse!(u32, boot_record, 0x024);
        let root_id_cls = parse!(u32, boot_record, 0x02C) as ClusterID;
        let volume_id = parse!(u32, boot_record, 0x043);

        log::debug!("FAT32 EBPB: sector per fat: {}", fat_size_sct);
        log::debug!("FAT32 EBPB: root dir cluster id: {}", root_id_cls);
        log::debug!("FAT32 EBPB: volume id: {}", volume_id);

        // calculate fat table begin
        let first_fat_begin_sct = reserved_size_sct as SectorID;

        let mut fat_begins = Vec::new();
        for i in 0..fat_cnt {
            let offset: SectorID = (i as SectorID) * (fat_size_sct as SectorID);
            fat_begins.push(first_fat_begin_sct + offset);
        }

        // TODO: examine all the fat tables are the same

        let main_fat_size_byte = (fat_size_sct as usize) * (sector_size_byte as usize);
        let mut main_fat =
            unsafe { Box::<[u32]>::new_uninit_slice(main_fat_size_byte).assume_init() };
        for i in 0..fat_size_sct {
            let blk_offset = first_fat_begin_sct + i as BlockID;
            // 4 for 4*u8 == u32
            let beg = (i as usize) * (sector_size_byte as usize) / 4;
            let end = ((i + 1) as usize) * (sector_size_byte as usize) / 4;
            let slice: &mut [u8] = unsafe {
                slice::from_raw_parts_mut(
                    (&mut main_fat[beg..end]) as *mut [u32] as *mut u8,
                    sector_size_byte as usize,
                )
            };
            blk_dev.read_block(blk_offset, slice).await.map_err(cvt_err)?;
        }

        let fat_table_mgr = FATTableManager {
            fat_begins,
            fat: main_fat,
        };
        fat_table_mgr.debug_print_all_used_cluster();

        let data_begin_sct =
            first_fat_begin_sct + (fat_cnt as SectorID) * (fat_size_sct as SectorID);

        log::debug!(
            "FAT32 Info: Root dentry chains: {:?}",
            fat_table_mgr.chain(root_id_cls)
        );
        log::debug!(
            "FAT32 Info: First free cluster: {}",
            fat_table_mgr.find_first_free()
        );
        log::debug!("FAT32 Info: Data begin sector: {}", data_begin_sct);

        Ok(Fat32FS {
            dir_blocks: SpinNoIrqLock::new(BTreeMap::new()),
            block_dev: SleepLock::new(blk_dev),
            fat_table_mgr: SpinNoIrqLock::new(fat_table_mgr),

            // FS Info
            device_id: 0, // TODO: get device id
            cluster_size_sct: cluster_size_sct as usize,
            cluster_size_byte: cluster_size_byte as usize,
            data_begin_sct,
            root_id_cls,
        })
    }

    pub fn root(&'static self) -> FATFile {
        FATFile {
            fs: self,
            begin_cluster: self.root_id_cls,
            last_cluster: None,
        }
    }

    fn with_fat<T>(&self, f: impl FnOnce(&FATTableManager) -> T) -> T {
        let fat_table_mgr = self.fat_table_mgr.lock(here!());
        f(&fat_table_mgr)
    }

    fn device_id(&self) -> usize {
        self.device_id
    }

    fn offset_cls(&self, offset_byte: usize) -> (usize, ClsOffsetT) {
        let cluster_size_byte = self.cluster_size_byte;
        let cluster_offset = offset_byte % cluster_size_byte;
        let cluster_id = offset_byte / cluster_size_byte;
        (cluster_id, cluster_offset as ClsOffsetT)
    }

    fn first_sector(&self, cluster_id: ClusterID) -> SectorID {
        self.data_begin_sct + (cluster_id as SectorID - 2) * (self.cluster_size_sct as SectorID)
    }

    async fn read_cluster(&self, cid: ClusterID, mut buf: &mut [u8]) -> SysResult<()> {
        let blkdev = self.block_dev.lock().await;
        let sct = self.first_sector(cid);
        for i in 0..self.cluster_size_sct {
            let slice_len = min(buf.len(), BLOCK_SIZE);
            let blk_id = sct + i as SectorID;
            log::debug!("read block: blk_id: {}", blk_id);
            blkdev.read_block(blk_id, &mut buf[..slice_len]).await.map_err(cvt_err)?;

            buf = &mut buf[slice_len..];
            if buf.is_empty() {
                break;
            }
        }
        Ok(())
    }

    async fn write_cluster(&self, cid: ClusterID, mut buf: &[u8]) -> SysResult<()> {
        let blkdev = self.block_dev.lock().await;
        for _ in 0..self.cluster_size_sct {
            let sct = self.first_sector(cid);
            let slice_len = min(buf.len(), BLOCK_SIZE);
            blkdev.write_block(sct, &buf[..slice_len]).await.map_err(cvt_err)?;

            buf = &buf[slice_len..];
            if buf.is_empty() {
                break;
            }
        }
        Ok(())
    }
}
// TODO: contents of a FAT dentry

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    struct Fat32DEntryAttr : u8 {
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

    fn attr(&self) -> super::new_vfs::VfsFileAttr {
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

pub struct FATFile {
    fs: &'static Fat32FS,
    begin_cluster: ClusterID,
    last_cluster: Option<ClusterID>,
}

fn round_up(x: usize, y: usize) -> usize {
    (x + y - 1) / y
}

impl FATFile {
    pub fn dentry_iter(&self) -> Pin<Box<dyn Stream<Item = SysResult<FATDentry>>>> {
        Box::pin(DEntryIter::new(self.fs, self.begin_cluster))
    }
}

impl ConcreteFile for FATFile {
    type DEntryRefT = FATDentry;

    fn read_at<'a>(
        &'a self,
        offset: usize,
        buf: &'a mut [u8],
    ) -> crate::tools::errors::ASysResult<usize> {
        let cls_size_byte = self.fs.cluster_size_byte;
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
        let cls_size_byte = self.fs.cluster_size_byte;
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
        skip_n: usize,
        name: Option<&str>,
    ) -> crate::tools::errors::ASysResult<(bool, Vec<Self::DEntryRefT>)> {
        todo!()
    }

    fn set_attr(
        &self,
        dentry_ref: Self::DEntryRefT,
        attr: super::new_vfs::VfsFileAttr,
    ) -> crate::tools::errors::ASysResult {
        todo!()
    }

    fn create(
        &self,
        name: &str,
        kind: super::new_vfs::VfsFileKind,
    ) -> crate::tools::errors::ASysResult<Self::DEntryRefT> {
        todo!()
    }

    fn remove(&self, dentry_ref: Self::DEntryRefT) -> crate::tools::errors::ASysResult {
        todo!()
    }

    fn detach(&self, dentry_ref: Self::DEntryRefT) -> crate::tools::errors::ASysResult<Self> {
        todo!()
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
        let buf = unsafe { Box::new_uninit_slice(fs.cluster_size_byte).assume_init() };
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

struct FATTableManager {
    fat_begins: Vec<SectorID>,
    fat: Box<[u32]>,
}

impl FATTableManager {
    fn get_fat(&self, cid: ClusterID) -> u32 {
        self.fat[cid as usize]
    }

    pub fn find_first_free(&self) -> ClusterID {
        // TODO: optimize
        for (i, &v) in self.fat.iter().enumerate() {
            if v == 0 {
                return i as ClusterID;
            }
        }
        panic!("FAT: no free cluster");
    }

    pub fn find(&self, beg: ClusterID, skip_n: usize) -> Option<ClusterID> {
        let mut cur = beg;
        for _ in 0..skip_n {
            if let Some(next) = self.next(cur) {
                cur = next;
            } else {
                return None;
            }
        }
        Some(cur)
    }

    pub fn find_range(&self, beg: ClusterID, skip_n: usize, n: usize) -> Option<Vec<ClusterID>> {
        let start = self.find(beg, skip_n)?;
        let mut ret = Vec::new();
        let mut cur = start;
        for _ in 0..n {
            ret.push(cur);
            if let Some(next) = self.next(cur) {
                cur = next;
            } else {
                break;
            }
        }
        Some(ret)
    }

    pub fn alloc(&mut self) -> ClusterID {
        let cid = self.find_first_free();
        self.fat[cid as usize] = 0x0FFFFFFF;
        cid
    }

    pub fn set_next(&mut self, cid: ClusterID, next_cid: ClusterID) {
        self.fat[cid as usize] = next_cid as u32;
    }

    pub fn next(&self, cid: ClusterID) -> Option<ClusterID> {
        let next_cid = self.get_fat(cid);
        if next_cid >= 0x0FFFFFF8 {
            None
        } else {
            Some(next_cid as ClusterID)
        }
    }

    pub fn chain(&self, beg: ClusterID) -> Vec<ClusterID> {
        let mut ret = Vec::new();
        let mut cur = beg;
        ret.push(cur);
        while let Some(next) = self.next(cur) {
            ret.push(next);
            cur = next;
        }
        ret
    }

    pub fn debug_print_all_used_cluster(&self) {
        self.fat.iter().enumerate().for_each(|(i, &v)| {
            if v != 0 {
                log::debug!("FAT: cluster {} is used", i);
            }
        });
    }
}

struct MemBlock(Box<[u8]>);

impl MemBlock {
    fn new(byte_size: usize) -> Self {
        MemBlock(unsafe { Box::<[u8]>::new_uninit_slice(byte_size).assume_init() })
    }
}
