mod sifive;
mod uart8250;
use super::wait_for;
use alloc::{boxed::Box, collections::VecDeque};
use log::{info, warn};
use ringbuffer::{RingBufferExt, RingBufferRead, RingBufferWrite};

use core::{
    cell::UnsafeCell,
    fmt::{Debug, Write},
    future::Future,
    pin::Pin,
    task::{Poll, Waker},
};

use crate::{
    consts::address_space::K_SEG_DTB, here, memory::kernel_phys_dev_to_virt, println,
    sync::SpinNoIrqLock,
};

use super::{CharDevice, DevResult, Device, DeviceType};

pub struct EarlyConsole;

impl Write for EarlyConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            sbi_rt::legacy::console_putchar(byte.into());
        }
        Ok(())
    }
}

trait UartDriver: Send + Sync {
    fn init(&mut self);
    fn putchar(&mut self, byte: u8);
    fn getchar(&mut self) -> Option<u8>;
}

pub struct Serial {
    inner: UnsafeCell<Box<dyn UartDriver>>,
    buffer: SpinNoIrqLock<ringbuffer::ConstGenericRingBuffer<u8, 512>>, // Hard-coded buffer size
    waiting: SpinNoIrqLock<VecDeque<Waker>>,
    base_address: usize,
    size: usize,
    interrupt_number: usize,
}

unsafe impl Send for Serial {}
unsafe impl Sync for Serial {}

impl Serial {
    fn new(
        base_address: usize,
        size: usize,
        interrupt_number: usize,
        driver: Box<dyn UartDriver>,
    ) -> Self {
        Self {
            inner: UnsafeCell::new(driver),
            buffer: SpinNoIrqLock::new(ringbuffer::ConstGenericRingBuffer::new()),
            base_address,
            size,
            interrupt_number,
            waiting: SpinNoIrqLock::new(VecDeque::new()),
        }
    }
}

impl Debug for Serial {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "Serial")
    }
}

impl Device for Serial {
    fn name(&self) -> &str {
        "serial"
    }

    fn mmio_base(&self) -> usize {
        self.base_address
    }

    fn mmio_size(&self) -> usize {
        self.size
    }

    fn device_type(&self) -> super::DeviceType {
        DeviceType::Char
    }

    fn interrupt_number(&self) -> Option<usize> {
        Some(self.interrupt_number)
    }

    fn interrupt_handler(&self) {
        let byte = unsafe { &mut *self.inner.get() }.as_mut().getchar();
        if let Some(b) = byte {
            info!(
                "Serial interrupt handler got byte: {}",
                core::str::from_utf8(&[b]).unwrap()
            );
            self.buffer.lock(here!()).enqueue(b);
            // Round Robin
            if let Some(waiting) = self.waiting.lock(here!()).pop_front() {
                waiting.wake();
            }
        } else {
            warn!("Serial interrupt handler got no byte");
        }
    }

    fn init(&self) {
        unsafe { &mut *self.inner.get() }.as_mut().init()
    }

    fn as_char(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn CharDevice>> {
        Some(self)
    }

    fn as_blk(self: alloc::sync::Arc<Self>) -> Option<alloc::sync::Arc<dyn super::BlockDevice>> {
        None
    }

    fn as_async_blk(
        self: alloc::sync::Arc<Self>,
    ) -> Option<alloc::sync::Arc<dyn super::AsyncBlockDevice>> {
        None
    }
}

pub struct SerialReadFuture<'a> {
    buf: Pin<&'a mut [u8]>,
    inner: &'a Serial,
}

impl Future for SerialReadFuture<'_> {
    type Output = DevResult<usize>;

    fn poll(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        let this = unsafe { self.get_unchecked_mut() };
        if let Some(char) = this.inner.buffer.lock(here!()).dequeue() {
            this.buf[0] = char;
            Poll::Ready(Ok(1))
        } else {
            // Push itself to the waiting queue
            this.inner.waiting.lock(here!()).push_back(cx.waker().clone());
            Poll::Pending
        }
    }
}

