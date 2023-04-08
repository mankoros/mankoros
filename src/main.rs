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

use core::mem;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use lazy_static::lazy_static;

mod boot;
mod consts;
mod driver;
mod interrupt;
mod logging;
mod memory;
mod sync;
mod syscall;
mod utils;
#[macro_use]
mod xdebug;
mod tools;

use driver::uart::Uart;
use log::{error, info, trace};
use memory::frame;
use memory::heap;
use memory::pagetable::pte::PTEFlags;
use sync::SpinNoIrqLock;

use consts::address_space;
use consts::memlayout;

use crate::memory::pagetable;

// Static memory

pub static DEVICE_REMAPPED: AtomicBool = AtomicBool::new(false);

// Init uart, called uart0
lazy_static! {
    pub static ref EARLY_UART: SpinNoIrqLock<Uart> = {
        let mut port = unsafe { Uart::new(memlayout::UART0_BASE) };
        port.init();
        SpinNoIrqLock::new(port)
    };
    pub static ref UART0: SpinNoIrqLock<Uart> = {
        let mut port =
            unsafe { Uart::new(memlayout::UART0_BASE + address_space::K_SEG_HARDWARE_BEG) };
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
    println!("Logging initializing...");
    logging::init();
    info!("Logging initialised");
    // Print boot memory layour
    memlayout::print_memlayout();

    // Initial memory system
    frame::init();
    // Test the physical frame allocator
    frame::test_first_frame();
    heap::init();

    // Get hart info
    let hart_cnt = boot::get_hart_status();
    info!("Total harts: {}", hart_cnt);

    // Initialize interrupt controller
    interrupt::trap::init();

    // Initialize timer
    interrupt::timer::init();

    // Test ebreak
    unsafe {
        riscv::asm::ebreak();
    }
    let mut kernal_page_table = memory::pagetable::pagetable::PageTable::new_with_paddr(
        (boot::boot_pagetable_paddr()).into(),
    );
    // Map physical memory
    pagetable::pagetable::map_kernel_phys_seg();
    info!("Physical memory mapped at {:#x}", consts::PHYMEM_START);
    // Map devices
    kernal_page_table.map_page(
        (memlayout::UART0_BASE + address_space::K_SEG_HARDWARE_BEG).into(),
        memlayout::UART0_BASE.into(),
        PTEFlags::R | PTEFlags::W,
    );
    info!("Console switching...");
    DEVICE_REMAPPED.store(true, Ordering::SeqCst);
    info!("Console switched to UART0");

    // Remove low memory mappings
    pagetable::pagetable::unmap_boot_seg();
    unsafe {
        riscv::asm::sfence_vma_all();
    }
    info!("Boot memory unmapped");

    // Avoid drop
    mem::forget(kernal_page_table);

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
