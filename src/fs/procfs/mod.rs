use super::new_vfs::{
    mount::GlobalMountManager,
    top::{
        DeviceInfo, MmapKind, PollKind, SizeInfo, TimeInfo, TimeInfoChange, VfsFS, VfsFSAttr,
        VfsFSKind, VfsFile, VfsFileRef,
    },
    DeviceIDCollection, VfsFileAttr, VfsFileKind,
};
use crate::{
    executor::hart_local::get_curr_lproc,
    impl_vfs_default_non_dir, impl_vfs_default_non_file,
    memory::address::PhysAddr4K,
    process::{lproc::LightProcess, lproc_mgr::GlobalLProcManager, pid::Pid},
    tools::errors::{dyn_future, ASysResult, SysError},
};
use alloc::{
    boxed::Box,
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

pub mod interrupts;

impl VfsFileAttr {
    pub fn new_mem_dir() -> Self {
        Self {
            kind: VfsFileKind::Directory,
            device_id: 0,
            self_device_id: 0,
            byte_size: 0,
            block_count: 0,
            access_time: 0,
            modify_time: 0,
            create_time: 0,
        }
    }
}

macro_rules! impl_proc_dir_default {
    () => {
        fn as_any(&self) -> &dyn core::any::Any {
            self
        }
        fn attr_kind(&self) -> VfsFileKind {
            VfsFileKind::Directory
        }
        fn attr_device(&self) -> DeviceInfo {
            DeviceInfo {
                device_id: DeviceIDCollection::PROC_FS_ID,
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

impl Pid {
    pub fn to_string(&self) -> String {
        Into::<usize>::into(*self).to_string()
    }
}

pub struct ProcFS;

impl VfsFS for ProcFS {
    fn root(&self) -> VfsFileRef {
        VfsFileRef::new(ProcFSRootDir)
    }
    fn attr(&self) -> VfsFSAttr {
        VfsFSAttr::default_mem(VfsFSKind::Proc, DeviceIDCollection::PROC_FS_ID)
    }
}

pub struct ProcFSRootDir;

impl ProcFSRootDir {
    fn create_mounts(&self) -> VfsFileRef {
        VfsFileRef::new(ProcFSStandaloneFile {
            kind: VfsFileKind::RegularFile,
            f: || {
                let mut content = String::with_capacity(150);
                for (path, fs) in GlobalMountManager::list() {
                    let kind = fs.attr().kind;

                    // TODO: real device path
                    let device_path = match kind {
                        VfsFSKind::Fat => "/dev/sda",
                        VfsFSKind::Dev => "devfs",
                        VfsFSKind::Tmp => "tmpfs",
                        VfsFSKind::Proc => "procfs",
                    };
                    content.push_str(device_path);
                    content.push_str(" ");

                    content.push_str(path.to_string().as_str());
                    content.push_str(" ");

                    let kind_str = match kind {
                        VfsFSKind::Fat => "fat32",
                        VfsFSKind::Dev => "devfs",
                        VfsFSKind::Tmp => "tmpfs",
                        VfsFSKind::Proc => "procfs",
                    };
                    content.push_str(kind_str);
                    content.push_str(" ");

                    content.push_str("rw 0 0\n");
                }
                content.as_bytes().into()
            },
        })
    }

    fn create_interrupts(&self) -> VfsFileRef {
        VfsFileRef::new(ProcFSStandaloneFile {
            kind: VfsFileKind::RegularFile,
            f: || {
                let mut content = String::with_capacity(1024);
                for (irq, cnt) in unsafe { interrupts::PROC_FS_IRQ_CNT.iter() } {
                    content.push_str(irq.to_string().as_str());
                    content.push_str(": ");

                    content.push_str(cnt.to_string().as_str());
                    content.push_str("\n");
                }
                log::warn!("proc: {content}");
                content.as_bytes().into()
            },
        })
    }
}

impl VfsFile for ProcFSRootDir {
    impl_vfs_default_non_file!(ProcFSRootDir);
    impl_proc_dir_default!();

    fn list(&self) -> ASysResult<Vec<(String, VfsFileRef)>> {
        dyn_future(async {
            let mut ret: Vec<(String, VfsFileRef)>;

            ret = GlobalLProcManager::all()
                .into_iter()
                .map(|(pid, lproc)| {
                    let pid_str = pid.to_string();
                    let file = VfsFileRef::new(ProcFSProcDir::new(lproc));
                    (pid_str, file)
                })
                .collect();
            {
                // add self
                let curr_proc = get_curr_lproc().ok_or(SysError::ENOENT)?;
                let file = VfsFileRef::new(ProcFSProcDir::new(curr_proc));
                ret.push(("self".into(), file));
            }
            {
                // add mounts
                let file = self.create_mounts();
                ret.push(("mounts".into(), file));
            }
            {
                // add interrupts
                let file = self.create_interrupts();
                ret.push(("interrupts".into(), file));
            }

            Ok(ret)
        })
    }

    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            if name == "mounts" {
                return Ok(self.create_mounts());
            } else if name == "interrupts" {
                return Ok(self.create_interrupts());
            }

            let lproc = if name == "self" {
                get_curr_lproc().ok_or(SysError::ENOENT)?
            } else {
                let pid = name.parse::<usize>().map_err(|_| SysError::ENOENT)?;
                GlobalLProcManager::get(pid.into()).ok_or(SysError::ENOENT)?
            };

            Ok(VfsFileRef::new(ProcFSProcDir::new(lproc)))
        })
    }
}

pub struct ProcFSProcDir {
    lproc: Arc<LightProcess>,
}

impl ProcFSProcDir {
    pub fn new(lproc: Arc<LightProcess>) -> Self {
        Self { lproc }
    }

    fn create_exe(&self) -> ProcFSNormalFile {
        ProcFSNormalFile {
            kind: VfsFileKind::SymbolLink,
            lproc: self.lproc.clone(),
            f: |lproc| {
                let path = lproc.with_procfs_info(|info| info.exe_path.clone()).unwrap();
                path.to_string().as_bytes().into()
            },
        }
    }
}

impl VfsFile for ProcFSProcDir {
    impl_vfs_default_non_file!(ProcFSProcDir);
    impl_proc_dir_default!();

    fn list(&self) -> ASysResult<Vec<(String, VfsFileRef)>> {
        dyn_future(async {
            let mut ret = Vec::new();
            ret.push(("exe".to_string(), VfsFileRef::new(self.create_exe())));
            Ok(ret)
        })
    }

    fn lookup<'a>(&'a self, name: &'a str) -> ASysResult<VfsFileRef> {
        dyn_future(async move {
            let file = match name {
                "exe" => self.create_exe(),
                _ => return Err(SysError::ENOENT),
            };
            Ok(VfsFileRef::new(file))
        })
    }
}

pub type GetStringInfoFn = fn(&Arc<LightProcess>) -> Box<[u8]>;

pub struct ProcFSNormalFile {
    kind: VfsFileKind,
    lproc: Arc<LightProcess>,
    f: GetStringInfoFn,
}

impl VfsFile for ProcFSNormalFile {
    impl_vfs_default_non_dir!(ProcFSFile);

    fn attr_kind(&self) -> VfsFileKind {
        self.kind
    }
    fn attr_device(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: DeviceIDCollection::PROC_FS_ID,
            self_device_id: 0,
        }
    }
    fn attr_size(&self) -> ASysResult<SizeInfo> {
        dyn_future(async {
            Ok(SizeInfo {
                bytes: (self.f)(&self.lproc).len(),
                blocks: 0,
            })
        })
    }
    fn attr_time(&self) -> ASysResult<TimeInfo> {
        dyn_future(async {
            Ok(TimeInfo {
                access: 0,
                modify: 0,
                change: 0,
            })
        })
    }

    fn update_time(&self, _info: TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            let data = (self.f)(&self.lproc);
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

pub type GetStandardaloneStringInfoFn = fn() -> Box<[u8]>;

pub struct ProcFSStandaloneFile {
    kind: VfsFileKind,
    f: GetStandardaloneStringInfoFn,
}

impl VfsFile for ProcFSStandaloneFile {
    impl_vfs_default_non_dir!(ProcFSStandaloneFile);

    fn attr_kind(&self) -> VfsFileKind {
        self.kind
    }
    fn attr_device(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: DeviceIDCollection::PROC_FS_ID,
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
        dyn_future(async {
            Ok(TimeInfo {
                access: 0,
                modify: 0,
                change: 0,
            })
        })
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
