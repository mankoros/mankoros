use alloc::{sync::Arc, vec::Vec};

use crate::{consts, process::lproc::LightProcess, sync::SpinNoIrqLock};
use core::arch::asm;

pub struct HartLocal {
    num_intr_off: usize,
    intr_enable: bool,
    current_lproc: Option<Arc<LightProcess>>,
}

impl HartLocal {
    fn new() -> Self {
        Self {
            num_intr_off: 0,
            intr_enable: true, // interrupt is enabled after init
            current_lproc: None,
        }
    }

    pub fn get_current_lproc(&self) -> Option<Arc<LightProcess>> {
        self.current_lproc.as_ref().map(Arc::clone)
    }
}

lazy_static::lazy_static! {
    pub static ref HART_CONTEXT: Vec<SpinNoIrqLock<HartLocal>> = {
        let mut context = Vec::new();
        for _ in 0..consts::MAX_SUPPORTED_CPUS {
            context.push(SpinNoIrqLock::new(HartLocal::new()));
        }
        context
    };
}

#[no_mangle]
#[inline(always)]
pub fn get_hart_id() -> usize {
    let mut hartid: usize;
    unsafe {
        asm!("mv {}, tp", out(reg) hartid);
    }
    hartid
}

pub fn get_curr_lproc() -> Option<Arc<LightProcess>> {
    let hart_id = get_hart_id();
    let hart_context = HART_CONTEXT[hart_id].lock(here!());
    hart_context.get_current_lproc()
}

pub fn set_curr_lproc(lproc: Arc<LightProcess>) {
    let hart_id = get_hart_id();
    let mut hart_context = HART_CONTEXT[hart_id].lock(here!());
    hart_context.current_lproc = Some(lproc);
}
