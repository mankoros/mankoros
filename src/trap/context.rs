use super::fp_ctx::FloatContext;
use alloc::boxed::Box;
use riscv::register::sstatus::Sstatus;

#[repr(C)]
#[derive(Clone)]
pub struct UKContext {
    // field 的顺序非常重要! 在切换上下文的汇编函数里要靠相对偏移量来正确地存放/读取上下文的!!!
    pub user_rx: [usize; 32],  // 0-31
    pub user_sepc: usize,      // 32
    pub user_sstatus: Sstatus, // 33

    pub kernel_sx: [usize; 12], // 34-45
    pub kernel_ra: usize,       // 46
    pub kernel_sp: usize,       // 47
    pub kernel_tp: usize,       // 48

    // ============非汇编保存部分============== //
    pub fp_ctx: FloatContext,
}

impl UKContext {
    pub unsafe fn new_uninit() -> Box<Self> {
        Box::new_uninit().assume_init()
    }

    pub fn init_user(
        &mut self,
        user_sp: usize,
        sepc: usize,
        sstatus: Sstatus,
        argc: usize,
        argv: usize,
        envp: usize,
    ) {
        self.user_rx[2] = user_sp;
        self.user_rx[10] = argc;
        self.user_rx[11] = argv;
        self.user_rx[12] = envp;
        self.user_sepc = sepc;
        self.user_sstatus = sstatus;
        self.fp_ctx.init_user();
    }

    pub fn set_user_pc_to_next(&mut self, curr_instruction_len: usize) {
        self.user_sepc += curr_instruction_len;
    }

    pub fn syscall_no(&self) -> usize {
        // a7 == x17
        self.user_rx[17]
    }

    pub fn syscall_args(&self) -> [usize; 6] {
        [
            self.user_rx[10],
            self.user_rx[11],
            self.user_rx[12],
            self.user_rx[13],
            self.user_rx[14],
            self.user_rx[15],
        ]
    }

    pub fn set_user_a0(&mut self, val: usize) {
        // a0 == x10
        self.user_rx[10] = val;
    }

    pub fn set_user_tp(&mut self, val: usize) {
        // tp == x4
        self.user_rx[4] = val;
    }

    pub fn set_user_sp(&mut self, val: usize) {
        // sp == x2
        self.user_rx[2] = val;
    }

    pub fn get_user_sp(&self) -> usize {
        self.user_rx[2]
    }
}
