mod dw_mshc;
mod probe;
mod virtio;

pub type VirtIoBlkDev<H, T> = virtio::VirtIoBlkDev<H, T>;
pub type VirtIoHalImpl = virtio::VirtIoHalImpl;

pub use probe::probe_virtio_blk;

use super::wait_for;
