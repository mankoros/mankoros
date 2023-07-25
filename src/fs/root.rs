//! Root directory of the filesystem
//!
//! Adapted from ArceOS
//! Copyright (C) 2023 by ArceOS
//! Copyright (C) 2023 by MankorOS

use alloc::sync::Arc;

use crate::lazy_init::LazyInit;

use super::new_vfs::mount::MountPoint;
use super::new_vfs::top::VfsFileRef;
use super::partition::Partition;

static ROOT_DIR: LazyInit<VfsFileRef> = LazyInit::new();

pub fn get_root_dir() -> VfsFileRef {
    ROOT_DIR.clone()
}

pub fn init_rootfs(part: Partition) {
    // static FAT_FS: LazyInit<Arc<FatFileSystem>> = LazyInit::new();
    // FAT_FS.init_by(Arc::new(FatFileSystem::new(part)));
    // FAT_FS.init();
    // let main_fs = FAT_FS.clone();

    // let root_dir = MountPoint::new(main_fs);
    // ROOT_DIR.init_by(VfsFileRef::new(root_dir));
}
