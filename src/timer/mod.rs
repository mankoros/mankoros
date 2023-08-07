mod async_sleep;
mod rusage;
mod timespec;
mod timestat;
mod timeval;
mod tms;

pub use self::rusage::Rusage;
pub use self::timespec::TimeSpec;
pub use self::timestat::TimeStat;
pub use self::timeval::TimeVal;
pub use self::tms::Tms;
use crate::{arch, consts};
pub use async_sleep::wake_after;
pub use async_sleep::with_timeout;
use log::info;
use riscv::register::{sie, time};

static mut TIMER_TICK: usize = 0;

/// timer init
pub fn init() {
    unsafe {
        TIMER_TICK = 0;
        sie::set_stimer();
    }
    set_next_timer_irq();
    info!("Timer IRQ initialized");
    async_sleep::init_sleep_queue();
}

/// timer api

/// 读 mtime 计时器的值
pub fn get_time() -> usize {
    time::read()
}

/// 获取毫秒格式的时间值。注意这不一定代表进程经过的时间值
pub fn get_time_ms() -> usize {
    (time::read() * 1000) / consts::time::clock_freq()
}

/// 获取秒格式的时间值。注意这不一定代表进程经过的时间值
pub fn get_time_sec() -> usize {
    time::read() / consts::time::clock_freq()
}

/// 获取微秒格式的时间值。注意这不一定代表进程经过的时间值
pub fn get_time_us() -> usize {
    time::read() / consts::time::machine_ticks_per_usec()
}

/// 当前时间为多少秒 (浮点数格式)
pub fn get_time_f64() -> f64 {
    get_time() as f64 / consts::time::clock_freq() as f64
}

/// 获取下一次中断时间
pub fn get_next_trigger() -> u64 {
    (get_time() + consts::time::clock_freq() / consts::time::INTERRUPT_PER_SEC)
        .try_into()
        .unwrap()
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
        if TIMER_TICK >= consts::time::INTERRUPT_PER_SEC {
            TIMER_TICK = 0;
            info!("Hart {}: +1s", arch::get_hart_id());
        }
        async_sleep::at_tick();
    }
}
