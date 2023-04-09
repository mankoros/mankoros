use crate::arch;
use crate::trap::context::UKContext;
use core::arch::global_asm;
use riscv::register::sstatus;
use riscv::register::{stvec, utvec::TrapMode};

use log::info;

global_asm!(include_str!("trap.asm"));

pub fn init() {
    unsafe {
        set_kernel_trap();
    }
}

#[inline(always)]
pub fn run_user(cx: &mut UKContext) {
    extern "C" {
        #[allow(improper_ctypes)]
        fn __entry_user(cx: *mut UKContext);
    }
    unsafe {
        set_user_trap();
        __entry_user(cx as *mut _ as *mut _);
        set_kernel_trap();
    }
}

#[inline(always)]
unsafe fn set_user_trap() {
    extern "C" {
        fn __user_trap_entry();
    }

    stvec::write(__user_trap_entry as usize, TrapMode::Direct);
}

#[inline(always)]
unsafe fn set_kernel_trap() {
    extern "C" {
        fn __kernel_trap_vector();
    }
    info!(
        "Try enabling trap vector at 0x{:x}",
        __kernel_trap_vector as usize
    );
    let trap_vaddr = __kernel_trap_vector as usize;
    stvec::write(trap_vaddr, TrapMode::Vectored);
    // Enable irq
    sstatus::set_sie();

    info!(
        "Interrupts enabled for hard {} at STVEC: 0x{:x}",
        arch::get_hart_id(),
        trap_vaddr
    );
}
