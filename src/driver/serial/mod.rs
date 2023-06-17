mod sifive;
mod uart8250;

pub use sifive::SifiveUart;
pub use uart8250::Uart;

use core::fmt::Write;

macro_rules! wait_for {
    ($cond:expr) => {
        while !$cond {
            core::hint::spin_loop()
        }
    };
}
pub(crate) use wait_for;

pub struct EarlyConsole;

impl Write for EarlyConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for byte in s.bytes() {
            sbi_rt::legacy::console_putchar(byte.into());
        }
        Ok(())
    }
}
