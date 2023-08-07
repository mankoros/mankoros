use crate::executor::hart_local::get_curr_lproc;
use crate::executor::hart_local::{get_curr_fp_belong_to, set_curr_fp_belong_to};
use core::arch::asm;
use riscv::register::fcsr::{RoundingMode, FCSR};
use riscv::register::sstatus;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FloatContext {
    pub fx: [usize; 32],
    pub fcsr: FCSR, // 32bit
    // because of repr(C), use u8 instead of bool
    /// 1 when the content of regiters don't match context,
    /// otherwise 0.
    pub need_load: u8,
}

fn default_fcsr() -> FCSR {
    // exception when NV(invalid operation)
    let fflags: u32 = 0b10000;
    let rm = RoundingMode::RoundToNearestEven as u32;
    let fr = rm << 4 | fflags;
    unsafe { core::mem::transmute(fr) }
}

impl FloatContext {
    pub fn init_user(&mut self) {
        self.fcsr = default_fcsr();
        self.need_load = 1;
    }
}

pub(super) fn fp_ctx_user_to_kernel() {
    // Ui -> K 时:
    // - 检查 fs, 若不为 clean, 则 Ui->need-load = true
    let curr_lproc = get_curr_lproc().unwrap();
    let fp_ctx = &mut curr_lproc.context().fp_ctx;

    let fs = sstatus::read().fs();
    if fs != sstatus::FS::Clean {
        fp_ctx.need_load = 1;
    }
}

pub(super) fn fp_ctx_kernel_to_user() {
    // K -> Uj 时:
    // - 若 HartLocal->curr-fp-reg-belong-to == i 且不为 j:
    //       - 如果同时 Ui->need-load 为 true, 则 Ui->Ctx = HartLocal->Reg
    //       - HartLocal->Reg = Uj->Ctx, HartLocal->curr-fp-reg-belong-to = i
    //       - 将 fs 设为 clean
    // 注意如果 i = j, 则 fs 的状态不变
    // 初始化时可认为 HartLocal->curr-fp-reg-belong-to = -1 (与任何东西都不相等)
    // 在 Rust 代码中, 用 None 和 Some(i) 表示 curr-fp-reg-belong-to

    let curr_lproc = get_curr_lproc().unwrap();
    let fp_lproc_opt = get_curr_fp_belong_to();

    // 判断 当前寄存器的内容 是否等价于 待切换的进程的浮点上下文内容
    let not_same = match &fp_lproc_opt {
        Some(fp_lproc) => fp_lproc.id() != curr_lproc.id(),
        None => true,
    };

    let curr_fp_ctx = &mut curr_lproc.context().fp_ctx;
    if not_same {
        // 如果不相同, 则需要发生上下文切换
        if let Some(fp_lproc) = fp_lproc_opt {
            // 如果现在寄存器的内容属于某个旧的进程, 并且它是脏的,
            // 则需要先将寄存器的内容写回到旧的进程的浮点上下文中
            let old_fp_ctx = &mut fp_lproc.context().fp_ctx;
            if old_fp_ctx.need_load == 1 {
                unsafe { sync_ctx_from_reg(old_fp_ctx) };
                old_fp_ctx.need_load = 0;
            }
        }
        // 然后需要将当前进程的浮点上下文内容写入到寄存器中
        unsafe { sync_ctx_to_reg(curr_fp_ctx) };
        // 同时维护当前寄存器中的内容属于哪个进程
        set_curr_fp_belong_to(get_curr_lproc().unwrap());
        // 并且重置 FS
        unsafe { sstatus::set_fs(sstatus::FS::Clean) };
    }
}

/// ctx = reg
unsafe fn sync_ctx_from_reg(fp_ctx: &mut FloatContext) {
    let mut _t: usize = 1; // alloc a register but not zero.
    asm!("
            fsd  f0,  0*8({0})
            fsd  f1,  1*8({0})
            fsd  f2,  2*8({0})
            fsd  f3,  3*8({0})
            fsd  f4,  4*8({0})
            fsd  f5,  5*8({0})
            fsd  f6,  6*8({0})
            fsd  f7,  7*8({0})
            fsd  f8,  8*8({0})
            fsd  f9,  9*8({0})
            fsd f10, 10*8({0})
            fsd f11, 11*8({0})
            fsd f12, 12*8({0})
            fsd f13, 13*8({0})
            fsd f14, 14*8({0})
            fsd f15, 15*8({0})
            fsd f16, 16*8({0})
            fsd f17, 17*8({0})
            fsd f18, 18*8({0})
            fsd f19, 19*8({0})
            fsd f20, 20*8({0})
            fsd f21, 21*8({0})
            fsd f22, 22*8({0})
            fsd f23, 23*8({0})
            fsd f24, 24*8({0})
            fsd f25, 25*8({0})
            fsd f26, 26*8({0})
            fsd f27, 27*8({0})
            fsd f28, 28*8({0})
            fsd f29, 29*8({0})
            fsd f30, 30*8({0})
            fsd f31, 31*8({0})
            csrr {1}, fcsr
            sw  {1}, 32*8({0})
        ", in(reg) fp_ctx,
        inout(reg) _t
    );
}

/// reg = ctx
unsafe fn sync_ctx_to_reg(fp_ctx: &mut FloatContext) {
    asm!("
        fld  f0,  0*8({0})
        fld  f1,  1*8({0})
        fld  f2,  2*8({0})
        fld  f3,  3*8({0})
        fld  f4,  4*8({0})
        fld  f5,  5*8({0})
        fld  f6,  6*8({0})
        fld  f7,  7*8({0})
        fld  f8,  8*8({0})
        fld  f9,  9*8({0})
        fld f10, 10*8({0})
        fld f11, 11*8({0})
        fld f12, 12*8({0})
        fld f13, 13*8({0})
        fld f14, 14*8({0})
        fld f15, 15*8({0})
        fld f16, 16*8({0})
        fld f17, 17*8({0})
        fld f18, 18*8({0})
        fld f19, 19*8({0})
        fld f20, 20*8({0})
        fld f21, 21*8({0})
        fld f22, 22*8({0})
        fld f23, 23*8({0})
        fld f24, 24*8({0})
        fld f25, 25*8({0})
        fld f26, 26*8({0})
        fld f27, 27*8({0})
        fld f28, 28*8({0})
        fld f29, 29*8({0})
        fld f30, 30*8({0})
        fld f31, 31*8({0})
        lw  {0}, 32*8({0})
        csrw fcsr, {0}", 
        in(reg) fp_ctx
    );
}
