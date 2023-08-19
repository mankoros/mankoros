use crate::{
    fs::new_vfs::{
        top::{DeviceInfo, MmapKind, PollKind, SizeInfo, TimeInfo, VfsFile},
        DeviceIDCollection, VfsFileKind,
    },
    impl_vfs_default_non_dir,
    memory::{address::PhysAddr4K, frame::alloc_frame},
    tools::errors::{dyn_future, ASysResult, SysError},
};

pub struct ZeroDev;
impl VfsFile for ZeroDev {
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

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move { Ok(self.poll_read(offset, buf)) })
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async move { Ok(self.poll_write(offset, buf)) })
    }

    fn get_page(&self, _offset: usize, kind: MmapKind) -> ASysResult<PhysAddr4K> {
        dyn_future(async move {
            match kind {
                MmapKind::Shared => unimplemented!(),
                MmapKind::Private => {
                    // TODO: 这直接 alloc 出来的内存真的是清零的吗
                    alloc_frame().ok_or(SysError::ENOMEM)
                }
            }
        })
    }
    fn truncate(&self, _length: usize) -> ASysResult {
        dyn_future(async move { Ok(()) })
    }

    fn poll_ready(&self, _offset: usize, len: usize, _kind: PollKind) -> ASysResult<usize> {
        dyn_future(async move { Ok(len) })
    }
    fn poll_read(&self, _offset: usize, buf: &mut [u8]) -> usize {
        buf.fill(0);
        buf.len()
    }
    fn poll_write(&self, _offset: usize, buf: &[u8]) -> usize {
        buf.len()
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }
}
