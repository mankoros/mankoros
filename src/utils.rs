use core::fmt;
use core::sync::atomic::Ordering;

use crate::{here, DEVICE_REMAPPED};

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::utils::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let remapped = DEVICE_REMAPPED.load(Ordering::SeqCst);
    if remapped {
        crate::UART0.lock(here!()).write_fmt(args).unwrap();
    } else {
        crate::EARLY_UART.lock(here!()).write_fmt(args).unwrap();
    }
}
