use core::{cell::SyncUnsafeCell};
use alloc::{ sync::Weak, vec::Vec, sync::Arc, collections::BTreeMap, string::{String, ToString}};
use crate::{sync::{SpinNoIrqLock}, here, tools::errors::{SysResult, SysError}};
use super::inode::VfsNode;
use core::sync::atomic::{AtomicBool};

struct DirEntry {
    name: String,
    parent: Option<Weak<DirEntry>>,
    cache: SpinNoIrqLock<DirEntryCacheStatus>,
    // inode 的所有方法都是线程安全的, 不需要上锁
    inode: Arc<VfsNode>,

    // 自己是 dirty 说明父母要写回
    dirty: AtomicBool,
}

impl Drop for DirEntry {
    fn drop(&mut self) {
        panic!("DirEntry should never be auto-dropped, it need a async write-back")
    }
}

enum DirEntryCacheStatus {
    Uninit,
    File,
    DirUncached,
    Dir(SyncUnsafeCell<BTreeMap<String, Arc<DirEntry>>>),
}

macro_rules! lock_and_acquire_children_cache {
    ($dentry:expr, $name:ident) => {
        $dentry.clone().check_cache().await?;
        let __guard = $dentry.cache.lock(here!());
        let $name = match *__guard {
            DirEntryCacheStatus::Dir(ref children) => unsafe{ &mut *children.get() },
            _ => unreachable!(),
        };
    };
}

impl DirEntry {
    fn new(name: &str, parent: &Arc<DirEntry>, inode: Arc<VfsNode>, is_dir: Option<bool>) -> Arc<Self> {
        let cache = match is_dir {
            Some(is_dir) => if is_dir {
                DirEntryCacheStatus::DirUncached
            } else {
                DirEntryCacheStatus::File
            },
            None => DirEntryCacheStatus::Uninit
        };

        Arc::new(Self {
            name: name.to_string(),
            parent: Some(Arc::downgrade(parent)),
            cache: SpinNoIrqLock::new(cache),
            inode,
            dirty: AtomicBool::new(true),
        })
    }

    async fn read_dirs_from_inode(self: Arc<Self>) -> SysResult<BTreeMap<String, Arc<DirEntry>>> {
        let mut children = BTreeMap::new();
        for name in self.inode.list().await? {
            let inode = self.inode.lookup(&name).await?;
            let child = DirEntry::new(&name, &self, Arc::new(inode), None);
            children.insert(name, child);
        }
        Ok(children)
    }

    async fn check_cache(self: Arc<Self>) -> SysResult<()> {
        let mut cache = self.cache.lock(here!());

        // likely way
        if let DirEntryCacheStatus::File | DirEntryCacheStatus::Dir(_) = *cache {
            return Ok(())
        }
        
        if let DirEntryCacheStatus::Uninit = *cache {
            let is_dir = self.inode.stat().await?.kind().is_dir();
            *cache = if is_dir {
                DirEntryCacheStatus::DirUncached
            } else {
                DirEntryCacheStatus::File
            };
        }
        
        if let DirEntryCacheStatus::DirUncached = *cache {
            let children = self.clone().read_dirs_from_inode().await?;
            *cache = DirEntryCacheStatus::Dir(SyncUnsafeCell::new(children));
        }

        Ok(())
    }

    async fn list(self: Arc<Self>) -> SysResult<Vec<String>> {
        lock_and_acquire_children_cache!(self, children);
        Ok(children.keys().map(String::clone).collect())
    }

    async fn lookup(self: Arc<Self>, name: &str) -> SysResult<Arc<DirEntry>> {
        lock_and_acquire_children_cache!(self, children);
        children.get(name).cloned().ok_or(SysError::ENOENT)
    }

    async fn create(self: Arc<Self>, name: &str, is_dir: bool) -> SysResult<Arc<DirEntry>> {
        lock_and_acquire_children_cache!(self, children);

        if children.contains_key(name) {
            return Err(SysError::EEXIST)
        }

        // Create must write-back immediately in order to get a valid inode
        let inode = self.inode.create(name, is_dir).await?;
        let new_entry = DirEntry::new(name, &self, Arc::new(inode), Some(is_dir));

        children.insert(name.to_string(), new_entry.clone());
        Ok(new_entry)
    }

    async fn link(self: Arc<Self>, name: &str, inode: Arc<VfsNode>) -> SysResult<Arc<DirEntry>> {
        lock_and_acquire_children_cache!(self, children);
        
        if children.contains_key(name) {
            return Err(SysError::EEXIST)
        }

        let new_entry = DirEntry::new(name, &self, inode, None);
        
        children.insert(name.to_string(), new_entry.clone());
        Ok(new_entry)
    }

    async fn unlink(self: Arc<Self>, name: &str) -> SysResult<()> {
        lock_and_acquire_children_cache!(self, children);
        children.remove(name).map(|_| ()).ok_or(SysError::ENOENT)
    }

    async fn sync_with_inode(&self) {
        todo!("find a way that can directly overwrite all data in inode")
    }
}

