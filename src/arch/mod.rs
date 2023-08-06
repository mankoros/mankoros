use log::debug;

use crate::consts;

/// Returns the current frame pointer or stack base pointer
#[inline(always)]
pub fn fp() -> usize {
    let ptr: usize;
    unsafe {
        core::arch::asm!("mv {}, s0", out(reg) ptr);
    }
    ptr
}

/// Returns the current link register or return address
#[inline(always)]
pub fn lr() -> usize {
    let ptr: usize;
    unsafe {
        core::arch::asm!("mv {}, ra", out(reg) ptr);
    }
    ptr
}

/// Returns the current stack pointer
#[inline(always)]
pub fn sp() -> usize {
    let ptr: usize;
    unsafe {
        core::arch::asm!("mv {}, sp", out(reg) ptr);
    }
    ptr
}

/// Hard ID is stored in tp register
#[inline]
pub fn get_hart_id() -> usize {
    let hart_id;
    unsafe { core::arch::asm!("mv {0}, tp", out(reg) hart_id) };
    hart_id
}

/// Switch to a new pagetable
/// returns the old pagetable
#[inline(always)]
pub fn switch_page_table(paddr: usize) -> usize {
    log::trace!("Switching to pagetable: 0x{:x}", paddr);
    let old_page_table_ptr = riscv::register::satp::read();
    unsafe {
        riscv::register::satp::set(
            riscv::register::satp::Mode::Sv39,
            0,
            paddr >> consts::PAGE_SIZE_BITS,
        );
        riscv::asm::sfence_vma_all();
    }
    debug!("Switched to pagetable: 0x{:x}", paddr);
    old_page_table_ptr.ppn() << consts::PAGE_SIZE_BITS
}

#[inline(always)]
pub fn get_curr_page_table_addr() -> usize {
    riscv::register::satp::read().ppn() << consts::PAGE_SIZE_BITS
}

pub fn flush_tlb(vaddr: usize) {
    unsafe { riscv::asm::sfence_vma(0, vaddr) };
}
pub fn flush_tlb_all() {
    unsafe { riscv::asm::sfence_vma_all() };
}

#[inline(never)]
pub fn spin(cycle: usize) {
    for _ in 0..cycle {
        core::hint::spin_loop();
    }
}
