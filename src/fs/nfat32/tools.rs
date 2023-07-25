use super::{BlkDevRef, BlockID, ClusterID, Fat32FS, SctOffsetT, SectorID};
use crate::{
    drivers::DevError,
    fs::disk::BLOCK_SIZE,
    here,
    sync::{SleepLock, SpinNoIrqLock},
    tools::errors::{SysError, SysResult},
};
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use core::{
    cell::SyncUnsafeCell,
    sync::atomic::{AtomicBool, AtomicUsize},
};

pub(super) fn cvt_err(dev_err: DevError) -> SysError {
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

pub(super) type BlockCacheEntryRef = Arc<BlockCacheEntry>;
pub(super) struct CachedBlkDev {
    blk_dev: SleepLock<BlkDevRef>,
    cache: SpinNoIrqLock<BTreeMap<BlockID, BlockCacheEntryRef>>,
}

impl CachedBlkDev {
    pub fn new(blk_dev: BlkDevRef) -> Self {
        CachedBlkDev {
            blk_dev: SleepLock::new(blk_dev),
            cache: SpinNoIrqLock::new(BTreeMap::new()),
        }
    }

    pub async fn get(&self, id: BlockID) -> SysResult<BlockCacheEntryRef> {
        match { self.cache.lock(here!()).get(&id) } {
            Some(entry) => Ok(entry.clone()),
            None => {
                let entry = BlockCacheEntry::new();
                self.read_noc(id, entry.just_mut()).await?;
                let entry = Arc::new(entry);
                self.cache.lock(here!()).insert(id, entry.clone());
                Ok(entry)
            }
        }
    }

    pub async fn read(&self, id: BlockID, buf: &mut [u8]) -> SysResult {
        let entry = self.get(id).await?;
        buf.copy_from_slice(entry.as_slice());
        Ok(())
    }

    pub async fn write(&self, id: BlockID, buf: &[u8]) -> SysResult {
        let entry = self.get(id).await?;
        entry.as_mut_slice().copy_from_slice(buf);
        // TODO: if too many blocks & too less use_cnt, write back the block
        Ok(())
    }

    pub async fn read_noc(&self, id: BlockID, buf: &mut [u8]) -> SysResult {
        let blk_dev = self.blk_dev.lock().await;
        blk_dev.read_block(id, buf).await.map_err(cvt_err)
    }
    pub async fn write_noc(&self, id: BlockID, buf: &[u8]) -> SysResult {
        let blk_dev = self.blk_dev.lock().await;
        blk_dev.write_block(id, buf).await.map_err(cvt_err)
    }

    pub async fn read_noc_multi(&self, id: BlockID, n: usize, buf: &mut [u8]) -> SysResult {
        let blk_dev = self.blk_dev.lock().await;
        for i in 0..n {
            let buf = &mut buf[i * BLOCK_SIZE..(i + 1) * BLOCK_SIZE];
            blk_dev.read_block(id + i as BlockID, buf).await.map_err(cvt_err)?;
        }
        Ok(())
    }
    pub async fn write_noc_multi(&self, id: BlockID, n: usize, buf: &[u8]) -> SysResult {
        let blk_dev = self.blk_dev.lock().await;
        for i in 0..n {
            let buf = &buf[i * BLOCK_SIZE..(i + 1) * BLOCK_SIZE];
            blk_dev.write_block(id + i as BlockID, buf).await.map_err(cvt_err)?;
        }
        Ok(())
    }
}

pub(super) struct BlockCacheEntry {
    data: [u8; BLOCK_SIZE],
    dirty: AtomicBool,
    use_cnt: AtomicUsize,
}

impl BlockCacheEntry {
    pub fn new() -> Self {
        Self {
            data: [0; BLOCK_SIZE],
            dirty: AtomicBool::new(false),
            use_cnt: AtomicUsize::new(0),
        }
    }
    fn add_use_cnt(&self, offset: usize) {
        self.use_cnt.fetch_add(offset, core::sync::atomic::Ordering::Relaxed);
    }

    pub fn as_slice(&self) -> &[u8] {
        self.add_use_cnt(1);
        &self.data
    }

    pub fn as_mut_slice(&self) -> &mut [u8] {
        self.add_use_cnt(1);
        self.dirty.store(true, core::sync::atomic::Ordering::Relaxed);
        self.just_mut()
    }

    fn just_mut(&self) -> &mut [u8] {
        unsafe { &mut *(self.data.as_slice() as *const _ as *mut _) }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(core::sync::atomic::Ordering::Relaxed)
    }
    pub fn use_cnt(&self) -> usize {
        self.use_cnt.load(core::sync::atomic::Ordering::Relaxed)
    }
}

pub(super) struct ClusterChain(SyncUnsafeCell<Vec<ClusterID>>);
impl ClusterChain {
    pub fn new() -> Self {
        Self(SyncUnsafeCell::new(Vec::new()))
    }

    fn inner(&self) -> &mut Vec<ClusterID> {
        unsafe { &mut *self.0.get() }
    }

    pub fn len(&self) -> usize {
        self.inner().len()
    }
    pub fn first(&self) -> ClusterID {
        self.inner().first().unwrap().clone()
    }
    pub fn last(&self) -> ClusterID {
        self.inner().last().unwrap().clone()
    }

    pub fn add(&self, cluster: ClusterID) {
        self.inner().push(cluster)
    }

    pub fn get_sector(&self, fs: &'static Fat32FS, byte_offset: usize) -> (SectorID, SctOffsetT) {
        let cluster_idx = byte_offset / fs.cluster_size_byte as usize;
        let cluster_offset = byte_offset % fs.cluster_size_byte as usize;

        todo!()
    }
}

pub(super) struct WithDirty<T> {
    inner: SyncUnsafeCell<T>,
    dirty: AtomicBool,
}

impl<T> WithDirty<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: SyncUnsafeCell::new(inner),
            dirty: AtomicBool::new(false),
        }
    }

    pub fn as_ref(&self) -> &T {
        unsafe { &*self.inner.get() }
    }
    pub fn as_mut(&self) -> &mut T {
        self.dirty.store(true, core::sync::atomic::Ordering::Relaxed);
        unsafe { &mut *self.inner.get() }
    }

    pub fn dirty(&self) -> bool {
        self.dirty.load(core::sync::atomic::Ordering::Relaxed)
    }
    pub fn clear(&self) {
        self.dirty.store(false, core::sync::atomic::Ordering::Relaxed);
    }
}

impl<T> Drop for WithDirty<T> {
    fn drop(&mut self) {
        if self.dirty() {
            log::warn!("Dropping a dirty WithDirty, may cause data loss");
        }
    }
}
