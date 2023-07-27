use core::{cell::SyncUnsafeCell, mem::MaybeUninit};
use super::get_hart_id;

struct HartLocalInfo {
    sum_cnt: usize
}

const HART_MAX: usize = 4;
static mut HART_LOCAL_INFO: [HartLocalInfo; HART_MAX] = unsafe { MaybeUninit::zeroed().assume_init() };

pub fn init_hart_local_info() {
    // need to do nothing currently
}

fn get_curr_hart_info() -> &'static mut HartLocalInfo {
    let hart_id = get_hart_id();
    debug_assert!(hart_id < HART_MAX);
    unsafe { &mut HART_LOCAL_INFO[hart_id] }
}

pub fn sum_mode_push() {
    let curr_hart_info = get_curr_hart_info();
    if curr_hart_info.sum_cnt == 0 {
        unsafe { riscv::register::sstatus::set_sum() };
    }
    curr_hart_info.sum_cnt += 1;
}

pub fn sum_mode_pop() {
    let curr_hart_info = get_curr_hart_info();
    if curr_hart_info.sum_cnt == 1 {
        unsafe { riscv::register::sstatus::clear_sum() };
    }
    curr_hart_info.sum_cnt -= 1;
}