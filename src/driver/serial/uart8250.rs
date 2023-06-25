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
#[derive(Debug)]
pub struct Uart {
    /// UART MMIO base address
    base_address: usize,
    clock_frequency: u32,
    baud_rate: u32,
    reg_io_width: usize,
    reg_shift: usize,
}

impl Uart {
    /// Creates a new UART interface on the given memory mapped address.
    ///
    /// This function is unsafe because the caller must ensure that the given base address
    /// really points to a serial port device.
    #[rustversion::attr(since(1.61), const)]
    pub unsafe fn new(
        base: usize,
        clock_frequency: usize,
        baud_rate: usize,
        reg_io_width: usize,
        reg_shift: usize,
    ) -> Self {
        Self {
            base_address: base,
            clock_frequency: clock_frequency as u32,
            baud_rate: baud_rate as u32,
            reg_io_width,
            reg_shift,
        }
    }

    /// Initializes the memory-mapped UART.
    ///
    ///
    ///
    pub fn init(&mut self) {
        match self.reg_io_width {
            1 => self.init_u8(),
            4 => self.init_u32(),
            _ => {
                panic!("Unsupported UART register width");
            }
        }
    }

    /// Sends a byte on the serial port.
    pub fn send(&mut self, c: u8) {
        match self.reg_io_width {
            1 => self.send_u8(c),
            4 => self.send_u32(c),
            _ => {
                panic!("Unsupported UART register width");
            }
        }
    }

    fn init_u8(&mut self) {
        let reg = self.base_address as *mut u8;

        unsafe {
            // Disable Interrupt
            reg.byte_add(IER << self.reg_shift).write_volatile(0x00);

            // Enable DLAB
            // Enter a setting mode to set baud rate
            reg.byte_add(LCR << self.reg_shift).write_volatile(0x80);

            // Set baud rate
            let divisor = self.clock_frequency / (16 * self.baud_rate);
            reg.byte_add(0 << self.reg_shift).write_volatile(divisor as u8);
            reg.byte_add(1 << self.reg_shift).write_volatile((divisor >> 8) as u8);

            // Disable DLAB and set data word length to 8 bits
            // Leave setting mode
            reg.byte_add(LCR << self.reg_shift).write_volatile(0x03);

            // Enable FIFO
            reg.byte_add(FCR << self.reg_shift).write_volatile(0x01);

            // No modem control
            reg.byte_add(MCR << self.reg_shift).write_volatile(0x00);

            // Enable interrupts now
            reg.byte_add(IER << self.reg_shift).write_volatile(0x01);
        }
    }

    fn init_u32(&mut self) {
        let reg = self.base_address as *mut u32;

        unsafe {
            // Disable Interrupt
            reg.byte_add(IER << self.reg_shift).write_volatile(0x00);

            // Enable DLAB
            // Enter a setting mode to set baud rate
            reg.byte_add(LCR << self.reg_shift).write_volatile(0x80);

            // Set baud rate
            let divisor = self.clock_frequency / (16 * self.baud_rate);
            reg.byte_add(0 << self.reg_shift).write_volatile(divisor & 0xff);
            reg.byte_add(1 << self.reg_shift).write_volatile((divisor >> 8) & 0xff);

            // Disable DLAB and set data word length to 8 bits
            // Leave setting mode
            reg.byte_add(LCR << self.reg_shift).write_volatile(0x03);

            // Enable FIFO
            reg.byte_add(FCR << self.reg_shift).write_volatile(0x01);

            // No modem control
            reg.byte_add(MCR << self.reg_shift).write_volatile(0x00);

            // Enable interrupts now
            reg.byte_add(IER << self.reg_shift).write_volatile(0x01);
        }
    }

    fn line_sts_u8(&mut self) -> LineStsFlags {
        let ptr = self.base_address as *mut u8;
        unsafe { LineStsFlags::from_bits_truncate(ptr.add(LSR << self.reg_shift).read_volatile()) }
    }
    fn line_sts_u32(&mut self) -> LineStsFlags {
        let ptr = self.base_address as *mut u32;
        unsafe {
            LineStsFlags::from_bits_truncate(ptr.add(LSR << self.reg_shift).read_volatile() as u8)
        }
    }

    /// Sends a byte on the serial port.
    pub fn send_u8(&mut self, c: u8) {
        let ptr = self.base_address as *mut u8;
        unsafe {
            match c {
                8 | 0x7F => {
                    wait_for!(self.line_sts_u8().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.byte_add(THR << self.reg_shift).write_volatile(8);
                    wait_for!(self.line_sts_u8().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.byte_add(THR << self.reg_shift).write_volatile(b' ');
                    wait_for!(self.line_sts_u8().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.byte_add(THR << self.reg_shift).write_volatile(8);
                }
                _ => {
                    // Wait until previous data is flushed
                    wait_for!(self.line_sts_u8().contains(LineStsFlags::OUTPUT_EMPTY));
                    // Write data
                    ptr.byte_add(THR << self.reg_shift).write_volatile(c);
                }
            }
        }
    }
    pub fn send_u32(&mut self, c: u8) {
        let ptr = self.base_address as *mut u32;
        unsafe {
            match c {
                8 | 0x7F => {
                    wait_for!(self.line_sts_u32().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.byte_add(THR << self.reg_shift).write_volatile(8);
                    wait_for!(self.line_sts_u32().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.byte_add(THR << self.reg_shift).write_volatile(b' '.into());
                    wait_for!(self.line_sts_u32().contains(LineStsFlags::OUTPUT_EMPTY));
                    ptr.byte_add(THR << self.reg_shift).write_volatile(8);
                }
                _ => {
                    // Wait until previous data is flushed
                    wait_for!(self.line_sts_u32().contains(LineStsFlags::OUTPUT_EMPTY));
                    // Write data
                    ptr.byte_add(THR << self.reg_shift).write_volatile(c.into());
                }
            }
        }
    }

    /// Receives a byte on the serial port.
    pub fn receive(&mut self) -> u8 {
        let ptr = self.base_address as *mut u8;
        unsafe {
            wait_for!(self.line_sts_u8().contains(LineStsFlags::INPUT_FULL));
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
