use core::arch::global_asm;

use riscv::register::{
    scause::{Exception, Interrupt, Trap},
    sscratch, sstatus, stvec,
};

use crate::arch;
use crate::syscall;

use super::{context, timer};
use log::info;

global_asm!(include_str!("trap.asm"));

pub fn init() {
    extern "C" {
        fn __smode_traps();
    }
    info!("Try enabling trap vector at 0x{:x}", __smode_traps as usize);
    let trap_vaddr = __smode_traps as usize;
    // Ensure interrupts are disabled.
    unsafe {
        sscratch::write(0);
        stvec::write(trap_vaddr, stvec::TrapMode::Direct);
        // Allow Timer interrupt
        sstatus::set_sie();
    }

    info!(
        "Interrupts enabled for hard {} at STVEC: 0x{:x}",
        arch::get_hart_id(),
        trap_vaddr
    );
}

// Software break point handler
fn breakpoint_handler(trapframe: &mut context::TrapFrame) {
    info!("Breakpoint hit");
    trapframe.sepc += 2;
}

fn syscall_handler(trapframe: &mut context::TrapFrame) {
    info!("Syscall hit");
    trapframe.sepc += 4;
    trapframe.x[10] = syscall::syscall(
        trapframe.x[17],
        [
            trapframe.x[10],
            trapframe.x[11],
            trapframe.x[12],
            trapframe.x[13],
            trapframe.x[14],
            trapframe.x[15],
        ],
    ) as usize;
}

// Main S-Mode trap handler
#[no_mangle]
pub fn rust_trap_handler(trapframe: &mut context::TrapFrame) {
    // Dispatch to the appropriate handler
    match trapframe.scause.cause() {
        Trap::Exception(Exception::Breakpoint) => breakpoint_handler(trapframe),
        Trap::Exception(Exception::UserEnvCall) => syscall_handler(trapframe),
        Trap::Interrupt(Interrupt::SupervisorTimer) => timer::timer_handler(),
        _ => {
            panic!(
                "Unhandled S-Mode Trap, SEPC: 0x{:x}, SCAUSE: {:?}, STVAL: 0x{:x}",
                trapframe.sepc,
                trapframe.scause.cause(),
                trapframe.stval,
            )
        }
    }
}
