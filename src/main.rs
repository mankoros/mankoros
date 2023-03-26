#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(naked_functions)]
#![feature(asm_const)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(const_trait_impl)]
#![feature(const_mut_refs)]
#![feature(sync_unsafe_cell)]
#![allow(dead_code)]
extern crate alloc;

use core::panic::PanicInfo;
use core::sync::atomic::AtomicUsize;
use lazy_static::lazy_static;

mod boot;
mod consts;
mod driver;
mod interrupt;
mod logging;
mod memory;
mod process;
mod sync;
mod syscall;
mod utils;
#[macro_use]
mod xdebug;

use driver::uart::Uart;
use log::{error, info};
use memory::frame;
use memory::heap_allocator::init_heap;
use sync::SpinNoIrqLock;

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
        "
            la  sp, {stack} + {stack_size}
            sd  x0, -16(sp)
            sd  x0, -8(sp)
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
    pub static ref UART0: SpinNoIrqLock<Uart> = {
        let mut port = unsafe { Uart::new(memlayout::UART0_BASE) };
        port.init();
        SpinNoIrqLock::new(port)
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
    info!("Logging initialised");
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
    frame::dealloc_frame(first_frame);

    // Get hart info
    let hart_cnt = boot::get_hart_status();
    info!("Total harts: {}", hart_cnt);

    // Initialize interrupt controller
    interrupt::trap::init();

    // Initialize timer
    interrupt::timer::init();

    unsafe {
        riscv::asm::ebreak();
    }

    // Enable paging
    let mut kernal_page_table = memory::pagetable::pagetable::PageTable::new();
    kernal_page_table.map_region(
        (memlayout::kernel_start as usize).into(),
        (memlayout::kernel_start as usize).into(),
        memlayout::kernel_end as usize - memlayout::kernel_start as usize,
        memory::pagetable::pte::PTEFlags::V
            | memory::pagetable::pte::PTEFlags::R
            | memory::pagetable::pte::PTEFlags::W
            | memory::pagetable::pte::PTEFlags::X,
    );

    kernal_page_table.map_page(
        memlayout::UART0_BASE.into(),
        memlayout::UART0_BASE.into(),
        memory::pagetable::pte::PTEFlags::V
            | memory::pagetable::pte::PTEFlags::R
            | memory::pagetable::pte::PTEFlags::W,
    );

    riscv::register::satp::write(kernal_page_table.root_paddr().into());
    unsafe {
        riscv::asm::sfence_vma_all();
    }

    loop {}

    // Shutdown
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);

    unreachable!();
}

static PANIC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if let Some(location) = info.location() {
        error!(
            "Panic at {}:{}, msg: {}",
            location.file(),
            location.line(),
            info.message().unwrap()
        );
    } else {
        if let Some(msg) = info.message() {
            error!("Panicked: {}", msg);
        } else {
            error!("Unknown panic: {:?}", info);
        }
    }

    if PANIC_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst) >= 1 {
        error!("Panicked while processing panic. Very Wrong!");
        loop {}
    }

    xdebug::backtrace();

    loop {}
}
