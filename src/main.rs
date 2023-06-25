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
#![feature(map_try_insert)]
#![feature(btree_drain_filter)]
#![feature(let_chains)]
#![feature(const_convert)]
#![feature(get_mut_unchecked)] // VFS workaround
#![feature(pointer_byte_offsets)]
extern crate alloc;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::fmt::Write;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use lazy_static::lazy_static;

mod arch;
mod boot;
mod consts;
mod driver;
mod fs;
mod logging;
mod memory;
mod sync;
mod syscall;
mod utils;
#[macro_use]
mod xdebug;
mod axerrno;
mod device_tree;
mod executor;
mod lazy_init;
mod process;
mod signal;
mod timer;
mod tools;
mod trap;

use driver::EarlyConsole;
use log::{error, info, warn};
use memory::frame;
use memory::heap;
use sync::SpinNoIrqLock;

use crate::boot::boot_pagetable_paddr;
use crate::consts::address_space::K_SEG_PHY_MEM_BEG;
use crate::fs::vfs::filesystem::VfsNode;
use crate::fs::vfs::node::VfsDirEntry;
use crate::memory::address::kernel_virt_text_to_phys;
use crate::memory::pagetable;

// use trap::ticks;

// Global shared atomic varible

pub static DEVICE_REMAPPED: AtomicBool = AtomicBool::new(false);

pub static BOOT_HART_CNT: AtomicUsize = AtomicUsize::new(0);

// Early console
pub static mut EARLY_UART: EarlyConsole = EarlyConsole {};

pub static mut UART0: SpinNoIrqLock<Option<Box<dyn Write>>> = SpinNoIrqLock::new(None);

/// Boot hart rust entry point
///
///
#[no_mangle]
pub extern "C" fn boot_rust_main(boot_hart_id: usize, boot_pc: usize) -> ! {
    // Clear BSS before anything else
    boot::clear_bss();
    unsafe { consts::device::PLATFORM_BOOT_PC = boot_pc };

    // Print boot message
    boot::print_boot_msg();
    // Print current boot hart
    println!("Early SBI console initialized");
    println!("Hart {} init booting up", boot_hart_id);
    // Parse device tree
    let device_tree = device_tree::early_parse_device_tree();

    // Initial logging support
    println!("Logging initializing...");
    logging::init();
    info!("Logging initialised");

    let device_tree_size =
        humansize::SizeFormatter::new(device_tree.total_size(), humansize::BINARY);
    info!("Device tree size: {}", device_tree_size);

    info!("UART start address: {:#x}", unsafe {
        consts::device::UART0_BASE
    });
    for memory_region in device_tree.memory().regions() {
        let memory_size =
            humansize::SizeFormatter::new(memory_region.size.unwrap_or(0), humansize::BINARY);
        info!(
            "Memory start: {:#x}, size: {}",
            memory_region.starting_address as usize, memory_size
        );
    }

    // Print boot memory layout
    consts::memlayout::print_memlayout();

    // Initial memory system
    frame::init();
    // Test the physical frame allocator
    frame::test_first_frame();
    heap::init();

    // Get hart info
    let hart_cnt = boot::get_hart_status();
    info!("Total harts: {}", hart_cnt);

    // Initialize interrupt controller
    trap::trap::init();

    // Initialize timer
    // trap::timer::init();
    timer::init();

    // Test ebreak
    unsafe {
        riscv::asm::ebreak();
    }

    // Map physical memory
    pagetable::pagetable::map_kernel_phys_seg();
    info!(
        "Physical memory mapped {:#x} -> {:#x}",
        K_SEG_PHY_MEM_BEG,
        unsafe { consts::device::PHYMEM_START }
    );

    // Next stage device initialization
    device_tree::device_init();

    info!("Console switching...");
    DEVICE_REMAPPED.store(true, Ordering::SeqCst);
    info!("Console switched to UART0");

    // Start other cores
    let alt_rust_main_phys = kernel_virt_text_to_phys(boot::alt_entry as usize);
    info!("Starting other cores at 0x{:x}", alt_rust_main_phys);
    for hart_id in 0..hart_cnt {
        if hart_id != boot_hart_id {
            sbi_rt::hart_start(hart_id, alt_rust_main_phys, boot_pagetable_paddr())
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

    // Probe devices
    let hd0 = driver::probe_virtio_blk().expect("Block device not found");
    fs::init_filesystems(hd0);

    let root_dir = fs::root::get_root_dir();

    let mut test_cases = Vec::new();
    for _ in 0..64 {
        test_cases.push(VfsDirEntry::new_empty());
    }

    let test_cases_amount = root_dir
        .clone()
        .lookup("/")
        .unwrap()
        .read_dir(0, &mut test_cases[..])
        .expect("Read root dir failed");

    let test_cases = test_cases[..test_cases_amount].to_vec();

    info!("Total cases: {}", test_cases.len());
    for case in test_cases.into_iter() {
        info!("{}", case.d_name());
    }

    let run_test_case = |path: &'static str| {
        let test_case = root_dir.clone().lookup(path).expect("Read test case failed");
        process::spawn_proc_from_file(test_case);
    };

    cfg_if::cfg_if! {
        if #[cfg(debug_assertions)] {
            let cases = ["fork"];
        } else {
            let cases = [
                "getpid",
                "getppid",
                "brk",
                "open",
                "fstat",
                "uname",
                "getcwd",
                "dup",
                "dup2",
                "mkdir_",
                "fork",
                "yield",
                "clone",
                "execve",
                "chdir",
                "exit",
                "read",
                "write",
                "close",
                "mmap",
                "munmap",
                "getdents",
                "unlink",
                "wait",
                "waitpid",
                "openat",
                "pipe",
                "mount",
                "umount",
                "gettimeofday",
                "times",
                "sleep",
            ];
        }
    }

    for case_name in cases.into_iter() {
        warn!(
            "============== Running test case: {} ================",
            case_name
        );
        run_test_case(case_name);
        executor::run_until_idle();
    }

    // Shutdown
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);

    unreachable!();
}
/// Other hart rust entry point
///
///
#[no_mangle]
pub extern "C" fn alt_rust_main(hart_id: usize) -> ! {
    info!("Hart {} started at stack: 0x{:x}", hart_id, arch::fp());
    BOOT_HART_CNT.fetch_add(1, Ordering::SeqCst);

    // Initialize interrupt controller
    // trap::trap::init();
    loop {}
    unreachable!();
}

pub static PANIC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let logging_initialized = unsafe { logging::INITIALIZED.load(Ordering::SeqCst) };
    DEVICE_REMAPPED.store(false, Ordering::SeqCst);
    if PANIC_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst) >= 1 {
        error!("Panicked while processing panic. Very Wrong!");
        loop {}
    }
    if let Some(location) = info.location() {
        if logging_initialized {
            error!(
                "Panic at {}:{}, msg: {}",
                location.file(),
                location.line(),
                info.message().unwrap()
            );
        } else {
            println!(
                "Panic at {}:{}, msg: {}",
                location.file(),
                location.line(),
                info.message().unwrap()
            );
        }
    } else {
        if let Some(msg) = info.message() {
            if logging_initialized {
                error!("Panicked: {}", msg);
            } else {
                println!("Panicked: {}", msg);
            }
        } else {
            if logging_initialized {
                error!("Unknown panic: {:?}", info);
            } else {
                println!("Unknown panic: {:?}", info);
            }
        }
    }

    xdebug::backtrace();

    loop {}
}
