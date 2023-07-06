use crate::arch;
use crate::consts::memlayout::{text_end, text_start};
use core::mem::size_of;
use log::error;

pub fn backtrace() {
    unsafe {
        let mut current_pc = arch::lr();
        let mut current_fp = arch::fp();
        let mut stack_num = 0;

        error!("");
        error!("=============== BEGIN BACKTRACE ================");

        while current_pc >= text_start as usize
            && current_pc <= text_end as usize
            && current_fp != 0
        {
            error!(
                "#{:02} PC: {:#018X} FP: {:#018X}",
                stack_num,
                current_pc - size_of::<usize>(),
                current_fp
            );
            stack_num += 1;
            current_fp = *(current_fp as *const usize).offset(-2);
            current_pc = *(current_fp as *const usize).offset(-1);
        }

        error!("=============== END BACKTRACE ================");
        error!("");
    }
}