impl CharDevice for Serial {
    fn read<'a>(&'a self, buf: Pin<&'a mut [u8]>) -> super::ADevResult<usize> {
        Box::pin(SerialReadFuture { buf, inner: self })
    }

    fn write(&self, buf: &[u8]) -> super::DevResult {
        for byte in buf {
            unsafe { &mut *self.inner.get() }.as_mut().putchar(*byte)
        }
        Ok(())
    }
}

pub fn probe() -> Option<Serial> {
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    let chosen = device_tree.chosen();
    if let Some(bootargs) = chosen.bootargs() {
        println!("Bootargs: {:?}", bootargs);
    }

    println!("Device: {}", device_tree.root().model());

    // Serial
    let mut stdout = chosen.stdout();
    if stdout.is_none() {
        println!("Non-standard stdout device, trying to workaround");
        let chosen = device_tree.find_node("/chosen").expect("No chosen node");
        let stdout_path = chosen
            .properties()
            .find(|n| n.name == "stdout-path")
            .and_then(|n| {
                let bytes = unsafe {
                    core::slice::from_raw_parts_mut((n.value.as_ptr()) as *mut u8, n.value.len())
                };
                let mut len = 0;
                for byte in bytes.iter() {
                    if *byte == b':' {
                        return core::str::from_utf8(&n.value[..len]).ok();
                    }
                    len += 1;
                }
                core::str::from_utf8(&n.value[..n.value.len() - 1]).ok()
            })
            .unwrap();
        println!("Searching stdout: {}", stdout_path);
        stdout = device_tree.find_node(stdout_path);
    }
    if stdout.is_none() {
        println!("Unable to parse /chosen, choosing first serial device");
        stdout = device_tree.find_compatible(&[
            "ns16550a",
            "snps,dw-apb-uart", // C910, VF2
            "sifive,uart0",     // sifive_u QEMU (FU540)
        ])
    }
    let stdout = stdout.expect("Still unable to get stdout device");
    println!("Stdout: {}", stdout.name);

    Some(probe_serial_console(&stdout))
}

/// This guarantees to return a Serial device
/// The device is not initialized yet
fn probe_serial_console(stdout: &fdt::node::FdtNode) -> Serial {
    let reg = stdout.reg().unwrap().next().unwrap();
    let base_paddr = reg.starting_address as usize;
    let size = reg.size.unwrap();
    let base_vaddr = kernel_phys_dev_to_virt(base_paddr);
    let mut irq_number = stdout.property("interrupts").unwrap().as_usize().unwrap();
    info!("IRQ number: {}", irq_number);

    let first_compatible = stdout.compatible().unwrap().first();
    match first_compatible {
        "ns16550a" | "snps,dw-apb-uart" => {
            // VisionFive 2 (FU740)
            // virt QEMU

            // Parse clock frequency
            let freq_raw = stdout
                .property("clock-frequency")
                .expect("No clock-frequency property of stdout serial device")
                .as_usize()
                .expect("Parse clock-frequency to usize failed");
            let mut reg_io_width = 1;
            if let Some(reg_io_width_raw) = stdout.property("reg-io-width") {
                reg_io_width =
                    reg_io_width_raw.as_usize().expect("Parse reg-io-width to usize failed");
            }
            let mut reg_shift = 0;
            if let Some(reg_shift_raw) = stdout.property("reg-shift") {
                reg_shift = reg_shift_raw.as_usize().expect("Parse reg-shift to usize failed");
            }
            let uart = unsafe {
                uart8250::Uart::new(
                    base_vaddr,
                    freq_raw,
                    115200,
                    reg_io_width,
                    reg_shift,
                    first_compatible == "snps,dw-apb-uart",
                )
            };
            return Serial::new(base_paddr, size, irq_number, Box::new(uart));
        }
        "sifive,uart0" => {
            // sifive_u QEMU (FU540)
            let uart = sifive::SifiveUart::new(
                base_vaddr,
                500 * 1000 * 1000, // 500 MHz hard coded for now
            );
            return Serial::new(base_paddr, size, irq_number, Box::new(uart));
        }
        _ => panic!("Unsupported serial console"),
    }
    unreachable!();
}
