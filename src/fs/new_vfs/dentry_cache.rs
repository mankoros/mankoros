use super::{underlying::ConcreteFile, top::{VfsFileRef, VfsFile}, sync_attr_cache::SyncAttrCacheFile};
use crate::{sync::{SpinNoIrqLock}, impl_vfs_default_non_file, tools::errors::{ASysResult, dyn_future, SysResult, SysError}, fs::{new_vfs::{VfsFileKind, page_cache::SyncPageCacheFile}}, here};
use alloc::{collections::BTreeMap, string::String, vec::Vec};
use core::{sync::atomic::{AtomicBool, Ordering}, cell::{SyncUnsafeCell}};
use crate::fs::new_vfs::underlying::DEntryRef;
use crate::alloc::string::ToString;

enum DEntryCache<F: ConcreteFile> {
    Unactive(F::DEntryRefT),
    Internal(F::DEntryRefT, VfsFileRef),
    /// 被外部的 mount 等操作遮盖了的目录项
    External(VfsFileRef, F::DEntryRefT, VfsFileRef),
}

unsafe impl<F: ConcreteFile> Send for DEntryCache<F> {}
unsafe impl<F: ConcreteFile> Sync for DEntryCache<F> {}

/// SyncUnsafeCell 只提供了 get() 方法, 返回的是指针, 而指针是被强迫实现了 !Send 的.
/// 在 async 上下文中, rustc 并没有智能到能意识到 `unsafe { &*cell.get() }` 中出现的指针只是临时变量,
/// 而是会尝试将指针存起来, 导致编译错误. 为此, 我们需要一个新的 Cell, 它提供了一个直接的获得 &T 和 &mut T 的方法,
/// 使其能够在 async 上下文中使用.
struct AsyncUnsafeCell<T: Sized>(SyncUnsafeCell<T>);
impl<T: Sized> AsyncUnsafeCell<T> {
    pub const fn new(t: T) -> Self {
        Self(SyncUnsafeCell::new(t))
    }
    pub unsafe fn get(&self) -> &T {
        &*self.0.get()
    }
    pub unsafe fn get_mut(&self) -> &mut T {
        &mut *self.0.get()
    }
}

type EntriesMap<F> = BTreeMap<String, AsyncUnsafeCell<DEntryCache<F>>>;

pub struct DEntryCacheDir<F: ConcreteFile> {
    all_cached: AtomicBool,
    entries: SpinNoIrqLock<EntriesMap<F>>,
    dir: SyncAttrCacheFile<F>,
}

async fn pack_entry<F: ConcreteFile>(dentry_ref: &F::DEntryRefT) -> SysResult<VfsFileRef> {
    let sync_file = SyncAttrCacheFile::<F>::new(dentry_ref);
    let vfs_file: VfsFileRef = 
        if sync_file.kind() == VfsFileKind::Directory {
            VfsFileRef::new(DEntryCacheDir::new(sync_file))
        } else {
            VfsFileRef::new(SyncPageCacheFile::new(sync_file))
        };
    Ok(vfs_file)
}

impl<F: ConcreteFile> DEntryCache<F> {
    pub async fn active(&mut self) -> SysResult<VfsFileRef> {
        Ok(loop {
            match self {
                Self::Unactive(dentry_ref) => {
                    let vfs_file = pack_entry::<F>(dentry_ref).await?;
                    *self = Self::Internal(dentry_ref.clone(), vfs_file.clone());
                    break vfs_file
                }
                Self::Internal(_, vfs_file) => break vfs_file.clone(),
                Self::External(vfs_file, _, _) => break vfs_file.clone(),
            }
        })
    }

    pub async fn shadow(&mut self, externel_file: VfsFileRef) -> SysResult {
        loop {
            match self {
                Self::Unactive(_) => {
                    self.active().await?;
                }
                Self::Internal(dentry_ref, vfs_file) => {
                    *self = Self::External(externel_file, dentry_ref.clone(), vfs_file.clone());
                    break Ok(())
                }
                Self::External(_, _, _) => {
                    break Err(SysError::EBUSY)
                }
            }
        }
    }

