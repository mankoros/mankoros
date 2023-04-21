use riscv::register::{
    scause::{self, Exception, Trap},
    sepc, stval,
};

use log::{error, info, warn};

use crate::arch;

#[no_mangle]
pub fn kernel_default_exception(a0: usize) {
    let sepc = sepc::read();
    let _stval = stval::read();

    let exception = match scause::read().cause() {
        Trap::Exception(e) => e,
        Trap::Interrupt(i) => panic!("should kernel_exception but {:?}", i),
    };
    match exception {
        Exception::InstructionMisaligned => {
            warn!(
                "Instruction misaligned exception, should not happen on machine with C-extension"
            );
            fatal_exception_error(a0)
        }
        Exception::InstructionFault => fatal_exception_error(a0),
        Exception::IllegalInstruction => {}
        Exception::Breakpoint => breakpoint_handler(sepc),
        Exception::LoadFault => fatal_exception_error(a0),
        Exception::StoreMisaligned => fatal_exception_error(a0),
        Exception::StoreFault => fatal_exception_error(a0),
        Exception::UserEnvCall => todo!(),
        Exception::InstructionPageFault => fatal_exception_error(a0),
        _e @ (Exception::LoadPageFault | Exception::StorePageFault) => {
            todo!()
        }
        _ => fatal_exception_error(a0),
    }
}

fn fatal_exception_error(_a0: usize) -> ! {
    let sepc = sepc::read();

    error!(
        "kernel fatal_exception_error! {:?} bad addr = {:#x}, sepc = {:#x}, hart = {} sp = {:#x}",
        scause::read().cause(),
        stval::read(),
        sepc,
        arch::get_hart_id(),
        arch::sp()
    );
    panic!()
}

// Software break point handler
fn breakpoint_handler(mut sepc: usize) {
    info!("Breakpoint hit");
    sepc = next_sepc(sepc);
    sepc::write(sepc);
}

fn next_instruction_sepc(sepc: usize, ir: u8) -> usize {
    if ir & 0b11 == 0b11 {
        sepc + 4
    } else {
        sepc + 2 //  RVC extend: Compressed Instructions
    }
}

/// 读取指令来判断它是2字节还是4字节
fn next_sepc(sepc: usize) -> usize {
    let ir = unsafe { *(sepc as *const u8) };
    next_instruction_sepc(sepc, ir)
}
