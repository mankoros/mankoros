#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(naked_functions)]
#![feature(asm_const)]
#![allow(unaligned_references)]

use core::arch::asm;
use core::panic::PanicInfo;

/// Assembly entry point
/// 
/// Allocation a init stack, then call rust_main
#[naked]
#[no_mangle]
#[link_section = ".text.entry"]
unsafe extern "C" fn _start() -> ! {
    const STACK_SIZE: usize = 4096;

    #[link_section = ".bss.stack"]
    static mut STACK: [u8; STACK_SIZE] = [0u8; STACK_SIZE];

    core::arch::asm!(
        "   la  sp, {stack} + {stack_size}
            j   rust_main
        ",
        stack_size = const STACK_SIZE,
        stack      =   sym STACK,
        options(noreturn),
    )
}

/// Rust entry point
/// 
/// 
#[no_mangle]
pub extern "C" fn rust_main(hart_id: usize, _device_tree_addr: usize) -> ! {
    // #0 core is responsible for init
    if hart_id != 0 {
        support_hart_resume(hart_id, 0);
    }
    
    panic!("正常关机")
}


/// Other core into this function
/// 
///
extern "C" fn support_hart_resume(hart_id: usize, _param: usize) {
    loop {
        unsafe { asm!("wfi") }
    }
}


/// This function is called on panic.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}