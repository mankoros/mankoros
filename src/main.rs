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
mod process;
mod tools;

use driver::uart::Uart;
use log::{error, info, trace};
use memory::frame;
use memory::heap_allocator::init_heap;
use memory::pagetable::pte::PTEFlags;
use riscv::register::satp;
use sync::SpinNoIrqLock;

use consts::address_space;
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
static KERNAL_REMAPPED: AtomicBool = AtomicBool::new(false);

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

    // Test ebreak
    unsafe {
        riscv::asm::ebreak();
    }

    remap_kernel();

    loop {}

    // Shutdown
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);

    unreachable!();
}

fn remap_kernel() {
    let mut kernal_page_table = memory::pagetable::pagetable::PageTable::new();
    // Map current position
    kernal_page_table.map_region(
        (memlayout::kernel_start as usize).into(),
        (memlayout::kernel_start as usize).into(),
        memlayout::kernel_end as usize - memlayout::kernel_start as usize,
        PTEFlags::R | PTEFlags::W | PTEFlags::X,
    );
    // Map new position
    kernal_page_table.map_region(
        (memlayout::kernel_start as usize + address_space::K_SEG_VIRT_MEM_BEG).into(),
        (memlayout::kernel_start as usize).into(),
        memlayout::kernel_end as usize - memlayout::kernel_start as usize,
        PTEFlags::R | PTEFlags::W | PTEFlags::X,
    );
    // Map physical memory
    kernal_page_table.map_region(
        address_space::K_SEG_PHY_MEM_BEG.into(),
        consts::PHYMEM_START.into(),
        consts::MAX_PHYSICAL_MEMORY,
        PTEFlags::R | PTEFlags::W,
    );

    // Map devices
    kernal_page_table.map_page(
        (memlayout::UART0_BASE + address_space::K_SEG_HARDWARE_BEG).into(),
        memlayout::UART0_BASE.into(),
        PTEFlags::R | PTEFlags::W,
    );

    // Enable paging
    unsafe {
        riscv::register::satp::set(
            satp::Mode::Sv39,
            0,
            kernal_page_table.root_paddr().page_num_down().into(),
        );
        riscv::asm::sfence_vma_all();
    }

    // Jump to new position
    kernel_jump();

    // Set KERNEL_REMAPPED
    KERNAL_REMAPPED.store(true, Ordering::SeqCst);

    // Set new S-Mode trap vector
    // TODO: This should be done after disabling interrupts
    interrupt::trap::init();

    loop {}

    // Unmap old position
    kernal_page_table.unmap_region(
        (memlayout::kernel_start as usize).into(),
        memlayout::kernel_end as usize - memlayout::kernel_start as usize,
    );

    // Avoid drop
    mem::forget(kernal_page_table);
}

#[naked]
#[no_mangle]
fn kernel_jump() {
    unsafe {
        core::arch::asm!(
            "
            li      t1, {offset}
            add     ra, t1, ra
            add     sp, t1, sp
            ret
        ",
            offset = const address_space::K_SEG_VIRT_MEM_BEG,
            options(noreturn),
        );
    }
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
