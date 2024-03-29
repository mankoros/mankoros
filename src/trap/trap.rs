use crate::trap::context::UKContext;
use crate::{arch, when_debug};
use core::arch::global_asm;
use riscv::register::sstatus;
use riscv::register::{stvec, utvec::TrapMode};

use crate::trap::fp_ctx::{fp_ctx_kernel_to_user, fp_ctx_user_to_kernel};
use log::trace;

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
        fn __kernel_to_user(cx: *mut UKContext);
    }
    unsafe {
        fp_ctx_kernel_to_user();
        set_user_trap();
        __kernel_to_user(cx as *mut _ as *mut _);
        set_kernel_trap();
        fp_ctx_user_to_kernel();
        enable_irq();
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
pub unsafe fn set_kernel_trap() {
    extern "C" {
        fn __kernel_trap_vector();
    }
    trace!(
        "Try enabling trap vector at 0x{:x}",
        __kernel_trap_vector as usize
    );
    let trap_vaddr = __kernel_trap_vector as usize;
    stvec::write(trap_vaddr, TrapMode::Vectored);

    trace!(
        "Interrupts enabled for hart {} at STVEC: 0x{:x}",
        arch::get_hart_id(),
        trap_vaddr
    );
}

#[inline(always)]
unsafe fn enable_irq() {
    sstatus::set_sie();
}

extern "C" {
    fn __user_rw_trap_vector();
}
#[inline(always)]
pub unsafe fn set_kernel_user_rw_trap() {
    let trap_vaddr = __user_rw_trap_vector as usize;
    stvec::write(trap_vaddr, TrapMode::Vectored);
    trace!(
        "Switch to User-RW checking mode for hart {} at STVEC: 0x{:x}",
        arch::get_hart_id(),
        trap_vaddr
    );
}

#[inline(always)]
pub fn will_read_fail(vaddr: usize) -> bool {
    when_debug!({
        let curr_stvec = stvec::read().address();
        debug_assert!(curr_stvec == __user_rw_trap_vector as usize);
    });

    extern "C" {
        fn __try_read_user(vaddr: usize) -> bool;
    }

    unsafe { __try_read_user(vaddr) }
}

#[inline(always)]
pub fn will_write_fail(vaddr: usize) -> bool {
    when_debug!({
        let curr_stvec = stvec::read().address();
        debug_assert!(curr_stvec == __user_rw_trap_vector as usize);
    });

    extern "C" {
        fn __try_write_user(vaddr: usize) -> bool;
    }
    unsafe { __try_write_user(vaddr) }
}
