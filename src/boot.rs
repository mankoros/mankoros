/// Copyright (c) 2023 Easton Man @Mankoros
///
/// Boot time magic
///
use crate::{
    consts::{
        address_space::K_SEG_DTB,
        memlayout::{kernel_end, kernel_start},
        KERNEL_LINK_ADDR, PAGE_SIZE,
    },
    memory::{
        address::kernel_virt_text_to_phys,
        pagetable::{
            pagetable::{p2_index, p3_index, ENTRY_COUNT},
            pte::{PTEFlags, PageTableEntry},
        },
    },
    println,
};
use log::info;

const BOOT_MSG: &str = r"
 __  __             _               ___  ____  
|  \/  | __ _ _ __ | | _____  _ __ / _ \/ ___| 
| |\/| |/ _` | '_ \| |/ / _ \| '__| | | \___ \ 
| |  | | (_| | | | |   < (_) | |  | |_| |___) |
|_|  |_|\__,_|_| |_|_|\_\___/|_|   \___/|____/ 

";

pub fn print_boot_msg() {
    println!("{}", BOOT_MSG);
}

/// Clear BSS segment at start up
///
///
pub fn clear_bss() {
    // linker.ld symbols
    extern "C" {
        fn bss_start();
        fn bss_end();
    }
    (bss_start as usize..bss_end as usize)
        .for_each(|a| unsafe { (a as *mut u8).write_volatile(0) });
}

/// Get currect HART status
///
/// Return the hart amount
pub fn get_hart_status() -> usize {
    let mut hart_cnt = 0;
    let mut hart_id = 0;
    loop {
        let hart_status = sbi_rt::hart_get_status(hart_id);
        if hart_status.is_ok() {
            info!("Hart {} status is {:?}", hart_id, hart_status.unwrap());
            hart_cnt += 1;
            hart_id += 1;
        } else {
            break;
        }
    }
    hart_cnt
}

#[repr(C, align(4096))]
struct OnePage([u8; 4096]);
// Boot pagetable
#[link_section = ".data"]
static mut _BOOT_PAGE_TABLE_SV39: core::mem::MaybeUninit<[OnePage; 1]> =
    core::mem::MaybeUninit::uninit();

/// Boot secondary page table
/// Used to support 2M-aligned relocation
#[link_section = ".data"]
static mut _BOOT_SECOND_PAGE_TABLE_SV39: core::mem::MaybeUninit<[OnePage; 512]> =
    core::mem::MaybeUninit::uninit();

/// Return the physical address of the boot page table
/// usually 0x80xxxxxx
pub fn boot_pagetable() -> &'static mut [PageTableEntry] {
    unsafe { core::slice::from_raw_parts_mut(boot_pagetable_paddr() as _, ENTRY_COUNT) }
}
/// Return the physical address of the boot page table
/// usually 0x80xxxxxx
pub fn boot_pagetable_paddr() -> usize {
    kernel_virt_text_to_phys(unsafe { _BOOT_PAGE_TABLE_SV39.as_ptr() } as usize)
}

/// 一个核的启动栈
#[repr(C, align(4096))]
struct KernelStack([u8; 1024 * 1024]); // 1MiB stack

/// 所有核的启动栈
#[link_section = ".bss.stack"]
static mut KERNEL_STACK: core::mem::MaybeUninit<[KernelStack; 8]> =
    core::mem::MaybeUninit::uninit(); // 8 core at max

/// Assembly entry point for boot hart
///
/// call rust_main
#[naked]
#[link_section = ".text.entry"]
#[export_name = "_start"]
unsafe extern "C" fn entry(hartid: usize) -> ! {
    core::arch::asm!(
        "   auipc s2, 0",  // Set s2 to boot_pc
        "   mv    tp, a0",
        "   mv    s1, a1", // Save dtb address to s1
        "   mv    a1, s2", // Set a1 to boot_pc
        // Set a temp stack for filling page table in Rust
        "   call  {set_early_stack}",
        // Setup boot page table
        "   mv    a2, s1", // Set a2 to dtb address
        "   call  {set_boot_pt}",
        // Set boot stack
        "   mv    a0, tp",
        "   mv    a1, s2",
        "   call  {set_stack}",
        // jump to boot_rust_main
        "   la    t0, boot_rust_main",
        "   sub   t0, t0, s2", // t0 = offset of boot_rust_main
        "   li    t1, 0xffffffff80000000",
        "   add   t0, t0, t1",
        "   mv    a0, tp", // Set a0 to hartid
        "   mv    a1, s2", // Set a1 to boot_pc
        "   jr    t0",
        set_early_stack   = sym set_early_stack,
        set_boot_pt = sym setup_vm,
        set_stack =  sym set_stack,
        options(noreturn),
    )
}

#[naked]
pub unsafe extern "C" fn alt_entry(hartid: usize) -> ! {
    core::arch::asm!(
        "   mv    tp, a0",
        "   call  {set_boot_pgtbl}",
        "   call  {set_stack}",
        // jump to alt_rust_main
        "   la    t0, alt_rust_main",
        "   mv    a0, tp
            jr    t0
        ",
        set_stack = sym set_stack,
        set_boot_pgtbl = sym set_boot_pgtbl,
        options(noreturn),
    )
}

/// 设置高地址启动栈
#[naked]
unsafe extern "C" fn set_stack(hartid: usize, boot_pc: usize) {
    core::arch::asm!(
        "   add  t0, a0, 1",
        "   slli t0, t0, 20", // 1 MiB Stack Each
        "   la   sp, {stack}",
        "   sub  t1, sp, a1", // t0 = offset of stack
        "   li   t2, 0xffffffff80000000",
        "   add  sp, t1, t2",
        "   add  sp, sp, t0
            ret
        ",
        stack = sym KERNEL_STACK,
        options(noreturn),
    )
}

/// Setup physical boot stack
/// usually low address
#[naked]
unsafe extern "C" fn set_early_stack(hartid: usize, boot_pc: usize) {
    core::arch::asm!(
        "   add  t0, a0, 1",
        "   slli t0, t0, 20", // 1 MiB Stack
        "   la   t1, {stack}",
        "   la   t2, kernel_start", // symbol in linker.ld
        "   sub  t1, t1, t2", // t1 now physical stack offset
        "   add  sp, t1, a1", // boot_pc + offset
        "   ret",
        stack = sym KERNEL_STACK,
        options(noreturn),
    )
}

#[naked]
unsafe extern "C" fn set_boot_pgtbl(_hartid: usize, root_paddr: usize) {
    // Set root page table and enable paging
    // can only use safe code
    core::arch::asm!(
        "   mv   t0, a1
            srli t0, t0, 12
            li   t1, 8 << 60
            or   t0, t0, t1
            csrw satp, t0
            ret
        ",
        options(noreturn),
    );
}

/// Fill in boot page table
/// And then switch to it
unsafe extern "C" fn setup_vm(_hartid: usize, boot_pc: usize, dtb_addr: usize) {
    let boot_page_align = 1usize << 21;

    let root_offset = _BOOT_PAGE_TABLE_SV39.as_ptr() as usize - kernel_start as usize;
    let root_paddr = boot_pc + root_offset;
    let boot_pgtbl =
        unsafe { core::slice::from_raw_parts_mut(root_paddr as *mut PageTableEntry, ENTRY_COUNT) };
    let second_offset = _BOOT_SECOND_PAGE_TABLE_SV39.as_ptr() as usize - kernel_start as usize;
    let second_paddr = boot_pc + second_offset;
    // Fill second page table with zero
    // TODO: remove hard coded
    let second_pgtbl =
        unsafe { core::slice::from_raw_parts_mut(second_paddr as *mut u8, 512 * 4096) };
    second_pgtbl.fill(0);

    // Fill root page table
    for (idx, pte) in boot_pgtbl.iter_mut().enumerate() {
        // non-leaf
        *pte = PageTableEntry::new((second_paddr + idx * PAGE_SIZE).into(), PTEFlags::V);
    }

    // Map [boot_pc, boot_pc + kernel_size) -> [K_SEG_DATA_BEG, xxx)
    let kernel_size = kernel_end as usize - kernel_start as usize;
    for offset in (0..kernel_size).step_by(boot_page_align) {
        let vaddr = KERNEL_LINK_ADDR + offset;
        let paddr = boot_pc + offset;
        // High -> phy
        let high_pte = (second_paddr
            + p3_index(vaddr.into()) * PAGE_SIZE
            + p2_index(vaddr.into()) * 8) as *mut PageTableEntry;
        // Low -> phy, identical mapping
        let low_pte = (second_paddr
            + p3_index(paddr.into()) * PAGE_SIZE
            + p2_index(paddr.into()) * 8) as *mut PageTableEntry;
        let new_pte = PageTableEntry::new(
            paddr.into(),
            // Must set A & D, some hardware cannot handle A & D set
            PTEFlags::R | PTEFlags::X | PTEFlags::W | PTEFlags::V | PTEFlags::A | PTEFlags::D,
        );
        *low_pte = new_pte;
        *high_pte = new_pte;
    }

    // Map FDT to fixed DTB address
    let fixed_ftb_pte = (second_paddr
        + p3_index(K_SEG_DTB.into()) * PAGE_SIZE
        + p2_index(K_SEG_DTB.into()) * 8) as *mut PageTableEntry;
    // DTB is expected to be read-only
    // Assume DTB is less than 2 MiB
    *fixed_ftb_pte = PageTableEntry::new(dtb_addr.into(), PTEFlags::R | PTEFlags::V | PTEFlags::A);

    // Set root page table and enable paging
    // can only use safe code
    core::arch::asm!(
        "   mv   t0, {}
            srli t0, t0, 12
            li   t1, 8 << 60
            or   t0, t0, t1
            csrw satp, t0
        ",
        in (reg) root_paddr
    );
}
