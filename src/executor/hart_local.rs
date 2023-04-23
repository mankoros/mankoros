use alloc::vec::Vec;

use crate::{consts, sync::SpinNoIrqLock};

pub struct HartLocal {
    // TODO: something point to a task
    //
    num_intr_off: usize,
    intr_enable: bool,
}

impl HartLocal {
    fn new() -> Self {
        Self {
            num_intr_off: 0,
            intr_enable: true, // interrupt is enabled after init
        }
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
