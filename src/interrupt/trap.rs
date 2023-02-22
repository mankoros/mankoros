use core::arch::global_asm;

use riscv::interrupt;
use riscv::register::{scause, sepc, sscratch, stvec};

use crate::println;
use log::info;

global_asm!(include_str!("trap.asm"));

pub fn init() {
    extern "C" {
        fn __smode_traps();
    }
    // Ensure interrupts are disabled.
    unsafe {
        // interrupt::disable();
        sscratch::write(0);
        stvec::write(__smode_traps as usize, stvec::TrapMode::Direct);
        // interrupt::enable();
    }

    info!("Interrupts enabled");
}

#[no_mangle]
pub fn rust_trap_handler() -> ! {
    let cause = scause::read().cause();
    let epc = sepc::read();
    println!("trap: cause: {:?}, epc: 0x{:#x}", cause, epc);
    panic!("trap handled!");
}
