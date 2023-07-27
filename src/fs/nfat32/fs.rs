use super::tools::CachedBlkDev;
use super::{super::disk::BLOCK_SIZE, BlockID, ClusterID, FATFile, SectorID};
use crate::fs::nfat32::tools::cvt_err;

use crate::{
    consts::PAGE_SIZE, drivers::AsyncBlockDevice, fs::nfat32::parse, here, sync::SpinNoIrqLock,
    tools::errors::SysResult,
};
use alloc::{boxed::Box, sync::Arc, vec::Vec};

use crate::fs::new_vfs::path_cache::PathCacheDir;
use crate::fs::new_vfs::sync_attr_file::SyncAttrFile;
use crate::fs::new_vfs::top::{VfsFS, VfsFileRef};
use crate::fs::new_vfs::underlying::ConcreteFS;
use core::pin::Pin;
use core::slice;

pub type BlkDevRef = Arc<dyn AsyncBlockDevice>;

pub struct Fat32FS {
    block_dev: CachedBlkDev,
    fat_table_mgr: SpinNoIrqLock<FATTableManager>,

    // FS Info
    device_id: usize,
    pub(super) cluster_size_byte: u32,
    pub(super) cluster_size_sct: u32,
    /// log2(cluster_size_sct), 用于便利地计算 SID -> CID
    pub(super) log_cls_size_sct: u8,
    pub(super) data_begin_sct: SectorID,
    pub(super) root_id_cls: ClusterID,
}

fn int_log2(x: u8) -> u8 {
    match x {
        0x01 => 0,
        0x02 => 1,
        0x04 => 2,
        0x08 => 3,
        0x10 => 4,
        0x20 => 5,
        0x40 => 6,
        0x80 => 7,
        _ => unreachable!("int_log2: invalid input"),
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

        // 用于便利地计算 SID -> CID
        let log_cls_size_sct = int_log2(cluster_size_sct);

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
        // fat_table_mgr.debug_print_all_used_cluster();

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
            block_dev: CachedBlkDev::new(blk_dev),
            fat_table_mgr: SpinNoIrqLock::new(fat_table_mgr),

            // FS Info
            device_id: 114514, // TODO: get device id
            cluster_size_sct: cluster_size_sct as u32,
            log_cls_size_sct,
            cluster_size_byte: cluster_size_byte as u32,
            data_begin_sct,
            root_id_cls,
        })
    }

    pub(super) fn with_fat<T>(&self, f: impl FnOnce(&mut FATTableManager) -> T) -> T {
        let mut fat_table_mgr = self.fat_table_mgr.lock(here!());
        f(&mut fat_table_mgr)
    }

    pub(super) fn device_id(&self) -> usize {
        self.device_id
    }

    pub(super) fn first_sector(&self, cluster_id: ClusterID) -> SectorID {
        // this formula can be cross verified with Self::next_sector,
        // and it's copied from https://wiki.osdev.org/FAT
        self.data_begin_sct + (cluster_id as SectorID - 2) * (self.cluster_size_sct as SectorID)
    }

    pub(super) fn find_cluster(&self, sector_id: SectorID) -> ClusterID {
        let lscc = self.log_cls_size_sct as u32;
        let relative_sid = sector_id - self.data_begin_sct;
        // this formula can be cross verified with Self::first_sector
        let cluster_id = ((relative_sid >> lscc) + 2) as ClusterID;
        cluster_id
    }

    pub(super) fn next_sector(&self, sid: SectorID) -> Option<SectorID> {
        let lcss = self.log_cls_size_sct as u32;
        let relative_sid = sid - self.data_begin_sct;
        // this formula can be cross verified with Self::first_sector
        let cluster_id = ((relative_sid >> lcss) + 2) as ClusterID;

        let offset = relative_sid & !(!0 << lcss);
        if offset == (1 << lcss) - 1 {
            self.with_fat(|fat| fat.next(cluster_id)).map(|ncid| self.first_sector(ncid))
        } else {
            Some(sid + 1)
        }
    }

    pub(super) async fn read_sector(&self, sid: SectorID, buf: &mut [u8]) -> SysResult<()> {
        log::debug!("read sector: sid: {}", sid);
        if buf.len() == BLOCK_SIZE {
            self.block_dev.read_noc(sid, buf).await
        } else {
            let mut blk_buf = [0; BLOCK_SIZE];
            self.block_dev.read_noc(sid, &mut blk_buf).await?;
            buf.copy_from_slice(blk_buf[0..buf.len()].as_ref());
            Ok(())
        }
    }
    pub(super) async fn write_sector(&self, sid: SectorID, buf: &[u8]) -> SysResult<()> {
        log::debug!("write sector: sid: {}", sid);
        if buf.len() == BLOCK_SIZE {
            self.block_dev.write_noc(sid, buf).await
        } else {
            let mut blk_buf = [0; BLOCK_SIZE];
            self.block_dev.read_noc(sid, &mut blk_buf).await?;
            blk_buf[0..buf.len()].copy_from_slice(buf);
            self.block_dev.write_noc(sid, &blk_buf).await
        }
    }

    pub(super) fn block_dev(&self) -> &CachedBlkDev {
        &self.block_dev
    }
}

pub(super) struct FATTableManager {
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
    pub fn free(&mut self, cid: ClusterID) {
        self.fat[cid as usize] = 0;
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

pub struct FatFSWrapper {
    fs: Pin<Box<Fat32FS>>,
}

impl FatFSWrapper {
    pub async fn new(blk_dev: BlkDevRef) -> SysResult<Self> {
        Fat32FS::new(blk_dev).await.map(Box::pin).map(|fs| Self { fs })
    }

    pub fn get(&self) -> &'static Fat32FS {
        unsafe { &*(&*self.fs as *const Fat32FS) }
    }
}

impl VfsFS for FatFSWrapper {
    fn root(&self) -> VfsFileRef {
        let cf = FATFile::new_free(
            self.get(),
            self.get().root_id_cls,
            crate::fs::new_vfs::VfsFileKind::Directory,
        );
        VfsFileRef::new(PathCacheDir::new_root(SyncAttrFile::new(cf)))
    }
}
