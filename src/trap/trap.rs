use crate::trap::context::UKContext;
use crate::{arch, when_debug};
use core::arch::global_asm;
use riscv::register::sstatus;
use riscv::register::{stvec, utvec::TrapMode};

use log::trace;

global_asm!(include_str!("trap.asm"));

pub fn init() {
    unsafe {
        set_kernel_trap();
        enable_irq();
    }
}

#[inline(always)]
pub fn run_user(cx: &mut UKContext) {
    extern "C" {
        #[allow(improper_ctypes)]
        fn __kernel_to_user(cx: *mut UKContext);
    }
    unsafe {
        set_user_trap();
        __kernel_to_user(cx as *mut _ as *mut _);
        set_kernel_trap();
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
unsafe fn set_kernel_trap() {
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

#[inline(always)]
unsafe fn set_kernel_user_rw_trap() {
    extern "C" {
        fn __user_rw_trap_entry();
    }
    let trap_vaddr = __user_rw_trap_entry as usize;
    stvec::write(trap_vaddr, TrapMode::Vectored);
    trace!(
        "Switch to User-RW checking mode for hart {} at STVEC: 0x{:x}",
        arch::get_hart_id(),
        trap_vaddr
    );
}

#[inline(always)]
fn will_read_fail(vaddr: usize) -> bool {
    when_debug!({
        extern "C" {
            fn __user_rw_trap_entry();
        }
        let curr_stvec = stvec::read().address();
        debug_assert!(curr_stvec == __user_rw_trap_entry as usize);
    });

    extern "C" {
        fn __try_read_user(vaddr: usize) -> bool;
    }

    unsafe { __try_read_user(vaddr) }
}

#[inline(always)]
fn will_write_fail(vaddr: usize) -> bool {
    when_debug!({
        extern "C" {
            fn __user_rw_trap_entry();
        }
        let curr_stvec = stvec::read().address();
        debug_assert!(curr_stvec == __user_rw_trap_entry as usize);
    });

    extern "C" {
        fn __try_write_user(vaddr: usize) -> bool;
    }
    unsafe { __try_write_user(vaddr) }
}
