//! Root directory of the filesystem
//!
//! Adapted from ArceOS
//! Copyright (C) 2023 by ArceOS
//! Copyright (C) 2023 by MankorOS



use crate::lazy_init::LazyInit;


use super::new_vfs::top::VfsFileRef;
use alloc::sync::Arc;
use crate::executor::block_on;
use crate::fs::new_vfs::mount::MountPoint;
use crate::drivers::BlockDevice;
use crate::fs::nfat32::FatFSWrapper;

static ROOT_DIR: LazyInit<VfsFileRef> = LazyInit::new();

pub fn get_root_dir() -> VfsFileRef {
    ROOT_DIR.clone()
}

pub fn init_rootfs(blk_dev: Arc<dyn BlockDevice>) {
    static FAT_FS: LazyInit<Arc<FatFSWrapper>> = LazyInit::new();
    FAT_FS.init_by(Arc::new(block_on(FatFSWrapper::new(blk_dev.use_as_async())).unwrap()));
    let main_fs = FAT_FS.clone();

    let root_dir = MountPoint::new(main_fs);
    ROOT_DIR.init_by(VfsFileRef::new(root_dir));
}
