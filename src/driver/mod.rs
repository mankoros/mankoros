pub mod uart;

mod blk;

/// General Device Operations
/// Adapted from ArceOS
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DeviceType {
    Block,
    Char,
    Net,
    Display,
}

#[derive(Debug)]
pub enum DevError {
    /// An entity already exists.
    AlreadyExists,
    /// Try again, for non-blocking APIs.
    Again,
    /// Bad internal state.
    BadState,
    /// Invalid parameter/argument.
    InvalidParam,
    /// Input/output error.
    IO,
    /// Not enough space/cannot allocate memory (DMA).
    NoMemory,
    /// Device or resource is busy.
    ResourceBusy,
    /// This operation is unsupported or unimplemented.
    Unsupported,
}

pub type DevResult<T = ()> = Result<T, DevError>;

#[const_trait]
pub trait BaseDriverOps: Send + Sync {
    fn device_name(&self) -> &str;
    fn device_type(&self) -> DeviceType;
}
pub trait BlockDriverOps: BaseDriverOps {
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;

    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> DevResult;
    fn write_block(&self, block_id: u64, buf: &[u8]) -> DevResult;
    fn flush(&self) -> DevResult;
}

use log::{info, warn};
pub use transport::mmio::MmioTransport;
use virtio_drivers::transport::{self, Transport};

use crate::{
    consts::{
        address_space::{K_SEG_DATA_BEG, K_SEG_DATA_END, K_SEG_PHY_MEM_BEG, K_SEG_PHY_MEM_END},
        platform,
    },
    memory::{
        frame, kernel_phys_dev_to_virt, kernel_phys_to_virt, kernel_virt_text_to_phys,
        kernel_virt_to_phys,
    },
};

// TODO: implement a device manager

fn probe_devices_common<D, F>(dev_type: DeviceType, ret: F) -> Option<D>
where
    D: BaseDriverOps,
    F: FnOnce(MmioTransport) -> Option<D>,
{
    for reg in platform::VIRTIO_MMIO_REGIONS {
        if let Some(transport) = probe_mmio_device(
            kernel_phys_dev_to_virt(reg.0.into()) as *mut u8,
            reg.1,
            Some(dev_type),
        ) {
            let dev = ret(transport)?;
            info!(
                "created a new {:?} device: {:?}",
                dev.device_type(),
                dev.device_name()
            );
            return Some(dev);
        }
    }
    None
}

pub fn probe_virtio_blk() -> Option<VirtIoBlockDev> {
    probe_devices_common(DeviceType::Block, |t| VirtIoBlockDev::try_new(t).ok())
}

fn probe_mmio_device(
    reg_base: *mut u8,
    _reg_size: usize,
    type_match: Option<DeviceType>,
) -> Option<MmioTransport> {
    use transport::mmio::VirtIOHeader;

    let header = NonNull::new(reg_base as *mut VirtIOHeader).unwrap();
    if let Ok(transport) = unsafe { MmioTransport::new(header) } {
        if type_match.is_none() || as_dev_type(transport.device_type()) == type_match {
            info!(
                "Detected virtio MMIO device with vendor id: {:#X}, device type: {:?}, version: {:?}",
                transport.vendor_id(),
                transport.device_type(),
                transport.version(),
            );
            Some(transport)
        } else {
            None
        }
    } else {
        None
    }
}

/// VirtIO Error mappings
const fn as_dev_type(t: transport::DeviceType) -> Option<DeviceType> {
    use transport::DeviceType::*;
    match t {
        Block => Some(DeviceType::Block),
        Network => Some(DeviceType::Net),
        GPU => Some(DeviceType::Display),
        _ => None,
    }
}

#[allow(dead_code)]
const fn as_dev_err(e: virtio_drivers::Error) -> DevError {
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

pub type VirtIoBlockDev =
    blk::VirtIoBlkDev<VirtIoHalImpl, virtio_drivers::transport::mmio::MmioTransport>;

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
