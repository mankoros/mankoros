use alloc::sync::Arc;
use log::info;

use crate::{
    drivers::{AsyncBlockDevice, BlockDevice},
    executor::block_on,
    fs::{
        memfs::{tmpdir::TmpDir, tty::TTY, zero::ZeroDev},
        new_vfs::top::VfsFileRef,
    },
};

pub mod disk;
pub mod partition;

pub mod memfs;
pub mod new_vfs;
pub mod nfat32;
pub mod pipe;
pub mod root;
pub mod stdio;

pub fn init_filesystems(blk_dev: Arc<dyn AsyncBlockDevice>) {
    info!("Filesystem built-in self testing (BIST)...");
    new_vfs::path::path_test();

    info!("Initialize filesystems...");
    info!("  use block device: {:?}", blk_dev.name());

    self::root::init_rootfs(blk_dev);

    let root_dir = self::root::get_root_dir();
    // Mount devfs
    let dev_dir = VfsFileRef::new(TmpDir::new());
    block_on(root_dir.attach("dev", dev_dir.clone())).unwrap();
    block_on(dev_dir.attach("zero", VfsFileRef::new(ZeroDev))).unwrap();
    block_on(dev_dir.attach("vda2", VfsFileRef::new(ZeroDev))).unwrap();
    block_on(dev_dir.attach("tty", VfsFileRef::new(TTY))).unwrap();
}
