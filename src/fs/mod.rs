use crate::driver;

pub mod disk;
pub mod fatfs;
pub mod partition;

pub mod vfs;

type BlockDevice = driver::VirtIoBlockDev;
