use super::{underlying::ConcreteFile, VfsFileAttr};
use crate::{
    sync::{SleepLock, SleepLockFuture},
};

pub struct SyncAttrFile<F: ConcreteFile> {
    file: SleepLock<F>,
}

impl<F: ConcreteFile> SyncAttrFile<F> {
    pub fn new(file: F) -> Self {
        Self {
            file: SleepLock::new(file),
        }
    }

    pub fn lock(&self) -> SleepLockFuture<F> {
        self.file.lock()
    }

    pub async fn attr(&self) -> VfsFileAttr {
        let file = self.file.lock().await;
        VfsFileAttr {
            kind: file.kind(),
            device_id: file.device_id(),
            self_device_id: 0,
            byte_size: file.size(),
            block_count: file.block_count(),
            access_time: 0,
            modify_time: 0,
            create_time: 0,
        }
    }
}
