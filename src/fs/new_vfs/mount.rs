use super::{
    path::Path,
    top::{VfsFS, VfsFile, VfsFileRef},
    DeviceID,
};
use crate::{impl_vfs_default_non_file, impl_vfs_forward_dir};
use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};

pub struct MountPoint {
    fs: Arc<dyn VfsFS>,
    root: VfsFileRef,
}

unsafe impl Send for MountPoint {}
unsafe impl Sync for MountPoint {}

impl MountPoint {
    pub fn new(fs: Arc<dyn VfsFS>) -> Self {
        let root = fs.root();
        Self { fs, root }
    }
}

impl VfsFile for MountPoint {
    fn attr(&self) -> crate::tools::errors::ASysResult<'_, super::VfsFileAttr> {
        self.root.attr()
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    impl_vfs_forward_dir!(root);
    impl_vfs_default_non_file!(MountPoint);
}

pub struct MountManager {
    mounted_fs: BTreeMap<DeviceID, Arc<dyn VfsFS>>,
    // 按照路径的长度降序排序
    mount_points: Vec<(Path, MountPoint)>,
}
