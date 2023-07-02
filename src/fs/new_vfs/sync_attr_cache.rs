use crate::{sync::{SleepLock, SpinNoIrqLock, SleepLockFuture}, here};
use super::{underlying::{ConcreteFile, DEntryRef}, VfsFileAttr, VfsFileKind};
use core::{sync::atomic::{AtomicBool, Ordering}};

pub struct SyncAttrCacheFile<F: ConcreteFile> {
    file: SleepLock<F>,
    // 属性是至关重要的, 不管什么文件都可能会用到属性, 所以不懒加载也行
    attr: SpinNoIrqLock<VfsFileAttr>,
    attr_dirty: AtomicBool,
}

impl<F: ConcreteFile> SyncAttrCacheFile<F> {
    pub fn new_direct(file: F, attr: VfsFileAttr) -> Self {
        Self {
            file: SleepLock::new(file),
            attr: SpinNoIrqLock::new(attr),
            attr_dirty: AtomicBool::new(false),
        }
    }

    pub fn new(dentry_ref: &F::DEntryRefT) -> Self {
        Self::new_direct(dentry_ref.file(), dentry_ref.attr())
    }

    pub fn kind(&self) -> VfsFileKind {
        self.attr.lock(here!()).kind
    }

    pub fn attr_clone(&self) -> VfsFileAttr {
        self.attr.lock(here!()).clone()
    }

    pub fn with_attr_read<T>(&self, f: impl FnOnce(&VfsFileAttr) -> T) -> T {
        f(&self.attr.lock(here!()))
    }

    pub fn with_attr_write<T>(&self, f: impl FnOnce(&mut VfsFileAttr) -> T) -> T {
        self.attr_dirty.store(true, Ordering::Relaxed);
        f(&mut self.attr.lock(here!()))
    }

    pub fn is_dirty(&self) -> bool {
        self.attr_dirty.load(Ordering::Relaxed)
    }

    pub fn lock(&self) -> SleepLockFuture<F> {
        self.file.lock()
    }
}

// impl<F: ConcreteFile> VfsFile for SyncAttrCacheFile<F> {
//     fn attr(&self) -> crate::tools::errors::ASysResult<VfsFileAttr> {
//         dyn_future(async {
//             Ok(self.with_attr_read(|&a| a.clone()))  
//         })
//     }

//     fn read_at(&self, offset: usize, buf: &mut [u8]) -> crate::tools::errors::ASysResult<usize> {
//         dyn_future(async {
//             self.lock().await.read_at(offset, buf).await  
//         })
//     }

//     fn write_at(&self, offset: usize, buf: &[u8]) -> crate::tools::errors::ASysResult<usize> {
//         dyn_future(async {
//             self.lock().await.write_at(offset, buf).await  
//         })
//     }

//     fn get_page(&self, offset: usize, kind: super::top::MmapKind) -> crate::tools::errors::ASysResult<crate::memory::address::PhysAddr4K> {
//         match kind {
//             super::top::MmapKind::Private => {
//                 dyn_future(async {
//                     let frame = alloc_frame().ok_or(SysError::ENOMEM)?;
//                     let slice = unsafe { frame.as_mut_page_slice() };
//                     self.read_at(offset, slice).await?;
//                     Ok(frame)
//                 })
//             },
//             super::top::MmapKind::Shared => todo!(),
//         }
//     }

//     fn list(&self) -> crate::tools::errors::ASysResult<alloc::vec::Vec<(alloc::string::String, super::top::VfsFileRef)>> {
//         dyn_future(async {
//             let (is_all, entries) = self.lock().await.lookup_batch(0, None).await?;
//             debug_assert!(is_all);

//             let mut ret = Vec::new();
//             for entry in entries {
//                 let name = entry.name().to_string();
//                 let file = Self::new(entry).await?;
//                 let file_ref: VfsFileRef = Arc::new(file);
//                 ret.push((name, file_ref));
//             }
//             Ok(ret)
//         })
//     }

//     fn lookup(&self, name: &str) -> crate::tools::errors::ASysResult<super::top::VfsFileRef> {
//         dyn_future(async {
//             let (_, entries) = self.lock().await.lookup_batch(0, Some(name)).await?;
//             let entry = *entries.last().ok_or(SysError::ENOENT)?;
//             let file = Self::new(entry).await?;
//             let file_ref: VfsFileRef = Arc::new(file);
//             Ok(file_ref)
//         })
//     }

//     fn create(&self, name: &str, kind: VfsFileKind) -> crate::tools::errors::ASysResult<super::top::VfsFileRef> {
//         dyn_future(async {
//             let file = self.lock().await.create(name, kind).await?;
//             let file = Self::new(file).await?;
//             let file_ref: VfsFileRef = Arc::new(file);
//             Ok(file_ref)
//         })
//     }

//     fn remove(&self, name: &str) -> crate::tools::errors::ASysResult {
//         dyn_future(async {
//             self.lock().await.remove(name).await  
//         })
//     }

//     fn detach(&self, name: &str) -> crate::tools::errors::ASysResult<super::top::VfsFileRef> {
//         todo!()
//     }

//     fn attach(&self, name: &str, file: super::top::VfsFileRef) -> crate::tools::errors::ASysResult {
//         todo!()
//     }
// } 