    pub async fn unshadow(&mut self) -> SysResult<(bool, VfsFileRef)> {
        loop {
            match self {
                Self::Unactive(_) => {
                    self.active().await?;
                }
                Self::Internal(_, vfs_file) => {
                    break Ok((true, vfs_file.clone()))
                }
                Self::External(new, dentry_ref, original) => {
                    let new = new.clone();
                    *self = Self::Internal(dentry_ref.clone(), original.clone());
                    break Ok((false, new.clone()))
                }
            }
        }
    }

    pub fn get_dentry_ref(&self) -> &F::DEntryRefT {
        match self {
            Self::Unactive(dentry_ref) => dentry_ref,
            Self::Internal(dentry_ref, _) => dentry_ref,
            Self::External(_, dentry_ref, _) => dentry_ref,
        }
    }
}


impl<F: ConcreteFile> DEntryCacheDir<F> {
    pub fn new_root(dir: SyncAttrCacheFile<F>) -> Self {
        Self::new(dir)
    }

    fn new(dir: SyncAttrCacheFile<F>) -> Self {
        Self {
            all_cached: AtomicBool::new(false),
            entries: SpinNoIrqLock::new(BTreeMap::new()),
            dir,
        }
    }

    fn is_all_cached(&self) -> bool {
        self.all_cached.load(core::sync::atomic::Ordering::Acquire)
    }

    // 因为 entries 是有锁的, 为了避免在一个小方法里频繁开关锁, 我们直接接受一个 entries_map 参数
    fn add_entry(entries: &mut EntriesMap<F>, dentry_ref: F::DEntryRefT) {
        let name = dentry_ref.name().to_string();
        let dentry_cache = AsyncUnsafeCell::new(DEntryCache::Unactive(dentry_ref));
        entries.insert(name, dentry_cache);
    }

    async fn lookup_entry(entries: &mut EntriesMap<F>, name: &str) -> SysResult<Option<VfsFileRef>> {
        if let Some(dentry_cache) = entries.get(name) {
            let dentry_cache = unsafe { dentry_cache.get_mut() };
            let vfs_file = dentry_cache.active().await?;
            Ok(Some(vfs_file))
        } else {
            Ok(None)
        }
    }

    async fn get_entries(&self, entries_map: &mut EntriesMap<F>, name: Option<&str>) -> SysResult<()> {
        let cached_cnt = entries_map.len();
        let (is_end, entries) = self.dir.lock().await.lookup_batch(cached_cnt, name).await?;
        for dentry_ref in entries {
            Self::add_entry(entries_map, dentry_ref);
        }
        if is_end {
            self.all_cached.store(true, Ordering::Release);
        }
        Ok(())
    }
}

impl<F: ConcreteFile> VfsFile for DEntryCacheDir<F> {
    impl_vfs_default_non_file!(DEntryCacheFile);

    fn attr(&self) -> ASysResult<super::VfsFileAttr> {
        dyn_future(async { Ok(self.dir.attr_clone()) })
    }

