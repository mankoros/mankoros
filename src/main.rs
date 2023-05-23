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
extern crate alloc;

use alloc::vec::Vec;
use core::mem;
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
mod executor;
mod lazy_init;
mod process;
mod signal;
mod timer;
mod tools;
mod trap;

use driver::uart::Uart;
use log::{error, info, warn};
use memory::frame;
use memory::heap;
use memory::pagetable::pte::PTEFlags;
use sync::SpinNoIrqLock;

use consts::address_space;
use consts::memlayout;

use crate::consts::platform;
use crate::fs::vfs::filesystem::VfsNode;
use crate::fs::vfs::node::VfsDirEntry;
use crate::memory::address::kernel_virt_text_to_phys;
use crate::memory::{kernel_phys_dev_to_virt, pagetable};

// use trap::ticks;

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
    trap::trap::init();

    // Initialize timer
    // trap::timer::init();
    timer::init();

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

    for reg in platform::VIRTIO_MMIO_REGIONS {
        kernal_page_table.map_region(
            kernel_phys_dev_to_virt(reg.0).into(),
            reg.0.into(),
            reg.1,
            PTEFlags::R | PTEFlags::W,
        );
    }

    info!("Console switching...");
    DEVICE_REMAPPED.store(true, Ordering::SeqCst);
    info!("Console switched to UART0");

    // Start other cores
    let alt_rust_main_phys = kernel_virt_text_to_phys(boot::alt_entry as usize);
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
            let cases = ["sleep"];
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
pub extern "C" fn alt_rust_main(hart_id: usize, _device_tree_addr: usize) -> ! {
    pagetable::pagetable::enable_boot_pagetable();
    info!("Hart {} started at stack: 0x{:x}", hart_id, arch::sp());
    BOOT_HART_CNT.fetch_add(1, Ordering::SeqCst);

    // Initialize interrupt controller
    // trap::trap::init();
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
