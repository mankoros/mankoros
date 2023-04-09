#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(naked_functions)]
#![feature(asm_const)]
#![feature(panic_info_message)]
#![feature(alloc_error_handler)]
#![feature(const_trait_impl)]
#![feature(const_mut_refs)]
#![feature(sync_unsafe_cell)]
#![feature(allocator_api)]
#![feature(new_uninit)]
#![allow(dead_code)]
extern crate alloc;

use core::mem;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use lazy_static::lazy_static;

mod arch;
mod boot;
mod consts;
mod driver;
mod logging;
mod memory;
mod sync;
mod syscall;
mod utils;
#[macro_use]
mod xdebug;
mod executor;
mod interrupt;
mod process;
mod tools;

use driver::uart::Uart;
use log::{error, info};
use memory::frame;
use memory::heap;
use memory::pagetable::pte::PTEFlags;
use sync::SpinNoIrqLock;

use consts::address_space;
use consts::memlayout;

use crate::memory::address::virt_text_to_phys;
use crate::memory::pagetable;

// Global shared atomic varible

pub static DEVICE_REMAPPED: AtomicBool = AtomicBool::new(false);

pub static BOOT_HART_CNT: AtomicUsize = AtomicUsize::new(0);

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

/// Boot hart rust entry point
///
///
#[no_mangle]
pub extern "C" fn boot_rust_main(boot_hart_id: usize, _device_tree_addr: usize) -> ! {
    // Clear BSS before anything else
    boot::clear_bss();
    // Print boot message
    boot::print_boot_msg();
    // Print current boot hart
    println!("Hart {} init booting up", boot_hart_id);

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
    // interrupt::timer::init();

    // Test ebreak
    // unsafe {
    //     riscv::asm::ebreak();
    // }
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

    // Start other cores
    let alt_rust_main_phys = virt_text_to_phys(boot::alt_entry as usize);
    info!("Starting other cores at 0x{:x}", alt_rust_main_phys);
    for hart_id in 0..hart_cnt {
        if hart_id != boot_hart_id {
            sbi_rt::hart_start(hart_id, alt_rust_main_phys, _device_tree_addr)
                .expect("Starting hart failed");
        }
    }
    BOOT_HART_CNT.fetch_add(1, Ordering::SeqCst);

    // Wait for all the harts to finish booting
    while BOOT_HART_CNT.load(Ordering::SeqCst) != hart_cnt {}
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
/// Other hart rust entry point
///
///
#[no_mangle]
pub extern "C" fn alt_rust_main(hart_id: usize, _device_tree_addr: usize) -> ! {
    pagetable::pagetable::enable_boot_pagetable();
    info!("Hart {} started at stack: 0x{:x}", hart_id, arch::sp());
    BOOT_HART_CNT.fetch_add(1, Ordering::SeqCst);

    // Initialize interrupt controller
    interrupt::trap::init();
    loop {}
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
