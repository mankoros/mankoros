use super::{
    page_cache::PageCacheFile,
    sync_attr_file::SyncAttrFile,
    top::{VfsFile, VfsFileRef},
    underlying::ConcreteFile,
    VfsFileKind,
};
use crate::{
    here, impl_vfs_default_non_file,
    sync::SpinNoIrqLock,
    tools::errors::{dyn_future, ASysResult, SysError, SysResult},
};
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    sync::{Arc, Weak},
    vec::Vec,
};
use futures::FutureExt;

pub struct PathCacheDir<F: ConcreteFile> {
    file: SyncAttrFile<F>,
    name: String,
    subdirs: SpinNoIrqLock<SubdirMap>,
}

struct SubdirMap {
    is_all: bool,
    map: BTreeMap<String, VfsFileRef>,
}

impl SubdirMap {
    pub const fn new() -> Self {
        Self {
            is_all: false,
            map: BTreeMap::new(),
        }
    }

    pub fn is_all(&self) -> bool {
        self.is_all
    }
    pub fn set_all(&mut self) {
        self.is_all = true;
    }

    pub fn all(&self) -> Vec<(String, VfsFileRef)> {
        self.map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }
    pub fn exist(&self, name: &str) -> bool {
        self.map.contains_key(name)
    }
    pub fn get(&self, name: &str) -> Option<VfsFileRef> {
        self.map.get(name).cloned()
    }
    pub fn pop(&mut self, name: &str) -> Option<VfsFileRef> {
        self.map.remove(name)
    }
    pub fn put(&mut self, name: String, file: VfsFileRef) {
        self.map.insert(name, file);
    }
}

impl<F: ConcreteFile> PathCacheDir<F> {
    pub fn new_root(file: SyncAttrFile<F>) -> Self {
        Self {
            file,
            name: String::from(""),
            subdirs: SpinNoIrqLock::new(SubdirMap::new()),
        }
    }

    fn new_sub(name: &str, file: SyncAttrFile<F>) -> Self {
        Self {
            file,
            name: name.to_string(),
            subdirs: SpinNoIrqLock::new(SubdirMap::new()),
        }
    }

    fn pack_concrete_file(&self, name: &str, file: F) -> VfsFileRef {
        let kind = file.kind();
        let file = SyncAttrFile::new(file);
        match kind {
            VfsFileKind::Directory => VfsFileRef::new(Self::new_sub(name, file)),
            VfsFileKind::RegularFile => VfsFileRef::new(PageCacheFile::new(file)),
            _ => panic!("unsupported file kind from concrete file"),
        }
    }

    async fn pack_file(&self, name: &str, file: SyncAttrFile<F>) -> VfsFileRef {
        let kind = file.kind().await;
        match kind {
            VfsFileKind::Directory => VfsFileRef::new(Self::new_sub(name, file)),
            VfsFileKind::RegularFile => VfsFileRef::new(PageCacheFile::new(file)),
            _ => panic!("unsupported file kind from concrete file"),
        }
    }

    async fn extract_file<'a>(&self, file: &'a VfsFileRef) -> Option<&'a SyncAttrFile<F>> {
        let file_dev_id = file.attr().await.ok()?.device_id;
        let self_dev_id = self.file.device_id().await;
        if file_dev_id == self_dev_id {
            let kind = file.attr().await.ok()?.kind;
            match kind {
                VfsFileKind::Directory => {
                    let file = file.as_any().downcast_ref::<Self>().unwrap();
                    Some(&file.file)
                }
                VfsFileKind::RegularFile => {
                    let file = file.as_any().downcast_ref::<PageCacheFile<F>>().unwrap();
                    Some(&file.file)
                }
                _ => panic!("unsupported file kind from concrete file"),
            }
        } else {
            None
        }
    }
}

impl<F: ConcreteFile> VfsFile for PathCacheDir<F> {
    fn attr(&self) -> ASysResult<super::VfsFileAttr> {
        dyn_future(async move { Ok(self.file.attr().await) })
    }

