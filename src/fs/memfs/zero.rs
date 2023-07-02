use crate::{fs::new_vfs::{top::{VfsFile, MmapKind}, VfsFileAttr, VfsFileKind, DeviceIDCollection}, impl_vfs_default_non_dir, tools::errors::{dyn_future, SysError, ASysResult}, memory::{frame::alloc_frame, address::PhysAddr4K}};

pub struct ZeroDev;
impl VfsFile for ZeroDev {
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

    fn read_at<'a>(&'a self, _offset: usize, buf: &'a mut [u8]) -> ASysResult<usize> {
        dyn_future(async move {
            buf.fill(0);
            Ok(buf.len())
        })
    }

    fn write_at<'a>(&'a self, _offset: usize, buf: &'a [u8]) -> ASysResult<usize> {
        dyn_future(async { Ok(buf.len()) })
    }

    fn get_page(&self, _offset: usize, kind: MmapKind) -> ASysResult<PhysAddr4K> {
        dyn_future(async move { 
            match kind {
                MmapKind::Shared => unimplemented!(),
                MmapKind::Private => {
                    // TODO: 这直接 alloc 出来的内存真的是清零的吗
                    alloc_frame().ok_or(SysError::ENOMEM)
                },
            }
        })
    }
}