use super::new_vfs::{
    top::{
        DeviceInfo, MmapKind, PollKind, SizeInfo, TimeInfo, TimeInfoChange, VfsFS, VfsFSAttr,
        VfsFSKind, VfsFile, VfsFileRef,
    },
    DeviceIDCollection, VfsFileKind,
};
use crate::{
    here, impl_vfs_default_non_dir, impl_vfs_default_non_file,
    memory::address::PhysAddr4K,
    sync::SpinNoIrqLock,
    tools::errors::{dyn_future, ASysResult, SysError},
};
use alloc::{
    boxed::Box,
    format,
    string::{String, ToString},
    vec::Vec,
};

static mut FUNC_CALLS: SpinNoIrqLock<Vec<usize>> = SpinNoIrqLock::new(Vec::new());
pub fn add_func_call(addr: usize) {
    unsafe {
        FUNC_CALLS.lock(here!()).push(addr);
    }
}

macro_rules! impl_debug_dir_default {
    () => {
        fn as_any(&self) -> &dyn core::any::Any {
            self
        }
        fn attr_kind(&self) -> VfsFileKind {
            VfsFileKind::Directory
        }
        fn attr_device(&self) -> DeviceInfo {
            DeviceInfo {
                device_id: DeviceIDCollection::DEBUG_FS_ID,
                self_device_id: 0,
            }
        }
        fn attr_size(&self) -> ASysResult<SizeInfo> {
            dyn_future(async {
                Ok(SizeInfo {
                    bytes: 0,
                    blocks: 0,
                })
            })
        }
        fn attr_time(&self) -> ASysResult<TimeInfo> {
            dyn_future(async { Ok(TimeInfo::new_zero()) })
        }
        fn update_time(&self, _info: TimeInfoChange) -> ASysResult {
            todo!()
        }

        fn create<'a>(&'a self, _name: &'a str, _kind: VfsFileKind) -> ASysResult<VfsFileRef> {
            dyn_future(async { Err(SysError::EPERM) })
        }
        fn remove<'a>(&'a self, _name: &'a str) -> ASysResult {
            dyn_future(async { Err(SysError::EPERM) })
        }
        fn detach<'a>(&'a self, _name: &'a str) -> ASysResult<VfsFileRef> {
            dyn_future(async { Err(SysError::EPERM) })
        }
        fn attach<'a>(&'a self, _name: &'a str, _file: VfsFileRef) -> ASysResult {
            dyn_future(async { Err(SysError::EPERM) })
        }
    };
}

pub struct DebugFS;

impl VfsFS for DebugFS {
    fn root(&self) -> VfsFileRef {
        VfsFileRef::new(DebugFSRootDir)
    }
    fn attr(&self) -> VfsFSAttr {
        VfsFSAttr::default_mem(VfsFSKind::Proc, DeviceIDCollection::DEBUG_FS_ID)
    }
}

pub struct DebugFSRootDir;

impl DebugFSRootDir {
    async fn create_kcov(&self) -> VfsFileRef {
        VfsFileRef::new(DebugFSStandaloneFile {
            kind: VfsFileKind::RegularFile,
            f: || {
                let func_calls = unsafe { FUNC_CALLS.lock(here!()) };
                func_calls
                    .iter()
                    .map(|x| format!("0x{:x}", x))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .into_bytes()
                    .into_boxed_slice()
            },
        })
    }
}

impl VfsFile for DebugFSRootDir {
    impl_vfs_default_non_file!(ProcFSRootDir);
    impl_debug_dir_default!();

    fn list(&self) -> ASysResult<Vec<(String, VfsFileRef)>> {
        dyn_future(async {
            let mut ret: Vec<(String, VfsFileRef)> = Vec::new();
            ret.push(("kcov".to_string(), self.create_kcov().await));
            Ok(ret)
        })
    }

    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            if name == "kcov" {
                Ok(self.create_kcov().await)
            } else {
                Err(SysError::ENOENT)
            }
        })
    }
}

pub type GetStandardaloneStringInfoFn = fn() -> Box<[u8]>;

pub struct DebugFSStandaloneFile {
    kind: VfsFileKind,
    f: GetStandardaloneStringInfoFn,
}

impl VfsFile for DebugFSStandaloneFile {
    impl_vfs_default_non_dir!(ProcFSStandaloneFile);

    fn attr_kind(&self) -> VfsFileKind {
        self.kind
    }
    fn attr_device(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: DeviceIDCollection::DEBUG_FS_ID,
            self_device_id: 0,
        }
    }
    fn attr_size(&self) -> ASysResult<SizeInfo> {
        dyn_future(async {
            Ok(SizeInfo {
                bytes: (self.f)().len(),
                blocks: 0,
            })
        })
    }
    fn attr_time(&self) -> ASysResult<TimeInfo> {
        dyn_future(async { Ok(TimeInfo::new_zero()) })
    }

    fn update_time(&self, _info: TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            let data = (self.f)();
            let len = core::cmp::min(data.len() - offset, buf.len());
            buf[..len].copy_from_slice(&data[offset..offset + len]);
            Ok(len)
        })
    }

    fn write_at<'a>(&'a self, _offset: usize, _buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async { Err(SysError::EPERM) })
    }
    fn truncate(&self, _length: usize) -> ASysResult {
        dyn_future(async { Err(SysError::EPERM) })
    }

    fn get_page(&self, _offset: usize, _kind: MmapKind) -> ASysResult<PhysAddr4K> {
        todo!()
    }
    fn poll_ready(&self, _offset: usize, _len: usize, _kind: PollKind) -> ASysResult<usize> {
        todo!()
    }
    fn poll_read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
        todo!()
    }
    fn poll_write(&self, _offset: usize, _buf: &[u8]) -> usize {
        todo!()
    }
    fn as_any(&self) -> &dyn core::any::Any {
        todo!()
    }
}