    impl_vfs_default_non_file!(PathCacheDir);

    fn list(&self) -> ASysResult<alloc::vec::Vec<(String, VfsFileRef)>> {
        dyn_future(async move {
            let mut subdirs = self.subdirs.lock(here!());
            if !subdirs.is_all() {
                let l = self.file.lock().await.list().await?;
                for (name, file) in l {
                    subdirs.put(name.clone(), self.pack_concrete_file(&name, file));
                }
                subdirs.set_all();
            }
            Ok(subdirs.all())
        })
    }

    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let mut subdirs = self.subdirs.lock(here!());
            if let Some(file) = subdirs.get(name) {
                // 如果有缓存，直接返回
                Ok(file)
            } else {
                if subdirs.is_all() {
                    // 若缓存已经代表了全部文件, 则可以直接返回 ENOENT
                    Err(SysError::ENOENT)
                } else {
                    // 若否, 则向具体文件系统查找, 如果找到了就缓存并返回
                    self.file.lock().await.lookup(name).await.map(|file| {
                        let file = self.pack_concrete_file(name, file);
                        subdirs.put(name.to_string(), file.clone());
                        file
                    })
                }
            }
        })
    }

    fn create<'a>(&'a self, name: &'a str, kind: super::VfsFileKind) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let mut subdirs = self.subdirs.lock(here!());
            if subdirs.exist(name) {
                Err(SysError::EEXIST)
            } else {
                let file = self.file.lock().await.create(name, kind).await?;
                let file = self.pack_concrete_file(name, file);
                subdirs.put(name.to_string(), file.clone());
                Ok(file)
            }
        })
    }

    fn remove<'a>(&'a self, name: &'a str) -> ASysResult {
        dyn_future(async move {
            let mut subdirs = self.subdirs.lock(here!());
            if let Some(file) = subdirs.pop(name) {
                if let Some(file) = self.extract_file(&file).await {
                    // 如果是本文件系统中的文件, 则从具体文件系统中删除
                    self.file.detach(file).await?;
                    // 同时延迟删
                    file.mark_deleted();
                }
                // 否则什么也不做
            } else {
                if subdirs.is_all() {
                    // 若缓存已经代表了全部文件, 则可以直接返回 ENOENT
                    return Err(SysError::ENOENT);
                } else {
                    // 若否, 则向具体文件系统查找并要求删除
                    let file = self.file.lookup(name).await?;
                    self.file.detach(&file).await?;
                    file.mark_deleted();
                }
            }
            Ok(())
        })
    }

    fn detach<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let mut subdirs = self.subdirs.lock(here!());
            if let Some(file) = subdirs.pop(name) {
                if let Some(file) = self.extract_file(&file).await {
                    // 如果是本文件系统中的文件, 则从具体文件系统中删除
                    self.file.detach(file).await?;
                }
                // 否则什么也不做
                Ok(file)
            } else {
                if subdirs.is_all() {
                    // 若缓存已经代表了全部文件, 则可以直接返回 ENOENT
                    Err(SysError::ENOENT)
                } else {
                    // 若否, 则向具体文件系统查找并要求删除
                    let file = self.file.lookup(name).await?;
                    self.file.detach(&file).await?;
                    Ok(self.pack_file(name, file).await)
                }
            }
        })
    }

    fn attach<'a>(&'a self, name: &'a str, file: VfsFileRef) -> ASysResult {
        dyn_future(async move {
            let mut subdirs = self.subdirs.lock(here!());
            if subdirs.exist(name) {
                Err(SysError::EEXIST)
            } else {
                if let Some(file) = self.extract_file(&file).await {
                    // 如果是本文件系统中的文件, 则向具体文件系统中加入它
                    self.file.attach(file, name).await?;
                }
                // 否则什么也不做
                subdirs.put(name.to_string(), file);
                Ok(())
            }
        })
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
