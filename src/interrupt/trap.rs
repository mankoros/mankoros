use crate::interrupt::context::UKContext;
use core::arch::global_asm;
use riscv::register::{stvec, utvec::TrapMode};

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
        fn __set_user_trap_entry();
    }

    stvec::write(__set_user_trap_entry as usize, TrapMode::Direct);
}

#[inline(always)]
unsafe fn set_kernel_trap() {
    extern "C" {
        fn __kernel_trap_vector();
    }

    stvec::write(__kernel_trap_vector as usize, TrapMode::Vectored);
}
