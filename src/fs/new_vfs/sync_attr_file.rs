use super::{
    top::{DeviceInfo, SizeInfo, TimeInfo},
    underlying::ConcreteFile,
    VfsFileKind,
};
use crate::{
    executor::block_on,
    sync::{SleepLock, SleepLockFuture},
    tools::errors::SysResult,
};
use alloc::{string::String, vec::Vec};
use core::sync::atomic::AtomicBool;

struct AttrInfo {
    kind: VfsFileKind,
    device: DeviceInfo,
    size: SizeInfo,
    time: TimeInfo,
}

pub struct SyncAttrFile<F: ConcreteFile> {
    is_deleted: AtomicBool,
    file: SleepLock<F>,
    kind: VfsFileKind,
    device: DeviceInfo,
}

impl<F: ConcreteFile> SyncAttrFile<F> {
    pub fn new(file: F) -> Self {
        Self {
            is_deleted: AtomicBool::new(false),
            file: SleepLock::new(file),
            kind: file.attr_kind(),
            device: file.attr_device(),
        }
    }

    pub fn lock(&self) -> SleepLockFuture<F> {
        self.file.lock()
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
    pub fn attr_kind(&self) -> VfsFileKind {
        self.kind
    }
    pub fn attr_device(&self) -> DeviceInfo {
        self.device
    }
    pub async fn attr_size(&self) -> SysResult<SizeInfo> {
        self.lock().await.attr_size().await
    }
    pub async fn attr_time(&self) -> SysResult<TimeInfo> {
        self.lock().await.attr_time().await
    }
    pub async fn attr_set_size(&self, info: SizeInfo) -> SysResult {
        self.lock().await.attr_set_size(info).await
    }
    pub async fn attr_set_time(&self, info: TimeInfo) -> SysResult {
        self.lock().await.attr_set_time(info).await
    }

    pub async fn delete(&self) -> SysResult {
        self.lock().await.delete().await
    }

    // 文件操作
    pub async fn read_page_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> SysResult<usize> {
        self.lock().await.read_page_at(offset, buf).await
    }
    pub async fn write_page_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> SysResult<usize> {
        self.lock().await.write_page_at(offset, buf).await
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
            block_on(async { self.lock().await.delete().await }).unwrap();
        }
    }
}
