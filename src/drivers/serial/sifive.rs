//! Copyright (c) 2023 Easton Man
//! Adapted from https://github.com/riscv-software-src/opensbi/blob/master/lib/utils/serial/sifive-uart.c

use core::fmt::Write;

use super::{wait_for, UartDriver};

/// Sifive uart
pub struct SifiveUart {
    base_address: usize,
    in_freq: usize,
    baud_rate: usize,
}

/// https://docs.tockos.org/src/sifive/uart.rs.html
const TXFIFO: usize = 0;
const RXFIFO: usize = 1;
const TXCTRL: usize = 2;
const RXCTRL: usize = 3;
const IE: usize = 4;
const IP: usize = 5;
const DIV: usize = 6;

const TXFIFO_FULL: u32 = 1 << 31;
const RXFIFO_EMPTY: u32 = 1 << 31;
const RXFIFO_DATA: u32 = 0xff;
const TXCTRL_TXEN: u32 = 0x1;
const RXCTRL_RXEN: u32 = 0x1;

impl SifiveUart {
    pub fn new(base_address: usize, in_freq: usize) -> SifiveUart {
        SifiveUart {
            base_address,
            in_freq,
            baud_rate: 115200,
        }
    }

    pub fn init(&mut self) {
        let reg = self.base_address as *mut u32;
        unsafe {
            // Configure baud rate
            reg.add(DIV)
                .write_volatile(min_clk_divisor(self.in_freq, self.baud_rate) as u32);

            // Disable interrupts
            reg.add(IE).write_volatile(0);

            // Enable TX
            reg.add(TXCTRL).write_volatile(TXCTRL_TXEN);

            // Enable RX
            reg.add(RXCTRL).write_volatile(RXCTRL_RXEN);
        }
    }

    pub fn putc(&mut self, c: u8) {
        let reg = self.base_address as *mut u32;
        unsafe {
            wait_for!((reg.read_volatile() & TXFIFO_FULL) == 0);
            reg.add(TXFIFO).write_volatile(c as u32);
        }
    }
    pub fn getc(&mut self) -> u8 {
        let reg = self.base_address as *mut u32;
        unsafe {
            wait_for!((reg.read_volatile() & RXFIFO_EMPTY) == 0);
            reg.add(RXFIFO).read_volatile() as u8
        }
    }
}

impl UartDriver for SifiveUart {
    fn init(&mut self) {
        self.init()
    }
    fn putchar(&mut self, byte: u8) {
        self.putc(byte)
    }
    fn getchar(&mut self) -> Option<u8> {
        Some(self.getc())
    }
}

impl Write for SifiveUart {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            self.putc(byte);
        }
        Ok(())
    }
}

fn min_clk_divisor(in_freq: usize, max_target_freq: usize) -> usize {
    let quotient = (in_freq + max_target_freq - 1) / max_target_freq;

    // Avoid underflow
    if quotient == 0 {
        0
    } else {
        quotient - 1
    }
}
