use core::arch::global_asm;

use riscv::register::{
    scause::{Exception, Interrupt, Trap},
    sscratch, sstatus, stvec,
};

use super::{context, timer};
use log::info;

global_asm!(include_str!("trap.asm"));

pub fn init() {
    extern "C" {
        fn __smode_traps();
    }
    // Ensure interrupts are disabled.
    unsafe {
        sscratch::write(0);
        stvec::write(__smode_traps as usize, stvec::TrapMode::Direct);
        // Allow Timer interrupt
        sstatus::set_sie();
    }

    info!("Interrupts enabled");
}

// Software break point handler
fn breakpoint_handler(trapframe: &mut context::TrapFrame) {
    info!("Breakpoint hit");
    trapframe.sepc += 2;
}

// Main S-Mode trap handler
#[no_mangle]
pub fn rust_trap_handler(trapframe: &mut context::TrapFrame) {
    // Dispatch to the appropriate handler
    match trapframe.scause.cause() {
        Trap::Exception(Exception::Breakpoint) => breakpoint_handler(trapframe),
        Trap::Interrupt(Interrupt::SupervisorTimer) => timer::timer_handler(),
        _ => {
            panic!(
                "Unhandled S-Mode Trap, SCAUSE: {:?}",
                trapframe.scause.cause()
            )
        }
    }
}
