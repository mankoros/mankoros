use core::cell::UnsafeCell;
use core::fmt::Debug;

use alloc::sync::Arc;
use log::warn;
use virtio_drivers::transport;

use crate::drivers::{BlockDevice, DevError, DevResult, Device, DeviceType};
use crate::{
    consts::address_space::{K_SEG_DATA_BEG, K_SEG_DATA_END, K_SEG_PHY_MEM_BEG, K_SEG_PHY_MEM_END},
    memory::{
        frame, kernel_phys_dev_to_virt, kernel_phys_to_virt, kernel_virt_text_to_phys,
        kernel_virt_to_phys,
    },
};

/// VirtIO blk driver
///
///
use virtio_drivers::{device::blk::VirtIOBlk as InnerDev, transport::Transport, Hal};
pub struct VirtIoBlkDev<H: Hal, T: Transport> {
    inner: UnsafeCell<InnerDev<H, T>>,
    base_address: usize,
    size: usize,
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
    pub fn try_new(transport: T, base_address: usize, size: usize) -> DevResult<Self> {
        Ok(Self {
            inner: UnsafeCell::new(InnerDev::new(transport).map_err(as_dev_err)?),
            pos: 0,
            base_address,
            size,
        })
    }
}
impl<H: Hal + 'static, T: Transport + 'static> Device for VirtIoBlkDev<H, T> {
    fn name(&self) -> &str {
        "virtio_blk"
    }

    fn mmio_base(&self) -> usize {
        self.base_address
    }

    fn mmio_size(&self) -> usize {
        self.size
    }

    fn init(&mut self) {
        // Not init needed
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Block
    }

    fn interrupt_number(&self) -> Option<usize> {
        None // No IRQ supported
    }

    fn interrupt_handler(&self) {
        panic!();
    }

    fn as_blk(self: Arc<Self>) -> Option<Arc<dyn BlockDevice>> {
        Some(self.clone())
    }
}

impl<H: Hal + 'static, T: Transport + 'static> BlockDevice for VirtIoBlkDev<H, T> {
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

/// VirtIO Error mappings
pub const fn as_dev_type(t: transport::DeviceType) -> Option<DeviceType> {
    use transport::DeviceType::*;
    match t {
        Block => Some(DeviceType::Block),
        Network => Some(DeviceType::Net),
        GPU => Some(DeviceType::Display),
        _ => None,
    }
}

#[allow(dead_code)]
pub const fn as_dev_err(e: virtio_drivers::Error) -> DevError {
    use virtio_drivers::Error::*;
    match e {
        QueueFull => DevError::BadState,
        NotReady => DevError::Again,
        WrongToken => DevError::BadState,
        AlreadyUsed => DevError::AlreadyExists,
        InvalidParam => DevError::InvalidParam,
        DmaError => DevError::NoMemory,
        IoError => DevError::IO,
        Unsupported => DevError::Unsupported,
        ConfigSpaceTooSmall => DevError::BadState,
        ConfigSpaceMissing => DevError::BadState,
    }
}

use core::ptr::NonNull;

/// VirtIO HAL DMA
///
pub struct VirtIoHalImpl;

unsafe impl virtio_drivers::Hal for VirtIoHalImpl {
    fn dma_alloc(
        pages: usize,
        _direction: virtio_drivers::BufferDirection,
    ) -> (virtio_drivers::PhysAddr, NonNull<u8>) {
        let paddr = if let Some(vaddr) = frame::alloc_frame_contiguous(pages, 1) {
            vaddr
        } else {
            return (0, NonNull::dangling());
        };
        let vaddr = kernel_phys_to_virt(paddr.bits());
        let ptr = NonNull::new(vaddr as _).unwrap();
        (paddr.bits(), ptr)
    }

    unsafe fn dma_dealloc(
        _paddr: virtio_drivers::PhysAddr,
        vaddr: NonNull<u8>,
        pages: usize,
    ) -> i32 {
        frame::dealloc_frames(kernel_virt_to_phys(vaddr.as_ptr() as usize), pages);
        0
    }

    #[inline]
    unsafe fn mmio_phys_to_virt(paddr: virtio_drivers::PhysAddr, _size: usize) -> NonNull<u8> {
        NonNull::new(kernel_phys_to_virt(paddr) as *mut u8).unwrap()
    }

    #[inline]
    unsafe fn share(
        buffer: NonNull<[u8]>,
        _direction: virtio_drivers::BufferDirection,
    ) -> virtio_drivers::PhysAddr {
        let vaddr = buffer.as_ptr() as *mut u8 as usize;
        if vaddr < K_SEG_PHY_MEM_END && vaddr >= K_SEG_PHY_MEM_BEG {
            kernel_virt_to_phys(vaddr)
        } else if vaddr < K_SEG_DATA_END && vaddr >= K_SEG_DATA_BEG {
            kernel_virt_text_to_phys(vaddr)
        } else {
            warn!(
                "VirtIO shares a buffer not in kernel text or phymem, vaddr: 0x{:#x}",
                vaddr
            );
            vaddr
        }
    }

    #[inline]
    unsafe fn unshare(
        _paddr: virtio_drivers::PhysAddr,
        _buffer: NonNull<[u8]>,
        _direction: virtio_drivers::BufferDirection,
    ) {
    }
}
