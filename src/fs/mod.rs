use alloc::{sync::Arc, vec::Vec};
use log::info;
use mbr_nostd::PartitionTable;

use crate::{
    drivers::BlockDevice,
    executor::block_on,
    fs::{
        memfs::{tmpdir::TmpDir, tty::TTY, zero::ZeroDev},
        new_vfs::top::VfsFileRef,
    },
    here,
    sync::SpinNoIrqLock,
};

pub mod disk;
pub mod partition;

pub mod memfs;
pub mod new_vfs;
pub mod nfat32;
pub mod pipe;
pub mod root;
pub mod stdio;

pub fn init_filesystems(blk_dev: Arc<dyn BlockDevice>) {
    info!("Filesystem built-in self testing (BIST)...");
    new_vfs::path::path_test();

    info!("Initialize filesystems...");
    info!("  use block device: {:?}", blk_dev.name());

    let mut disk = self::disk::Disk::new(blk_dev);
    let mbr = disk.mbr();
    let disk = Arc::new(SpinNoIrqLock::new(disk));
    let mut partitions = Vec::new();
    for entry in mbr.partition_table_entries() {
        if entry.partition_type != mbr_nostd::PartitionType::Unused {
            info!("Partition table entry: {:x?}", entry);
            partitions.push(partition::Partition::new(
                entry.logical_block_address as u64 * disk::BLOCK_SIZE as u64,
                entry.sector_count as u64 * disk::BLOCK_SIZE as u64,
                disk.clone(),
            ))
        }
    }
    if partitions.is_empty() {
        // The disk may not have a partition table.
        // Assume it is a FAT32 filesystem.
        partitions.push(partition::Partition::new(
            0,
            disk.lock(here!()).size(),
            disk.clone(),
        ))
    }

    self::root::init_rootfs(partitions[0].clone());

    let root_dir = self::root::get_root_dir();
    // Mount devfs
    let dev_dir = VfsFileRef::new(TmpDir::new());
    block_on(root_dir.attach("dev", dev_dir.clone())).unwrap();
    block_on(dev_dir.attach("zero", VfsFileRef::new(ZeroDev))).unwrap();
    block_on(dev_dir.attach("vda2", VfsFileRef::new(ZeroDev))).unwrap();
    block_on(dev_dir.attach("tty", VfsFileRef::new(TTY))).unwrap();
}
