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

#[inline(always)]
pub fn switch_page_table(paddr: usize) {
    debug!("Switching to pagetable: 0x{:x}", paddr);
    unsafe {
        riscv::register::satp::set(
            riscv::register::satp::Mode::Sv39,
            0,
            paddr >> consts::PAGE_SIZE_BITS,
        );
        riscv::asm::sfence_vma_all();
    }
    debug!("Switched to pagetable: 0x{:x}", paddr);
}

#[inline(always)]
pub fn get_curr_page_table_addr() -> usize {
    riscv::register::satp::read().ppn() << consts::PAGE_SIZE_BITS
}

#[inline(always)]
pub fn within_sum<T>(f: impl FnOnce() -> T) -> T {
    // Allow acessing user vaddr
    unsafe { riscv::register::sstatus::set_sum() };
    let ret = f();
    unsafe { riscv::register::sstatus::clear_sum() };
    ret
}
