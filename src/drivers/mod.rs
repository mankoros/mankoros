mod blk;
mod manager;
mod serial;

use core::fmt::Debug;
use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::sync::Arc;
pub use manager::DeviceManager;
pub use serial::EarlyConsole;

pub use transport::mmio::MmioTransport;
use virtio_drivers::transport;

pub type VirtIoBlockDev =
    blk::VirtIoBlkDev<blk::VirtIoHalImpl, virtio_drivers::transport::mmio::MmioTransport>;

static mut DEVICE_MANAGER: Option<DeviceManager> = None;

pub fn get_device_manager() -> &'static DeviceManager {
    unsafe { DEVICE_MANAGER.as_ref().unwrap() }
}
pub fn get_device_manager_mut() -> &'static mut DeviceManager {
    unsafe { DEVICE_MANAGER.as_mut().unwrap() }
}

pub fn init_device_manager() {
    unsafe {
        DEVICE_MANAGER = Some(DeviceManager::new());
    }
}

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
type Async<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type ADevResult<'a> = Async<'a, DevResult>;

#[const_trait]
pub trait Device: Send + Sync {
    fn name(&self) -> &str;

    /// Register base address
    fn mmio_base(&self) -> usize;
    /// Device register & FIFO etc size
    fn mmio_size(&self) -> usize;

    fn device_type(&self) -> DeviceType;

    /// Interrupt number
    fn interrupt_number(&self) -> Option<usize>;

    /// Interrupt handler
    fn interrupt_handler(&self);

    fn init(&self);

    // Trait convertion
    fn as_blk(self: Arc<Self>) -> Option<Arc<dyn BlockDevice>>;
    fn as_char(self: Arc<Self>) -> Option<Arc<dyn CharDevice>>;
}

pub trait BlockDevice: Device + Debug {
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;

    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> DevResult;
    fn write_block(&self, block_id: u64, buf: &[u8]) -> DevResult;
    fn flush(&self) -> DevResult;
}

pub trait CharDevice: Device + Debug {
    fn read(&self, buf: &mut [u8]) -> ADevResult;
    fn write(&self, buf: &[u8]) -> DevResult;
}
