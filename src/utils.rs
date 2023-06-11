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
        unsafe { crate::EARLY_UART.write_fmt(args).unwrap() };
    }
}

/// 获取一个裸指针指向的字符串长度
///
/// 函数会从 start 往后不断读取内存，直到遇到 0 为止。
/// 所以如果字符串没有以 \0 结尾，函数就有可能读到其他内存。
pub unsafe fn get_str_len(start: *const u8) -> usize {
    let mut ptr = start as usize;
    while *(ptr as *const u8) != 0 {
        ptr += 1;
    }
    ptr - start as usize
}

/// 从一个裸指针获取一个 &str 类型
///
/// 注意这个函数没有复制字符串本身，只是换了个类型
pub unsafe fn raw_ptr_to_ref_str(start: *const u8) -> &'static str {
    let len = get_str_len(start);
    // 因为这里直接用用户空间提供的虚拟地址来访问，所以一定能连续访问到字符串，不需要考虑物理地址是否连续
    let slice = core::slice::from_raw_parts(start, len);
    if let Ok(s) = core::str::from_utf8(slice) {
        s
    } else {
        println!("not utf8 slice");
        for c in slice {
            print!("{c} ");
        }
        println!("");
        &"p"
    }
}
