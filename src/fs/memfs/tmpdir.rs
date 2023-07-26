use crate::{
    fs::new_vfs::{
        top::{VfsFile, VfsFileRef},
        DeviceIDCollection, VfsFileAttr, VfsFileKind,
    },
    here, impl_vfs_default_non_file,
    sync::SpinNoIrqLock,
    tools::errors::{dyn_future, ASysResult, SysError},
};
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

pub struct TmpDir {
    children: SpinNoIrqLock<BTreeMap<String, VfsFileRef>>,
}

impl TmpDir {
    pub fn new() -> Self {
        Self {
            children: SpinNoIrqLock::new(BTreeMap::new()),
        }
    }
}

impl VfsFile for TmpDir {
    impl_vfs_default_non_file!(TmpDir);

    fn attr(&self) -> ASysResult<VfsFileAttr> {
        dyn_future(async {
            Ok(VfsFileAttr {
                kind: VfsFileKind::Directory,
                device_id: DeviceIDCollection::TMP_FS_ID,
                self_device_id: 0,
                byte_size: 0,
                block_count: 0,
                access_time: 0,
                modify_time: 0,
                create_time: 0,
            })
        })
    }

    fn create<'a>(&'a self, _name: &'a str, _kind: VfsFileKind) -> ASysResult<VfsFileRef> {
        unimplemented!("TmpDir::create")
    }

    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let children = self.children.lock(here!());
            match children.get(name) {
                Some(file) => Ok(file.clone()),
                None => Err(SysError::ENOENT),
            }
        })
    }

    fn detach<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let mut children = self.children.lock(here!());
            match children.remove(name) {
                Some(file) => Ok(file),
                None => Err(SysError::ENOENT),
            }
        })
    }

    fn attach<'a>(&'a self, name: &'a str, file: VfsFileRef) -> ASysResult {
        dyn_future(async move {
            let mut children = self.children.lock(here!());
            match children.insert(name.to_string(), file) {
                Some(_) => Err(SysError::EEXIST),
                None => Ok(()),
            }
        })
    }

    fn remove<'a>(&'a self, name: &'a str) -> ASysResult {
        dyn_future(async { self.detach(name).await.map(|_| ()) })
    }

    fn list(&self) -> ASysResult<Vec<(String, VfsFileRef)>> {
        dyn_future(async move {
            let children = self.children.lock(here!());
            let mut ret = Vec::new();
            for (name, file) in children.iter() {
                ret.push((name.clone(), file.clone()));
            }
            Ok(ret)
        })
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
