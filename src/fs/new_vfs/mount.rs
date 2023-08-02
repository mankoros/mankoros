use super::{
    path::Path,
    top::{VfsFSRef, VfsFile, VfsFileRef},
    DeviceID,
};
use crate::{here, impl_vfs_default_non_file, impl_vfs_forward_dir, sync::SpinNoIrqLock};
use alloc::{collections::BTreeMap, vec::Vec};

pub struct MountPoint {
    fs: VfsFSRef,
    root: VfsFileRef,
}

unsafe impl Send for MountPoint {}
unsafe impl Sync for MountPoint {}

impl MountPoint {
    fn new(fs: VfsFSRef) -> Self {
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

pub struct GlobalMountManager {
    mounted_fs: BTreeMap<DeviceID, VfsFSRef>,
    // 按照路径的长度降序排序
    mount_points: Vec<(Path, VfsFSRef)>,
}

static MGR: SpinNoIrqLock<GlobalMountManager> = SpinNoIrqLock::new(GlobalMountManager {
    mounted_fs: BTreeMap::new(),
    mount_points: Vec::new(),
});

impl GlobalMountManager {
    pub fn register(path: Path, fs: VfsFSRef) -> MountPoint {
        MGR.lock(here!()).mount_points.push((path, fs.clone()));
        MountPoint::new(fs)
    }
    /// just helper function for [[register]]
    pub fn register_as_file(path: &str, fs: VfsFSRef) -> VfsFileRef {
        let path = Path::from(path);
        let mp = Self::register(path, fs);
        VfsFileRef::new(mp)
    }
}
