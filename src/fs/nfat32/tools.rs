use super::{BlkDevRef, BlockID, ClusterID, Fat32FS, SctOffsetT, SectorID};
use crate::{
    drivers::DevError,
    fs::disk::{BLOCK_SIZE, LOG2_BLOCK_SIZE},
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
        let result = {
            let cache = self.cache.lock(here!());
            cache.get(&id).cloned()
        };
        match result {
            Some(entry) => Ok(entry),
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
    pub fn new_empty() -> Self {
        Self(SyncUnsafeCell::new(Vec::new()))
    }
    pub fn new(fs: &'static Fat32FS, begin_cluster: ClusterID) -> Self {
        let chain = fs.with_fat(|f| f.chain(begin_cluster));
        Self(SyncUnsafeCell::new(chain))
    }

    fn inner(&self) -> &mut Vec<ClusterID> {
        unsafe { &mut *self.0.get() }
    }

    pub fn len(&self) -> usize {
        self.inner().len()
    }
    pub fn first(&self) -> ClusterID {
        *self.inner().first().unwrap()
    }
    pub fn last(&self) -> ClusterID {
        *self.inner().last().unwrap()
    }

    /// 向链尾添加一个 cluster
    pub fn add(&self, cluster: ClusterID) {
        self.inner().push(cluster)
    }

    pub fn alloc_push(&self, n: usize, fs: &'static Fat32FS) {
        fs.with_fat(|f| {
            for _ in 0..n {
                self.inner().push(f.alloc());
            }
        })
    }
    pub fn free_pop(&self, n: usize, fs: &'static Fat32FS) {
        fs.with_fat(|f| {
            for _ in 0..n {
                f.free(self.inner().pop().unwrap());
            }
        })
    }
    pub fn free_all(&self, fs: &'static Fat32FS) {
        fs.with_fat(|f| {
            for c in self.inner() {
                f.free(*c);
            }
        })
    }

    /// 根据文件内的字节偏移量, 计算出这个偏移量的位置所在的扇区号和扇区内偏移量
    pub fn offset_sct(&self, fs: &'static Fat32FS, byte_offset: usize) -> (SectorID, SctOffsetT) {
        // 希望再也不用碰这块逻辑, 哈人

        // 计算出 log2(cluster size in byte). 因为我们知道 cluster size 是 2 的幂,
        // 而 rustc 在编译时不知道, 所以我们要手动计算
        // 这个函数目测调用还挺频繁的, 用位运算优化掉乘除法应该能提升不少性能
        let lcsb = fs.log_cls_size_sct as usize + LOG2_BLOCK_SIZE;
        // 计算出 byte_offset 越过了几个 cluster
        let cluster_idx = byte_offset >> lcsb;
        // 然后在缓存的本文件 cluster 链中, 找到对应的 cluster 的 id
        let cluster_id = self.inner()[cluster_idx];
        // 然后向文件系统查询这个 cluster 的第一个 sector 的 id
        let first_sector_id = fs.first_sector(cluster_id);

        // 然后计算出在本 cluster 中的 byte offset
        let cluster_offset = byte_offset & !(!0 << lcsb);
        // 并计算这个偏移越过了多少个 cluster 中的 sector
        let sector_idx = cluster_offset >> LOG2_BLOCK_SIZE;
        // 由于同一个 cluster 内的 sector 是连续的, 所以直接相加就可以计算出 sector id
        let sector_id = first_sector_id + sector_idx as SectorID;

        // 最后计算出在 sector 中的字节偏移量并返回
        let sector_offset = cluster_offset & !(!0 << LOG2_BLOCK_SIZE);
        (sector_id, sector_offset as SctOffsetT)
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
