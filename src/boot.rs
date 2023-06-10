use log::info;

/// Copyright (c) 2023 Easton Man @Mankoros
/// Copyright (c) 2022 Maturin OS
///
/// Adapted from Maturin OS
///
/// Boot time things
///
use crate::{
    memory::{address::kernel_virt_text_to_phys, pagetable::pagetable::ENTRY_COUNT},
    println,
};

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

// Boot pagetable
core::arch::global_asm!(
    "   .section .data
        .align 12
    _boot_page_table_sv39:
        # 0x00000000_00000000 -> 0x00000000 (1G, VRWXAD) for early console
        .quad (0x00000 << 10) | 0xcf
        .quad 0
        # 0x00000000_80000000 -> 0x80000000 (1G, VRWXAD)
        .quad (0x80000 << 10) | 0xcf
        .zero 8 * 507
        # 0xffffffff_80000000 -> 0x80000000 (1G, VRWXAD)
        .quad (0x80000 << 10) | 0xcf
        .quad 0
    "
);
extern "C" {
    fn _boot_page_table_sv39();
}
/// Return the physical address of the boot page table
/// usually 0x80xxxxxx
pub fn boot_pagetable() -> &'static mut [usize] {
    unsafe { core::slice::from_raw_parts_mut(boot_pagetable_paddr() as _, ENTRY_COUNT) }
}
/// Return the physical address of the boot page table
/// usually 0x80xxxxxx
pub fn boot_pagetable_paddr() -> usize {
    kernel_virt_text_to_phys(_boot_page_table_sv39 as usize)
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
    // DO NOT MODIFY a1
    core::arch::asm!(
        "   mv   tp, a0",
        "   call {set_stack}",
        "   call {set_boot_pt}",
        // jump to boot_rust_main
        "   la   t0, boot_rust_main
            li   t1, 0xffffffff00000000
            add  t0, t0, t1
            add  sp, sp, t1
            jr   t0
        ",
        set_stack   = sym set_stack,
        set_boot_pt = sym set_boot_pt,
        options(noreturn),
    )
}

#[naked]
pub unsafe extern "C" fn alt_entry(hartid: usize) -> ! {
    // DO NOT MODIFY a1
    core::arch::asm!(
        "   mv   tp, a0",
        "   call {set_stack}",
        "   call {set_boot_pt}",
        // jump to alt_rust_main
        "   la   t0, alt_rust_main
            li   t1, 0xffffffff00000000
            add  t0, t0, t1
            add  sp, sp, t1
            jr   t0
        ",
        set_stack = sym set_stack,
        set_boot_pt = sym set_boot_pt,
        options(noreturn),
    )
}

/// 设置启动栈
#[naked]
unsafe extern "C" fn set_stack(hartid: usize) {
    // DO NOT MODIFY a1
    core::arch::asm!(
        "   add  t0, a0, 1
            slli t0, t0, 18
            la   sp, {stack}
            add  sp, sp, t0
            ret
        ",
        stack = sym KERNEL_STACK,
        options(noreturn),
    )
}

/// 设置启动页表
#[naked]
unsafe extern "C" fn set_boot_pt(hartid: usize) {
    // DO NOT MODIFY a1
    core::arch::asm!(
        "   la   t0, _boot_page_table_sv39
            srli t0, t0, 12
            li   t1, 8 << 60
            or   t0, t0, t1
            csrw satp, t0
            ret
        ",
        options(noreturn),
    )
}
