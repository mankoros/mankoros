use super::{top::{VfsFS, VfsFile, VfsFileRef}, path::Path, DeviceID, underlying::ConcreteFS};
use alloc::{sync::Arc, vec::Vec, collections::BTreeMap};
use crate::{impl_vfs_default_non_file, impl_vfs_forward_dir};

struct MountPoint {
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
    fn attr<'a>(&'a self) -> crate::tools::errors::ASysResult<'a, super::VfsFileAttr> {
        self.root.attr()
    }

    impl_vfs_forward_dir!(root);
    impl_vfs_default_non_file!(MountPoint);
}

pub struct MountManager {
    mounted_fs: BTreeMap<DeviceID, Arc<dyn VfsFS>>,
    // 按照路径的长度降序排序
    mount_points: Vec<(Path, MountPoint)>
}

