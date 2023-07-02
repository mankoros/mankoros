use super::{underlying::ConcreteFile, top::{VfsFile, MmapKind}, sync_attr_cache::SyncAttrCacheFile};
use crate::{impl_vfs_default_non_dir, tools::errors::{dyn_future, SysError, ASysResult}, memory::{frame::alloc_frame, address::PhysAddr4K}};

pub struct SyncPageCacheFile<F: ConcreteFile> {
    mgr: PageManager,
    file: SyncAttrCacheFile<F>,
}

struct PageManager {

}

impl PageManager {
    pub fn new() -> Self {
        Self {}
    }
}

impl<F: ConcreteFile> SyncPageCacheFile<F> {
    pub fn new(file: SyncAttrCacheFile<F>) -> Self {
        Self {
            mgr: PageManager::new(),
            file,
        }
    }
}

impl<F: ConcreteFile> VfsFile for SyncPageCacheFile<F> {
    fn attr(&self) -> ASysResult<super::VfsFileAttr> {
        dyn_future(async {
            Ok(self.file.with_attr_read(|attr| attr.clone()))
        })  
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        // TODO: 页缓存
        dyn_future(async move {
            self.file.lock().await.read_at(offset, buf).await
        })
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        // TODO: 页缓存
        dyn_future(async move {
            self.file.lock().await.write_at(offset, buf).await
        })
    }

    fn get_page(&self, offset: usize, _kind: MmapKind) -> ASysResult<PhysAddr4K> {
        // TODO: 页缓存
        dyn_future(async move {
            let page = alloc_frame().ok_or(SysError::ENOMEM)?;
            let slice = unsafe { page.as_mut_page_slice() };
            self.read_at(offset, slice).await?;
            Ok(page)
        })
    }

    impl_vfs_default_non_dir!(SyncPageCacheFile);
}