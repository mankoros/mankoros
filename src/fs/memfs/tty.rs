use crate::{
    executor::hart_local::get_curr_lproc,
    fs::{
        new_vfs::{
            top::{DeviceInfo, IOCTLCmd, MmapKind, PollKind, SizeInfo, TimeInfo, VfsFile},
            DeviceIDCollection, VfsFileKind,
        },
        stdio::{Stdin, Stdout},
    },
    impl_vfs_default_non_dir,
    memory::{address::PhysAddr4K, UserReadPtr, UserWritePtr},
    tools::errors::{dyn_future, ASysResult},
};
use core::sync::atomic::AtomicUsize;

impl IOCTLCmd {
    pub const TIOCGPGRP: Self = Self(0x540F);
    pub const TIOCSPGRP: Self = Self(0x5410);
}

pub struct TTY {
    holding_pgid: AtomicUsize,
}
impl TTY {
    pub fn new() -> Self {
        Self {
            // default pgid is 1 (init)
            holding_pgid: AtomicUsize::new(1),
        }
    }
}

impl VfsFile for TTY {
    impl_vfs_default_non_dir!(ZeroDev);

    fn attr_kind(&self) -> VfsFileKind {
        VfsFileKind::CharDevice
    }
    fn attr_device(&self) -> DeviceInfo {
        DeviceInfo {
            device_id: DeviceIDCollection::DEV_FS_ID,
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
        dyn_future(async {
            Ok(TimeInfo {
                access: 0,
                modify: 0,
                change: 0,
            })
        })
    }
    fn update_time(&self, _info: crate::fs::new_vfs::top::TimeInfoChange) -> ASysResult {
        todo!()
    }

    fn ioctl(&self, cmd: IOCTLCmd, arg: usize) -> ASysResult<usize> {
        #[allow(non_camel_case_types)]
        type pid_t = i32;
        use core::sync::atomic::Ordering;

        dyn_future(async move {
            let lproc = get_curr_lproc().unwrap();
            match cmd {
                IOCTLCmd::TIOCGPGRP => {
                    let argp = UserWritePtr::<pid_t>::from(arg);
                    let pgid = self.holding_pgid.load(Ordering::Relaxed);
                    argp.write(&lproc, pgid as pid_t)?;
                }
                IOCTLCmd::TIOCSPGRP => {
                    let argp = UserReadPtr::<pid_t>::from(arg);
                    let pgid = argp.read(&lproc)?;
                    self.holding_pgid.store(pgid as usize, Ordering::Relaxed);
                }
                _ => {
                    log::warn!("unsupported ioctl cmd: {:?}, just return 0", cmd)
                }
            };
            Ok(0)
        })
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        Stdin.read_at(offset, buf)
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        Stdout.write_at(offset, buf)
    }

    fn get_page(&self, _offset: usize, _kind: MmapKind) -> ASysResult<PhysAddr4K> {
        dyn_future(async move { unimplemented!() })
    }
    fn truncate(&self, _length: usize) -> ASysResult {
        dyn_future(async move { unimplemented!() })
    }

    fn poll_ready(&self, offset: usize, len: usize, kind: PollKind) -> ASysResult<usize> {
        Stdin.poll_ready(offset, len, kind)
    }
    fn poll_read(&self, offset: usize, buf: &mut [u8]) -> usize {
        Stdin.poll_read(offset, buf)
    }
    fn poll_write(&self, offset: usize, buf: &[u8]) -> usize {
        Stdout.poll_write(offset, buf)
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
