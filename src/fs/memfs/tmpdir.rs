use crate::{
    fs::new_vfs::{
        top::{DeviceInfo, SizeInfo, TimeInfo, VfsFile, VfsFileRef},
        DeviceIDCollection, VfsFileKind,
    },
    here, impl_vfs_default_non_dir, impl_vfs_default_non_file,
    sync::SpinNoIrqLock,
    timer::get_time_us,
    tools::errors::{dyn_future, ASysResult, SysError},
};
use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};

pub struct TmpFile {
    time: SpinNoIrqLock<TimeInfo>,
    content: SpinNoIrqLock<Vec<u8>>,
}

impl TmpFile {
    pub fn new() -> Self {
        Self {
            time: SpinNoIrqLock::new(TimeInfo {
                access: 0,
                modify: 0,
                change: get_time_us() * 1000,
            }),
            content: SpinNoIrqLock::new(Vec::new()),
        }
    }
}

impl VfsFile for TmpFile {
    fn attr_kind(&self) -> VfsFileKind {
        VfsFileKind::RegularFile
    }
    fn attr_device(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: DeviceIDCollection::TMP_FS_ID,
            self_device_id: 0,
        }
    }
    fn attr_size(&self) -> ASysResult<SizeInfo> {
        dyn_future(async {
            Ok(SizeInfo {
                bytes: self.content.lock(here!()).len(),
                blocks: 0,
            })
        })
    }
    fn attr_time(&self) -> ASysResult<TimeInfo> {
        dyn_future(async { Ok(self.time.lock(here!()).clone()) })
    }
    fn update_time(&self, info_change: crate::fs::new_vfs::top::TimeInfoChange) -> ASysResult {
        dyn_future(async move {
            self.time.lock(here!()).apply_change(info_change);
            Ok(())
        })
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            let content = self.content.lock(here!());
            let len = core::cmp::min(buf.len(), content.len() - offset);
            buf[..len].copy_from_slice(&content[offset..offset + len]);
            Ok(len)
        })
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            let mut content = self.content.lock(here!());

            if content.len() < offset {
                content.resize(offset, 0);
            }

            // content.len() >= offset
            let in_content_len = core::cmp::min(buf.len(), content.len() - offset);
            for i in 0..in_content_len {
                content[i + offset] = buf[i];
            }

            let out_content_len = buf.len() - in_content_len;
            for i in 0..out_content_len {
                content.push(buf[i + in_content_len]);
            }

            Ok(buf.len())
        })
    }

    fn get_page(
        &self,
        _offset: usize,
        _kind: crate::fs::new_vfs::top::MmapKind,
    ) -> ASysResult<crate::memory::address::PhysAddr4K> {
        unimplemented!("mmap for tmpfile")
    }

    fn truncate(&self, length: usize) -> ASysResult {
        dyn_future(async move {
            self.content.lock(here!()).resize(length, 0);
            Ok(())
        })
    }

    fn poll_ready(
        &self,
        _offset: usize,
        _len: usize,
        _kind: crate::fs::new_vfs::top::PollKind,
    ) -> ASysResult<usize> {
        unimplemented!("poll for tmpfile")
    }

    fn poll_read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
        unimplemented!("poll for tmpfile")
    }

    fn poll_write(&self, _offset: usize, _buf: &[u8]) -> usize {
        unimplemented!("poll for tmpfile")
    }

    impl_vfs_default_non_dir!(TmpFile);

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}

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

    fn attr_kind(&self) -> VfsFileKind {
        VfsFileKind::Directory
    }
    fn attr_device(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: DeviceIDCollection::TMP_FS_ID,
            self_device_id: 0,
        }
    }
    fn attr_size(&self) -> ASysResult<SizeInfo> {
        dyn_future(async { Ok(SizeInfo::new_zero()) })
    }
    fn attr_time(&self) -> ASysResult<TimeInfo> {
        dyn_future(async { Ok(TimeInfo::new_zero()) })
    }
    fn update_time(&self, _info: crate::fs::new_vfs::top::TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn create<'a>(&'a self, name: &'a str, kind: VfsFileKind) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let mut children = self.children.lock(here!());

            if children.contains_key(name) {
                return Err(SysError::EEXIST);
            }

            let new_file = match kind {
                VfsFileKind::Directory => VfsFileRef::new(Self::new()),
                VfsFileKind::RegularFile => VfsFileRef::new(TmpFile::new()),
                _ => panic!("unknown kind"),
            };

            let ret = children.insert(name.to_string(), new_file.clone());
            debug_assert!(ret.is_none());

            Ok(new_file)
        })
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
