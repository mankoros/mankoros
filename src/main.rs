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
extern crate alloc;

use alloc::sync::Arc;
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
mod tools;
mod trap;

use driver::uart::Uart;
use log::{error, info};
use mbr_nostd::PartitionTable;
use memory::frame;
use memory::heap;
use memory::pagetable::pte::PTEFlags;
use sync::SpinNoIrqLock;

use consts::address_space;
use consts::memlayout;

use crate::consts::platform;
use crate::fs::vfs::filesystem::Vfs;
use crate::fs::vfs::node::VfsDirEntry;
use crate::fs::{disk, partition};
use crate::memory::address::kernel_virt_text_to_phys;
use crate::memory::{kernel_phys_dev_to_virt, pagetable};

use trap::ticks;

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
    trap::timer::init();

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

    let mut disk = disk::Disk::new(hd0);

    let mbr = disk.mbr();
    let disk = Arc::new(SpinNoIrqLock::new(disk));
    let mut partitions = Vec::new();
    for entry in mbr.partition_table_entries() {
        if entry.partition_type != mbr_nostd::PartitionType::Unused {
            info!("Partition table entry: {:#x?}", entry);
            partitions.push(partition::Partition::new(
                entry.logical_block_address as u64 * disk::BLOCK_SIZE as u64,
                entry.sector_count as u64 * disk::BLOCK_SIZE as u64,
                disk.clone(),
            ))
        }
    }

    static FAT_FS: lazy_init::LazyInit<Arc<fs::fatfs::FatFileSystem>> = lazy_init::LazyInit::new();
    FAT_FS.init_by(Arc::new(fs::fatfs::FatFileSystem::new(
        partitions[0].clone(),
    )));
    FAT_FS.init();
    let main_fs = FAT_FS.clone();

    let root_dir = main_fs.root_dir();

    let mut test_cases = Vec::new();
    for _ in 0..64 {
        test_cases.push(VfsDirEntry::new_empty());
    }

    let test_cases_amount =
        root_dir.read_dir(0, &mut test_cases[..]).expect("Read root dir failed");

    let test_cases = test_cases[..test_cases_amount].to_vec();

    info!("Total cases: {}", test_cases.len());
    for case in test_cases.into_iter() {
        info!("{}", case.d_name());
    }

    let getpid = root_dir.lookup("/getpid").expect("Read getpid failed");

    // TODO: wait for VFS
    process::spawn_initproc(getpid);

    loop {
        executor::run_until_idle();
        // TODO: if no task, sleep for a time instance
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
    trap::trap::init();
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
