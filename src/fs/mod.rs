use crate::driver;

pub mod disk;
pub mod fatfs;
pub mod partition;

pub mod root;
pub mod stdio;
pub mod vfs;

type BlockDevice = driver::VirtIoBlockDev;
