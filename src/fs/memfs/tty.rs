use crate::{
    fs::{
        new_vfs::{
            top::{MmapKind, VfsFile},
            DeviceIDCollection, VfsFileAttr, VfsFileKind,
        },
        stdio::{Stdin, Stdout},
    },
    impl_vfs_default_non_dir,
    memory::{address::PhysAddr4K, frame::alloc_frame},
    tools::errors::{dyn_future, ASysResult, SysError},
};

pub struct TTY;
impl VfsFile for TTY {
    impl_vfs_default_non_dir!(ZeroDev);

    fn attr(&self) -> ASysResult<VfsFileAttr> {
        dyn_future(async {
            Ok(VfsFileAttr {
                kind: VfsFileKind::CharDevice,
                device_id: DeviceIDCollection::DEV_FS_ID,
                self_device_id: 0,
                byte_size: 0,
                block_count: 0,
                access_time: 0,
                modify_time: 0,
                create_time: 0, // TODO: create time
            })
        })
    }

    fn read_at<'a>(&'a self, offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        Stdin.read_at(offset, buf)
    }

    fn write_at<'a>(&'a self, offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        Stdout.write_at(offset, buf)
    }

    fn get_page(&self, _offset: usize, kind: MmapKind) -> ASysResult<PhysAddr4K> {
        dyn_future(async move { unimplemented!() })
    }
}
