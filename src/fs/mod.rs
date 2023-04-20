use crate::driver;

pub mod disk;
pub mod fat32;
pub mod partition;

type BlockDevice = driver::VirtIoBlockDev;
