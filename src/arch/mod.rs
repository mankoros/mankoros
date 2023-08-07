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
pub fn switch_page_table(new_pgt_addr: usize) -> usize {
    debug_assert!(new_pgt_addr % consts::PAGE_SIZE == 0);
    log::trace!("Switching to pagetable: 0x{:x}", new_pgt_addr);
    let old_satp = riscv::register::satp::read();
    let old_pgt_addr = old_satp.ppn() << consts::PAGE_SIZE_BITS;

    // if the pagetable is the same, do nothing
    if old_pgt_addr == new_pgt_addr {
        return old_pgt_addr;
    }

    // else switch pagetable and flush tlb
    let new_pgt_ppn = new_pgt_addr >> consts::PAGE_SIZE_BITS;
    unsafe {
        use riscv::register::satp;
        satp::set(satp::Mode::Sv39, 0, new_pgt_ppn);
        riscv::asm::sfence_vma_all();
    }
    debug!("Switched to pagetable: 0x{:x}", new_pgt_addr);
    old_pgt_addr
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
