use log::info;
use riscv::register::{sie, time};
use sbi_rt;

static TIME_INTERVAL: u64 = 100000; // TODO: Hard coded for now

pub static mut TICKS: usize = 0;

pub fn init() {
    unsafe {
        TICKS = 0;
        sie::set_stimer();
    }
    set_next_timer_irq();
    info!("Timer IRQ initialized");
}

// S-Mode timer interrupt handler
pub fn timer_handler() {
    set_next_timer_irq();
    unsafe {
        TICKS += 1;
        if TICKS == 100 {
            TICKS = 0;
            info!("Timer IRQ fired");
        }
    }
}

pub fn set_next_timer_irq() {
    // Add interval to current cycle
    // See SBI spec for details
    sbi_rt::set_timer(TIME_INTERVAL + time::read64());
}

// Read global ticks
pub fn ticks() -> usize {
    unsafe { TICKS }
}
