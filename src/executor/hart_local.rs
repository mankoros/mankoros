use alloc::sync::Arc;

use crate::process::lproc::LightProcess;
use core::arch::asm;

pub struct HartLocalInfo {
    sum_cnt: usize,
    no_irq_cnt: usize,
    current_lproc: Option<Arc<LightProcess>>,
}

impl HartLocalInfo {
    const fn new() -> Self {
        Self {
            sum_cnt: 0,
            no_irq_cnt: 0,
            current_lproc: None,
        }
    }

    pub fn current_lproc(&self) -> Option<Arc<LightProcess>> {
        self.current_lproc.as_ref().map(Arc::clone)
    }
}

// hart local 的东西修改肯定也是 hart local 的, 不用锁
const HART_MAX: usize = 8;
static mut HART_LOCAL_INFO: [HartLocalInfo; HART_MAX] = [
    // [HartLocalInfo::new(); HART_MAX] needs impl Copy for HartLocalInfo,
    // but we can't impl Copy for HartLocalInfo because of Arc<LightProcess>
    HartLocalInfo::new(),
    HartLocalInfo::new(),
    HartLocalInfo::new(),
    HartLocalInfo::new(),
    HartLocalInfo::new(),
    HartLocalInfo::new(),
    HartLocalInfo::new(),
    HartLocalInfo::new(),
];

#[no_mangle]
#[inline(always)]
pub fn get_hart_id() -> usize {
    let mut hartid: usize;
    unsafe {
        asm!("mv {}, tp", out(reg) hartid);
    }
    hartid
}

fn get_curr_hart_info() -> &'static mut HartLocalInfo {
    let hart_id = get_hart_id();
    debug_assert!(hart_id < HART_MAX);
    unsafe { &mut HART_LOCAL_INFO[hart_id] }
}

pub fn get_curr_lproc() -> Option<Arc<LightProcess>> {
    get_curr_hart_info().current_lproc()
}

pub fn set_curr_lproc(lproc: Arc<LightProcess>) {
    get_curr_hart_info().current_lproc = Some(lproc);
}

pub fn no_irq_push() {
    let curr = get_curr_hart_info();
    if curr.no_irq_cnt == 0 {
        unsafe { riscv::register::sstatus::clear_sie() };
    }
    curr.no_irq_cnt += 1;
}

pub fn no_irq_pop() {
    let curr = get_curr_hart_info();
    if curr.no_irq_cnt == 1 {
        unsafe { riscv::register::sstatus::set_sie() };
    }
    curr.no_irq_cnt -= 1;
}

#[inline(always)]
pub fn within_no_irq<T>(f: impl FnOnce() -> T) -> T {
    // Allow acessing user vaddr
    no_irq_push();
    let ret = f();
    no_irq_pop();
    ret
}

pub struct AutoSIE;
impl AutoSIE {
    pub fn new() -> Self {
        no_irq_push();
        Self {}
    }
}
impl Drop for AutoSIE {
    fn drop(&mut self) {
        no_irq_pop();
    }
}

pub fn sum_mode_push() {
    let curr = get_curr_hart_info();
    if curr.sum_cnt == 0 {
        unsafe { riscv::register::sstatus::set_sum() };
    }
    curr.sum_cnt += 1;
}

pub fn sum_mode_pop() {
    let curr = get_curr_hart_info();
    debug_assert_ne!(curr.sum_cnt, 0);
    if curr.sum_cnt == 1 {
        unsafe { riscv::register::sstatus::clear_sum() };
    }
    curr.sum_cnt -= 1;
}

#[inline(always)]
pub fn within_sum<T>(f: impl FnOnce() -> T) -> T {
    // Allow acessing user vaddr
    sum_mode_push();
    let ret = f();
    sum_mode_pop();
    ret
}

pub struct AutoSUM;
impl AutoSUM {
    pub fn new() -> Self {
        sum_mode_push();
        Self {}
    }
}
impl Drop for AutoSUM {
    fn drop(&mut self) {
        sum_mode_pop();
    }
}
