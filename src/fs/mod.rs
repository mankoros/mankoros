use alloc::{sync::Arc, vec::Vec};
use log::info;
use mbr_nostd::PartitionTable;

use crate::{
    driver::{self, BaseDriverOps},
    here,
    sync::SpinNoIrqLock,
};

pub mod disk;
pub mod fatfs;
pub mod partition;

pub mod pipe;
pub mod root;
pub mod stdio;
pub mod vfs;

type BlockDevice = driver::VirtIoBlockDev;

pub fn init_filesystems(blk_dev: BlockDevice) {
    info!("Filesystem built-in self testing (BIST)...");
    vfs::path::path_test();

    info!("Initialize filesystems...");
    info!("  use block device: {:?}", blk_dev.device_name());

    let mut disk = self::disk::Disk::new(blk_dev);
    let mbr = disk.mbr();
    let disk = Arc::new(SpinNoIrqLock::new(disk));
    let mut partitions = Vec::new();
    for entry in mbr.partition_table_entries() {
        if entry.partition_type != mbr_nostd::PartitionType::Unused {
            info!("Partition table entry: {:#x?}", entry);
            partitions.push(partition::Partition::new(
                entry.logical_block_address as u64 * disk::BLOCK_SIZE as u64,
                entry.sector_count as u64 * disk::BLOCK_SIZE as u64,
                disk.clone(),
            ))
        }
    }
    if partitions.len() == 0 {
        // The disk may not have a partition table.
        // Assume it is a FAT32 filesystem.
        partitions.push(partition::Partition::new(
            0,
            disk.lock(here!()).size(),
            disk.clone(),
        ))
    }
    self::root::init_rootfs(partitions[0].clone());
}
