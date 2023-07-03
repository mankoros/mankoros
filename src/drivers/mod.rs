mod blk;
mod manager;
mod serial;

use core::fmt::Debug;

pub use manager::DeviceManager;
pub use serial::EarlyConsole;
pub use serial::SifiveUart;
pub use serial::Uart;

extern crate downcast_rs;

use downcast_rs::DowncastSync;
pub use transport::mmio::MmioTransport;
use virtio_drivers::transport;

pub type VirtIoBlockDev =
    blk::VirtIoBlkDev<blk::VirtIoHalImpl, virtio_drivers::transport::mmio::MmioTransport>;

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
pub trait Device: Send + Sync + DowncastSync {
    fn name(&self) -> &str;

    /// Register base address
    fn mmio_base(&self) -> usize;
    /// Device register & FIFO etc size
    fn mmio_size(&self) -> usize;

    fn device_type(&self) -> DeviceType;

    /// Interrupt number
    fn interrupt_number(&self) -> usize;

    /// Interrupt handler
    fn interrupt_handler(&self);

    fn init(&mut self);
}
impl_downcast!(sync Device);

pub trait BlockDevice: Device + Debug {
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;

    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> DevResult;
    fn write_block(&self, block_id: u64, buf: &[u8]) -> DevResult;
    fn flush(&self) -> DevResult;
}
