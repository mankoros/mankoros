use alloc::sync::Arc;
use log::info;

use crate::{
    drivers::{AsyncBlockDevice, BlockDevice},
    executor::block_on,
    fs::{
        memfs::{tmpdir::TmpDir, tty::TTY, zero::ZeroDev},
        new_vfs::{mount::MountPoint, top::VfsFileRef},
        procfs::ProcFS,
    },
    tools::errors::SysResult,
};

pub mod disk;
pub mod partition;

pub mod memfs;
pub mod new_vfs;
pub mod nfat32;
pub mod pipe;
pub mod procfs;
pub mod root;
pub mod stdio;

pub fn init_filesystems(blk_dev: Arc<dyn AsyncBlockDevice>) {
    info!("Filesystem built-in self testing (BIST)...");
    new_vfs::path::path_test();

    info!("Initialize filesystems...");
    info!("  use block device: {:?}", blk_dev.name());

    self::root::init_rootfs(blk_dev);
    block_on(mount_all_fs()).unwrap();
}

async fn mount_all_fs() -> SysResult<()> {
    let root_dir = self::root::get_root_dir();
    // Mount devfs
    let dev_dir = VfsFileRef::new(TmpDir::new());
    root_dir.attach("dev", dev_dir.clone()).await?;
    dev_dir.attach("zero", VfsFileRef::new(ZeroDev)).await?;
    dev_dir.attach("vda2", VfsFileRef::new(ZeroDev)).await?;
    dev_dir.attach("tty", VfsFileRef::new(TTY)).await?;

    // Mount procfs
    let proc_mp = VfsFileRef::new(MountPoint::new(Arc::new(ProcFS)));
    root_dir.attach("proc", proc_mp).await?;

    Ok(())
}
