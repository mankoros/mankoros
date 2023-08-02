use alloc::sync::Arc;
use log::info;

use self::new_vfs::{
    mount::GlobalMountManager,
    top::{VfsFS, VfsFSAttr, VfsFSKind, VfsFSRef, NORMAL_FILE_NAME_LENGTH},
    DeviceIDCollection,
};
use crate::{
    drivers::{AsyncBlockDevice, BlockDevice},
    executor::block_on,
    fs::{
        memfs::{tmpdir::TmpDir, tty::TTY, zero::ZeroDev},
        new_vfs::top::VfsFileRef,
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
pub mod stdio;

use crate::fs::nfat32::FatFSWrapper;
use crate::lazy_init::LazyInit;

static ROOT_DIR: LazyInit<VfsFileRef> = LazyInit::new();

pub fn get_root_dir() -> VfsFileRef {
    ROOT_DIR.clone()
}

pub fn init_rootfs(blk_dev: Arc<dyn AsyncBlockDevice>) {
    static FAT_FS: LazyInit<VfsFSRef> = LazyInit::new();
    FAT_FS.init_by(VfsFSRef::new(block_on(FatFSWrapper::new(blk_dev)).unwrap()));
    let main_fs = FAT_FS.clone();

    let root_dir = GlobalMountManager::register_as_file("/", main_fs);
    ROOT_DIR.init_by(root_dir);
}

pub fn init_filesystems(blk_dev: Arc<dyn AsyncBlockDevice>) {
    info!("Filesystem built-in self testing (BIST)...");
    new_vfs::path::path_test();

    info!("Initialize filesystems...");
    info!("  use block device: {:?}", blk_dev.name());

    init_rootfs(blk_dev);
    block_on(mount_all_fs()).unwrap();
}

struct DevFS(VfsFileRef);
impl VfsFS for DevFS {
    fn root(&self) -> VfsFileRef {
        self.0.clone()
    }
    fn attr(&self) -> VfsFSAttr {
        VfsFSAttr::default_mem(VfsFSKind::Dev, DeviceIDCollection::DEV_FS_ID)
    }
}

async fn mount_all_fs() -> SysResult<()> {
    let root_dir = get_root_dir();
    // Mount devfs
    let dev_fs = VfsFSRef::new(DevFS(VfsFileRef::new(TmpDir::new())));
    let dev_dir = dev_fs.root();
    dev_dir.attach("null", VfsFileRef::new(ZeroDev)).await?;
    dev_dir.attach("zero", VfsFileRef::new(ZeroDev)).await?;
    dev_dir.attach("vda2", VfsFileRef::new(ZeroDev)).await?;
    dev_dir.attach("tty", VfsFileRef::new(TTY)).await?;

    // Mount devfs
    let dev_mp = GlobalMountManager::register_as_file("/dev", dev_fs);
    root_dir.attach("dev", dev_mp).await?;
    // Mount procfs
    let proc_mp = GlobalMountManager::register_as_file("/proc", VfsFSRef::new(ProcFS));
    root_dir.attach("proc", proc_mp).await?;

    Ok(())
}
