use core::cell::UnsafeCell;
use core::fmt::Debug;

use super::as_dev_err;
use super::BaseDriverOps;
/// Block device general traits
use super::BlockDriverOps;
use super::DevResult;
use super::DeviceType;

/// VirtIO blk driver
///
///
use virtio_drivers::{device::blk::VirtIOBlk as InnerDev, transport::Transport, Hal};
pub struct VirtIoBlkDev<H: Hal, T: Transport> {
    inner: UnsafeCell<InnerDev<H, T>>,
    pos: u64,
}

unsafe impl<H: Hal, T: Transport> Send for VirtIoBlkDev<H, T> {}
unsafe impl<H: Hal, T: Transport> Sync for VirtIoBlkDev<H, T> {}

impl<H: Hal, T: Transport> Debug for VirtIoBlkDev<H, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VirtIoBlkDev").field("pos", &self.pos).finish()
    }
}

impl<H: Hal, T: Transport> VirtIoBlkDev<H, T> {
    pub fn try_new(transport: T) -> DevResult<Self> {
        Ok(Self {
            inner: UnsafeCell::new(InnerDev::new(transport).map_err(as_dev_err)?),
            pos: 0,
        })
    }
}

impl<H: Hal, T: Transport> const BaseDriverOps for VirtIoBlkDev<H, T> {
    fn device_name(&self) -> &str {
        "virtio-blk"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }
}

impl<H: Hal, T: Transport> BlockDriverOps for VirtIoBlkDev<H, T> {
    #[inline]
    fn num_blocks(&self) -> u64 {
        (unsafe { &*self.inner.get() }).capacity()
    }

    #[inline]
    fn block_size(&self) -> usize {
        virtio_drivers::device::blk::SECTOR_SIZE
    }

    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> DevResult {
        (unsafe { &mut *self.inner.get() })
            .read_block(block_id as _, buf)
            .map_err(as_dev_err)
    }

    fn write_block(&self, block_id: u64, buf: &[u8]) -> DevResult {
        (unsafe { &mut *self.inner.get() })
            .write_block(block_id as _, buf)
            .map_err(as_dev_err)
    }

    fn flush(&self) -> DevResult {
        Ok(())
    }
}
