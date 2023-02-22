#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(naked_functions)]
#![feature(asm_const)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::panic::PanicInfo;
use lazy_static::lazy_static;

mod boot;
mod consts;
mod driver;
mod logging;
mod memory;
mod sync;
mod utils;

use driver::uart::Uart;
use log::info;
use memory::frame;
use memory::heap_allocator::init_heap;
use sync::SpinLock;

use consts::memlayout;

/// Assembly entry point
///
/// Allocation a init stack, then call rust_main
#[naked]
#[no_mangle]
#[link_section = ".text.entry"]
unsafe extern "C" fn _start() -> ! {
    // 32K large init stack
    const STACK_SIZE: usize = 32 * 1024;

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

// Static memory

// Init uart, called uart0
lazy_static! {
    pub static ref UART0: SpinLock<Uart> = {
        let mut port = unsafe { Uart::new(0x1000_0000) };
        port.init();
        SpinLock::new(port)
    };
}

/// Rust entry point
///
///
#[no_mangle]
pub extern "C" fn rust_main(hart_id: usize, _device_tree_addr: usize) -> ! {
    // Clear BSS before anything else
    boot::clear_bss();
    // Print boot message
    boot::print_boot_msg();
    // Print current boot hart
    println!("Hart {} init booting up", hart_id);

    // Initial logging support
    logging::init();
    // Print boot memory layour
    memlayout::print_memlayout();

    // Initial memory system
    frame::init();
    init_heap();

    // Test the physical frame allocator
    let first_frame = frame::alloc_frame().unwrap();
    let kernel_end = memlayout::kernel_end as usize;
    assert!(first_frame == kernel_end);
    info!("First available frame: 0x{:x}", first_frame);

    // Get hart info
    let hart_cnt = boot::get_hart_status();
    info!("Total harts: {}", hart_cnt);

    // Shutdown
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);

    unreachable!();
}

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        println!(
            "Panic at {}:{}, msg: {}",
            location.file(),
            location.line(),
            info.message().unwrap()
        );
    } else {
        if let Some(msg) = info.message() {
            println!("Panicked: {}", msg);
        } else {
            println!("Unknown panic: {:?}", info);
        }
    }

    loop {}
}