    fn list(&self) -> ASysResult<Vec<(String, VfsFileRef)>> {
        dyn_future(async {
            let mut entries_map = self.entries.lock(here!());

            // 1. 若非缓存了所有项，则先缓存所有项
            if !self.is_all_cached() {
                self.get_entries(&mut entries_map, None).await?;
            }

            // 2. 若有未 active 的项，则先 active
            for (_, dentry_cache) in entries_map.iter_mut() {
                let dentry_cache = unsafe { dentry_cache.get_mut() };
                dentry_cache.active().await?;
            }

            // 3. 收集并返回所有项
            let mut res = Vec::new();
            for (name, dentry_cache) in entries_map.iter() {
                let dentry_cache = unsafe { &*dentry_cache.get() };
                match dentry_cache {
                    DEntryCache::Internal(_, vfs_file) => {
                        res.push((name.clone(), vfs_file.clone()));
                    }
                    DEntryCache::External(vfs_file, _, _) => {
                        res.push((name.clone(), vfs_file.clone()));
                    }
                    _ => unreachable!(),
                }
            }
            Ok(res)
        })
    }

    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async {
            let mut entries_map = self.entries.lock(here!());
            
            // 1. 先尝试从缓存中获取
            if let Some(vfs_file) = Self::lookup_entry(&mut entries_map, name).await? {
                return Ok(vfs_file);
            }

            // 2. 若缓存中没有, 且缓存已经代表了全部项, 则直接返回 ENOENT
            if self.is_all_cached() {
                return Err(SysError::ENOENT);
            }

            // 3. 若缓存中没有, 且缓存还没有代表了全部项, 则向 fs 发出查找指令
            self.get_entries(&mut entries_map, Some(name)).await?;
            Self::lookup_entry(&mut entries_map, name).await?.ok_or(SysError::ENOENT)
        })
    }

    fn create<'a>(&'a self, name: &'a str, kind: super::VfsFileKind) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let dentry_ref = self.dir.lock().await.create(name, kind).await?;
            let mut entries_map = self.entries.lock(here!());
            Self::add_entry(&mut entries_map, dentry_ref);
            let entry = entries_map.get(name).unwrap();
            unsafe { entry.get_mut() }.active().await
        })
    }

    fn remove<'a>(&'a self, name: &'a str) -> ASysResult {
        dyn_future(async {
            let mut entries_map = self.entries.lock(here!());
            // 1. 如果缓存中有, 删除之
            if let Some((_, dentry_cache)) = entries_map.remove_entry(name) {
                let dentry_ref = unsafe { dentry_cache.get() }.get_dentry_ref().clone();
                self.dir.lock().await.remove(dentry_ref).await?;
                return Ok(());
            }

            // 2. 如果缓存中没有, 若缓存已经代表了全部项, 则直接返回 ENOENT
            if self.is_all_cached() {
                return Err(SysError::ENOENT);
            }

            // 3. 否则向 fs 查询, 若有则删除之, 若无则返回 ENOENT
            self.get_entries(&mut entries_map, Some(name)).await?;
            if let Some((_, dentry_cache)) = entries_map.remove_entry(name) {
                let dentry_ref = unsafe { dentry_cache.get() }.get_dentry_ref().clone();
                self.dir.lock().await.remove(dentry_ref).await?;
                Ok(())
            } else {
                Err(SysError::ENOENT)
            }
        })
    }

    fn detach<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async {
            let mut entries_map = self.entries.lock(here!());
            // 1. 如果缓存中有, 删除或反挂载之
            if let Some((_, dentry_cache)) = entries_map.remove_entry(name) {
                let dentry_cache = unsafe { dentry_cache.get_mut() };
                
                let (is_internal, vfs_file) = dentry_cache.unshadow().await?;
                log::debug!("AAAAAAAAAAA: is_internal: {}, name: {}", is_internal, name.to_string());
                if is_internal {
                    let dentry_ref = dentry_cache.get_dentry_ref().clone();
                    self.dir.lock().await.detach(dentry_ref).await?;
                }
                return Ok(vfs_file);
            }

            // 2. 如果缓存中没有, 若缓存已经代表了全部项, 则直接返回 ENOENT
            if self.is_all_cached() {
                return Err(SysError::ENOENT);
            }

            // 3. 否则向 fs 查询, 若有则删除之, 若无则返回 ENOENT
            self.get_entries(&mut entries_map, Some(name)).await?;
            if let Some((_, dentry_cache)) = entries_map.remove_entry(name) {
                let dentry_cache = unsafe { dentry_cache.get_mut() };
                let vfs_file = dentry_cache.active().await?;
                let dentry_ref = dentry_cache.get_dentry_ref().clone();
                self.dir.lock().await.detach(dentry_ref).await?;
                Ok(vfs_file)
            } else {
                Err(SysError::ENOENT)
            }
        })
    }

    fn attach<'a>(&'a self, name: &'a str, file: VfsFileRef) -> ASysResult {
        dyn_future(async {
            log::debug!("BBBBBBBBBBB: name: {}", name.to_string());
            let mut entries_map = self.entries.lock(here!());
            // 1. 如果缓存中有, 则检查名字是否是代表一个目录并替换之
            if let Some(dentry_cache) = entries_map.get(name) {
                let dentry_cache = unsafe { dentry_cache.get_mut() };
                return dentry_cache.shadow(file).await
            }

            // 2. 如果缓存中没有, 则向 fs 查询, 若有则替换之, 若无则返回 ENOENT
            self.get_entries(&mut entries_map, Some(name)).await?;
            if let Some(dentry_cache) = entries_map.get(name) {
                let dentry_cache = unsafe { dentry_cache.get_mut() };
                dentry_cache.shadow(file).await
            } else {
                Err(SysError::ENOENT)
            }
        })
    }
}