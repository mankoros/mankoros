mod blk;
mod cpu;
mod manager;
mod plic;
mod serial;

use core::fmt::Debug;
use core::future::Future;
use core::pin::Pin;

use alloc::boxed::Box;
use alloc::sync::Arc;
pub use manager::DeviceManager;
pub use serial::EarlyConsole;

use crate::tools::errors::dyn_future;
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

macro_rules! wait_for {
    ($cond:expr) => {
        while !$cond {
            core::hint::spin_loop();
        }
    };
}
pub(crate) use wait_for;

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
pub type ADevResult<'a, T = ()> = Async<'a, DevResult<T>>;

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
    fn as_async_blk(self: Arc<Self>) -> Option<Arc<dyn AsyncBlockDevice>>;
}

pub trait BlockDevice: Device + Debug {
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;

    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> DevResult;
    fn write_block(&self, block_id: u64, buf: &[u8]) -> DevResult;
    fn flush(&self) -> DevResult;

    fn use_as_async(self: Arc<Self>) -> Arc<dyn AsyncBlockDevice>;
}

pub trait CharDevice: Device + Debug {
    fn read<'a>(&'a self, buf: Pin<&'a mut [u8]>) -> ADevResult<usize>;
    fn write(&self, buf: &[u8]) -> DevResult;
}

pub trait AsyncBlockDevice: Device + Debug {
    fn num_blocks(&self) -> u64;
    fn block_size(&self) -> usize;

    #[must_use = "futures do nothing unless polled"]
    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> ADevResult;
    #[must_use = "futures do nothing unless polled"]
    fn write_block(&self, block_id: u64, buf: &[u8]) -> ADevResult;
    #[must_use = "futures do nothing unless polled"]
    fn flush(&self) -> ADevResult;
}

impl<T: BlockDevice> AsyncBlockDevice for T {
    fn num_blocks(&self) -> u64 {
        self.num_blocks()
    }
    fn block_size(&self) -> usize {
        self.block_size()
    }
    fn read_block(&self, block_id: u64, buf: &mut [u8]) -> ADevResult {
        let result = self.read_block(block_id, buf);
        dyn_future(async move { result })
    }
    fn write_block(&self, block_id: u64, buf: &[u8]) -> ADevResult {
        let result = self.write_block(block_id, buf);
        dyn_future(async move { result })
    }
    fn flush(&self) -> ADevResult {
        let result = self.flush();
        dyn_future(async move { result })
    }
}
