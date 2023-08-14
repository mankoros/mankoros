use super::{underlying::ConcreteFile, VfsFileAttr, VfsFileKind};
use crate::{
    executor::block_on,
    sync::{SleepLock, SleepLockFuture},
    tools::errors::SysResult,
};
use alloc::{string::String, vec::Vec};
use core::sync::atomic::AtomicBool;

pub struct SyncAttrFile<F: ConcreteFile> {
    is_deleted: AtomicBool,
    file: SleepLock<F>,
}

impl<F: ConcreteFile> SyncAttrFile<F> {
    pub fn new(file: F) -> Self {
        Self {
            is_deleted: AtomicBool::new(false),
            file: SleepLock::new(file),
        }
    }

    pub fn lock(&self) -> SleepLockFuture<F> {
        self.file.lock()
    }

    pub async fn attr(&self) -> VfsFileAttr {
        let file = self.file.lock().await;
        let f_time = file.get_time();
        VfsFileAttr {
            kind: file.kind(),
            device_id: file.device_id(),
            self_device_id: 0,
            byte_size: file.size(),
            block_count: file.block_count(),
            access_time: f_time[0],
            modify_time: f_time[1],
            create_time: f_time[2],
        }
    }

    pub fn mark_deleted(&self) {
        self.is_deleted.store(true, core::sync::atomic::Ordering::Relaxed);
    }
    pub fn is_deleted(&self) -> bool {
        self.is_deleted.load(core::sync::atomic::Ordering::Relaxed)
    }
}

impl<F: ConcreteFile> SyncAttrFile<F> {
    // 通用操作
    pub async fn kind(&self) -> VfsFileKind {
        self.lock().await.kind()
    }
    pub async fn size(&self) -> usize {
        self.lock().await.size()
    }
    pub async fn block_count(&self) -> usize {
        self.lock().await.block_count()
    }
    pub async fn device_id(&self) -> usize {
        self.lock().await.device_id()
    }
    pub async fn delete(&self) -> SysResult {
        self.lock().await.delete().await
    }
    pub async fn get_time(&self) -> [usize; 3] {
        self.lock().await.get_time()
    }

    // 文件操作
    pub async fn read_page_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> SysResult<usize> {
        self.lock().await.read_page_at(offset, buf).await
    }
    pub async fn write_page_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> SysResult<usize> {
        self.lock().await.write_page_at(offset, buf).await
    }
    pub async fn truncate<'a>(&'a self, new_size: usize) -> SysResult {
        self.lock().await.truncate(new_size).await
    }

    // 文件夹操作
    pub async fn lookup<'a>(&'a self, name: &'a str) -> SysResult<Self> {
        self.lock().await.lookup(name).await.map(Self::new)
    }
    pub async fn list<'a>(&'a self) -> SysResult<Vec<(String, Self)>> {
        let l = self.lock().await.list().await;
        l.map(|v| v.into_iter().map(|(s, f)| (s, Self::new(f))).collect())
    }
    pub async fn create<'a>(&'a self, name: &'a str, kind: VfsFileKind) -> SysResult<Self> {
        self.lock().await.create(name, kind).await.map(Self::new)
    }
    pub async fn rename<'a>(&'a self, file: &'a Self, new_name: &'a str) -> SysResult {
        let other = file.lock().await;
        self.lock().await.rename(&other, new_name).await
    }
    pub async fn detach<'a>(&'a self, file: &'a Self) -> SysResult {
        let other = file.lock().await;
        self.lock().await.detach(&other).await
    }
    pub async fn attach<'a>(&'a self, file: &'a Self, name: &'a str) -> SysResult {
        let other = file.lock().await;
        self.lock().await.attach(&other, name).await
    }
}

impl<F: ConcreteFile> Drop for SyncAttrFile<F> {
    fn drop(&mut self) {
        if self.is_deleted() {
            block_on(self.delete()).unwrap();
        }
    }
}
