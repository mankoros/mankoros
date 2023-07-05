//! RISC-V Platform Level Interrupt Controller
//!
//! Controller setup helper
//!

use crate::{consts::address_space::K_SEG_DTB, memory::kernel_phys_dev_to_virt};

pub struct PLIC {
    pub base_address: usize,
    pub size: usize,
}

impl PLIC {
    pub fn new(base_address: usize, size: usize) -> PLIC {
        PLIC { base_address, size }
    }
    pub fn enable_irq(&mut self, irq: usize, ctx_id: usize) {
        let plic = kernel_phys_dev_to_virt(0xc000000) as *mut plic::Plic;

        // Setup PLIC
        let src = PLICSrcWrapper::new(irq);
        let ctx = PLICCtxWrapper::new(ctx_id);

        unsafe { (*plic).set_threshold(ctx, 0) };
        unsafe { (*plic).enable(src, ctx) };
        unsafe { (*plic).set_priority(src, 6) };
    }

    /// Return the IRQ number of the highest priority pending interrupt
    pub fn claim_irq(&mut self, ctx_id: usize) -> Option<usize> {
        let plic = kernel_phys_dev_to_virt(0xc000000) as *mut plic::Plic;
        let ctx = PLICCtxWrapper::new(ctx_id);

        let irq = unsafe { (*plic).claim(ctx) };
        irq.map(|irq| irq.get() as usize)
    }

    pub fn complete_irq(&mut self, irq: usize, ctx_id: usize) {
        let plic = kernel_phys_dev_to_virt(0xc000000) as *mut plic::Plic;
        let src = PLICSrcWrapper::new(irq);
        let ctx = PLICCtxWrapper::new(ctx_id);
        unsafe { (*plic).complete(ctx, src) };
    }
}

/// Guaranteed to have a PLIC
pub fn probe() -> PLIC {
    let device_tree = unsafe { fdt::Fdt::from_ptr(K_SEG_DTB as _).expect("Parse DTB failed") };
    let plic_reg = device_tree
        .find_compatible(&["riscv,plic0", "sifive,plic-1.0.0"])
        .unwrap()
        .reg()
        .unwrap()
        .next()
        .unwrap();

    let base_address = plic_reg.starting_address as usize;
    let size = plic_reg.size.unwrap();

    PLIC { base_address, size }
}

#[derive(Debug, Clone, Copy)]
struct PLICSrcWrapper {
    irq: usize,
}
impl PLICSrcWrapper {
    fn new(irq: usize) -> Self {
        Self { irq }
    }
}
impl plic::InterruptSource for PLICSrcWrapper {
    fn id(self) -> core::num::NonZeroU32 {
        core::num::NonZeroU32::try_from(self.irq as u32).unwrap()
    }
}

#[derive(Debug, Clone, Copy)]
struct PLICCtxWrapper {
    ctx: usize,
}
impl PLICCtxWrapper {
    fn new(ctx: usize) -> Self {
        Self { ctx }
    }
}
impl plic::HartContext for PLICCtxWrapper {
    fn index(self) -> usize {
        self.ctx
    }
}
