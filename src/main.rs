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
#![allow(mutable_transmutes)]
#![feature(map_try_insert)]
#![feature(btree_drain_filter)]
#![feature(let_chains)]
#![feature(const_convert)]
#![feature(get_mut_unchecked)] // VFS workaround
#![feature(negative_impls)]
#![feature(pointer_byte_offsets)]
#![feature(box_into_inner)]
#![feature(async_iterator)]
#![feature(const_maybe_uninit_zeroed)]
extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;
use process::lproc::LightProcess;
use process::spawn_proc;

use core::panic::PanicInfo;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use lazy_static::lazy_static;

mod arch;
mod boot;
mod consts;
mod drivers;
mod fs;
mod logging;
mod memory;
mod sync;
mod syscall;
mod utils;
#[macro_use]
mod xdebug;
mod device_tree;
mod executor;
mod lazy_init;
mod process;
mod signal;
mod timer;
mod tools;
mod trap;

use drivers::EarlyConsole;
use log::{error, info};
use memory::frame;
use memory::heap;
use sync::SpinNoIrqLock;

use crate::boot::boot_pagetable_paddr;
use crate::consts::address_space::K_SEG_PHY_MEM_BEG;
use crate::utils::SerialWrapper;

use crate::arch::init_hart_local_info;
use crate::executor::block_on;
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
    heap::init();

    // Initialize interrupt controller
    trap::trap::init();

    // Initialize timer
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

    // Probe devices
    drivers::init_device_manager();
    let manager = drivers::get_device_manager_mut();
    manager.probe();
    manager.map_devices();
    manager.devices_init();
    info!("Device initialization complete");
    manager.enable_external_interrupts();
    info!("External interrupts enabled");

    let serial0 = manager.serials()[0].clone();
    let serial = SerialWrapper::new(serial0);
    unsafe { *UART0.lock(here!()) = Some(Box::new(serial)) };

    info!("Console switching...");
    DEVICE_REMAPPED.store(true, Ordering::SeqCst);
    info!("Console switched to UART0");

    // Start other cores
    let alt_rust_main_phys = kernel_virt_text_to_phys(boot::alt_entry as usize);
    info!("Starting other cores at 0x{:x}", alt_rust_main_phys);
    let harts = drivers::get_device_manager().bootable_cpus();
    let hart_freq = drivers::get_device_manager().cpu_freqs();
    let hart_cnt = harts.len();
    for hart_id in harts {
        if hart_id != boot_hart_id {
            sbi_rt::hart_start(hart_id, alt_rust_main_phys, boot_pagetable_paddr())
                .expect("Starting hart failed");
        }
    }
    BOOT_HART_CNT.fetch_add(1, Ordering::SeqCst);

    // Wait for all the harts to finish booting
    while BOOT_HART_CNT.load(Ordering::SeqCst) != hart_cnt {}

    info!("Total harts booted: {}", hart_cnt);
    info!(
        "Hart frequency: {:?} MHz",
        hart_freq.iter().map(|f| f / 1000000).collect::<Vec<_>>()
    );

    // Init hart local info
    init_hart_local_info();

    // Remove low memory mappings
    pagetable::pagetable::unmap_boot_seg();
    unsafe {
        riscv::asm::sfence_vma_all();
    }
    info!("Boot memory unmapped");

    fs::init_filesystems(manager.disks()[0].clone());

    // Probe prelimiary tests
    run_preliminary_test();

    #[cfg(feature = "final")]
    {
        run_final_test();
        executor::run_until_idle();
    }

    #[cfg(feature = "shell")]
    {
        process::spawn_init();
        // Loop even if nothing in queue
        // Maybe all the task is sleeping
        loop {
            executor::run_until_idle();
        }
    }

    // Shutdown
    sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);

    unreachable!();
}

/// Execute final competition tests
///
fn run_final_test() {
    let root_dir = fs::root::get_root_dir();
    let busybox = block_on(root_dir.lookup("busybox")).expect("Read busybox failed");

    let args = ["busybox", "sh", "busybox_testcode.sh"]
        .to_vec()
        .into_iter()
        .map(|s: &str| s.to_string())
        .collect::<Vec<_>>();

    // Some necessary environment variables.
    let mut envp = Vec::new();
    envp.push(String::from("LD_LIBRARY_PATH=."));
    envp.push(String::from("SHELL=/busybox"));
    envp.push(String::from("PWD=/"));
    envp.push(String::from("USER=root"));
    envp.push(String::from("MOTD_SHOWN=pam"));
    envp.push(String::from("LANG=C.UTF-8"));
    envp.push(String::from(
        "INVOCATION_ID=e9500a871cf044d9886a157f53826684",
    ));
    envp.push(String::from("TERM=vt220"));
    envp.push(String::from("SHLVL=2"));
    envp.push(String::from("JOURNAL_STREAM=8:9265"));
    envp.push(String::from("OLDPWD=/root"));
    envp.push(String::from("_=busybox"));
    envp.push(String::from("LOGNAME=root"));
    envp.push(String::from("HOME=/"));
    envp.push(String::from("PATH=/"));

    let lproc = LightProcess::new();
    lproc.clone().do_exec(busybox, args, Vec::new());
    spawn_proc(lproc);
}

fn run_preliminary_test() {
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

    let root_dir = fs::root::get_root_dir();
    for case_name in cases.into_iter() {
        let test_case = block_on(root_dir.lookup(case_name));
        if test_case.is_err() {
            break;
        }
        process::spawn_proc_from_file(test_case.unwrap());
        executor::run_until_idle();
    }
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
}

pub static PANIC_COUNT: AtomicUsize = AtomicUsize::new(0);

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Ignore interrupts
    unsafe { riscv::register::sstatus::clear_sie() };
    let logging_initialized = unsafe { logging::INITIALIZED.load(Ordering::SeqCst) };
    DEVICE_REMAPPED.store(false, Ordering::SeqCst);
    if PANIC_COUNT.fetch_add(1, core::sync::atomic::Ordering::SeqCst) >= 1 {
        error!("Panicked while processing panic. Very Wrong!");
        loop {}
    }
    if let Some(location) = info.location() {
        if logging_initialized {
            error!(
                "Hart {} panic at {}:{}, msg: {}",
                arch::get_hart_id(),
                location.file(),
                location.line(),
                info.message().unwrap()
            );
        } else {
            println!(
                "Hart {} panic at {}:{}, msg: {}",
                arch::get_hart_id(),
                location.file(),
                location.line(),
                info.message().unwrap()
            );
        }
    } else if let Some(msg) = info.message() {
        if logging_initialized {
            error!("Panicked: {}", msg);
        } else {
            println!("Panicked: {}", msg);
        }
    } else if logging_initialized {
        error!("Unknown panic: {:?}", info);
    } else {
        println!("Unknown panic: {:?}", info);
    }

    xdebug::backtrace();

    // Safe energy
    unsafe { riscv::asm::wfi() }

    loop {}
}
