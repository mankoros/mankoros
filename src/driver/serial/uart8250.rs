//! Copyright (c) 2023 Easton Man
//! Adapted from https://docs.rs/uart_16550/latest/src/uart_16550/mmio.rs.html

use super::wait_for;
use bitflags::bitflags;
use core::fmt::Write; // for formatted output

// the UART control registers.
// some have different meanings for
// read vs write.
// http://byterunner.com/16550.html
const RHR: usize = 0; // receive holding register (for input bytes)
const THR: usize = 0; // transmit holding register (for output bytes)
const IER: usize = 1; // interrupt enable register
const FCR: usize = 2; // FIFO control register
const ISR: usize = 2; // interrupt status register
const LCR: usize = 3; // line control register
const MCR: usize = 4; // modem control register
const LSR: usize = 5; // line status register

bitflags! {
    /// Interrupt enable flags
    struct IntEnFlags: u8 {
        const RECEIVED = 1;
        const SENT = 1 << 1;
        const ERRORED = 1 << 2;
        const STATUS_CHANGE = 1 << 3;
        // 4 to 7 are unused
    }
}

bitflags! {
    /// Line status flags
    struct LineStsFlags: u8 {
        const INPUT_FULL = 1;
        // 1 to 4 unknown
        const OUTPUT_EMPTY = 1 << 5;
        // 6 and 7 unknown
    }
}

/// UART driver
pub struct Uart {
    /// UART MMIO base address
    base_address: usize,
}

impl Uart {
    /// Creates a new UART interface on the given memory mapped address.
    ///
    /// This function is unsafe because the caller must ensure that the given base address
    /// really points to a serial port device.
    #[rustversion::attr(since(1.61), const)]
    pub unsafe fn new(base: usize) -> Self {
        Self { base_address: base }
    }

    /// Initializes the memory-mapped UART.
    ///
    /// The default configuration of [38400/8-N-1](https://en.wikipedia.org/wiki/8-N-1) is used.
    pub fn init(&mut self) {
        let reg = self.base_address as *mut u8;

        unsafe {
            // Disable Interrupt
            reg.add(IER).write_volatile(0x00);

            // Enable DLAB
            // Enter a setting mode to set baud rate
            reg.add(LCR).write_volatile(0x80);

            // Set baud rate to 38.4K, other value may be valid
            // but here just copy xv6 behaviour
            reg.add(0).write_volatile(0x09);
            reg.add(1).write_volatile(0x00);

            // Disable DLAB and set data word length to 8 bits
            // Leave setting mode
            reg.add(LCR).write_volatile(0x03);

            // Enable FIFO, clear TX/RX queues and
            // set interrupt watermark at 14 bytes
            reg.add(FCR).write_volatile(0xC7);

            // Mark data terminal ready, signal request to send
            // and enable auxilliary output #2 (used as interrupt line for CPU)
            reg.add(MCR).write_volatile(0x0B);

            // Enable interrupts now
            reg.add(IER).write_volatile(0x01);
        }
    }

    fn line_sts(&mut self) -> LineStsFlags {
        let ptr = self.base_address as *mut u8;
        unsafe { LineStsFlags::from_bits_truncate(ptr.add(LSR).read_volatile()) }
    }

    /// Sends a byte on the serial port.
    pub fn send(&mut self, c: u8) {
        let ptr = self.base_address as *mut u8;
        unsafe {
            match c {
                8 | 0x7F => {
                    wait_for!(self.line_sts().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.add(THR).write_volatile(8);
                    wait_for!(self.line_sts().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.add(THR).write_volatile(b' ');
                    wait_for!(self.line_sts().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.add(THR).write_volatile(8);
                }
                _ => {
                    // Wait until previous data is flushed
                    wait_for!(self.line_sts().contains(LineStsFlags::OUTPUT_EMPTY));
                    // Write data
                    ptr.add(THR).write_volatile(c);
                }
            }
        }
    }

    /// Receives a byte on the serial port.
    pub fn receive(&mut self) -> u8 {
        let ptr = self.base_address as *mut u8;
        unsafe {
            wait_for!(self.line_sts().contains(LineStsFlags::INPUT_FULL));
            ptr.add(0).read_volatile()
        }
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            self.send(byte);
        }
        Ok(())
    }
}
