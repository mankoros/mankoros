mod timespec;
mod timestat;
mod timeval;
mod tms;

pub use self::timespec::TimeSpec;
pub use self::timeval::TimeVal;
pub use self::tms::Tms;
use crate::arch;
use log::info;
use riscv::register::{sie, time};
use sbi_rt;

/// 时钟频率，和平台有关
/// 目前硬编码为 10MHz(for qemu)
pub const CLOCK_FREQ: usize = 10_000_000;
/// 每秒的时钟中断数
pub const INTERRUPT_PER_SEC: usize = 100;
/// 每微秒的时钟周期数
pub const MACHINE_TICKS_PER_USEC: usize = CLOCK_FREQ / USEC_PER_SEC;
/// 每秒有多少微秒
const USEC_PER_SEC: usize = 1_000_000;
/// 每个时钟中断占多少微秒
pub const USEC_PER_INTERRUPT: usize = USEC_PER_SEC / INTERRUPT_PER_SEC;
/// 每秒的纳秒数
pub const NSEC_PER_SEC: usize = 1_000_000_000;
/// 每个时钟周期需要多少纳秒(取整)
pub const NSEC_PER_MACHINE_TICKS: usize = NSEC_PER_SEC / CLOCK_FREQ;
/// 当 nsec 为这个特殊值时，指示修改时间为现在
pub const UTIME_NOW: usize = 0x3fffffff;
/// 当 nsec 为这个特殊值时，指示不修改时间
pub const UTIME_OMIT: usize = 0x3ffffffe;

static mut TIMER_TICK: usize = 0;

/// timer init
pub fn init() {
    unsafe {
        TIMER_TICK = 0;
        sie::set_stimer();
    }
    set_next_timer_irq();
    info!("Timer IRQ initialized");
}

/// timer api

/// 读 mtime 计时器的值
pub fn get_time() -> usize {
    time::read()
}

/// 获取毫秒格式的时间值。注意这不一定代表进程经过的时间值
pub fn get_time_ms() -> usize {
    (time::read() * 1000) / CLOCK_FREQ
}

/// 获取秒格式的时间值。注意这不一定代表进程经过的时间值
pub fn get_time_sec() -> usize {
    time::read() / CLOCK_FREQ
}

/// 获取微秒格式的时间值。注意这不一定代表进程经过的时间值
pub fn get_time_us() -> usize {
    time::read() / MACHINE_TICKS_PER_USEC
}

/// 当前时间为多少秒(浮点数格式)
pub fn get_time_f64() -> f64 {
    get_time() as f64 / CLOCK_FREQ as f64
}

/// 获取下一次中断时间
pub fn get_next_trigger() -> u64 {
    (get_time() + CLOCK_FREQ / INTERRUPT_PER_SEC).try_into().unwrap()
}

/// use for time trap
/// in timer_handler
pub fn set_next_timer_irq() {
    sbi_rt::set_timer(get_next_trigger());
}

pub fn timer_handler() {
    set_next_timer_irq();
    unsafe {
        TIMER_TICK += 1;
        if TIMER_TICK >= 100 {
            TIMER_TICK = 0;
            info!("Timer IRQ fired at hart {}", arch::get_hart_id());
        }
    }
}
