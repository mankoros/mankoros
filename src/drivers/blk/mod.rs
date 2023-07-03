mod dev;
mod probe;

pub type VirtIoBlkDev<H, T> = dev::VirtIoBlkDev<H, T>;
pub type VirtIoHalImpl = dev::VirtIoHalImpl;

pub use probe::probe;
