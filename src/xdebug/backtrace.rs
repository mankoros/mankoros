use crate::consts;
use crate::error;
use core::arch::asm;
use core::mem::size_of;

/// Returns the current frame pointer or stack base pointer
#[inline(always)]
pub fn fp() -> usize {
    let ptr: usize;
    unsafe {
        asm!("mv {}, s0", out(reg) ptr);
    }
    ptr
}

/// Returns the current link register.or return address
#[inline(always)]
pub fn lr() -> usize {
    let ptr: usize;
    unsafe {
        asm!("mv {}, ra", out(reg) ptr);
    }
    ptr
}

pub fn backtrace() {
    unsafe {
        let mut current_pc = lr();
        let mut current_fp = fp();
        let mut stack_num = 0;

        error!("");
        error!("=============== BEGIN BACKTRACE ================");

        loop {
            error!(
                "#{:02} PC: {:#018X} FP: {:#018X}",
                stack_num,
                current_pc - 4,
                current_fp
            );
            stack_num = stack_num + 1;
            current_fp = *(current_fp as *const usize).offset(-2);
            current_pc = *(current_fp as *const usize).offset(-1);
            if current_fp == 0 || current_fp % consts::PAGE_SIZE == 0 {
                error!(
                    "#{:02} PC: {:#018X} FP: {:#018X}",
                    stack_num,
                    current_pc - size_of::<usize>(),
                    current_fp
                );
                break;
            }
        }

        error!("=============== END BACKTRACE ================");
        error!("");
    }
}
