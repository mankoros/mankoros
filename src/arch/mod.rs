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
    unsafe {
        core::arch::asm!("sfence.vma");
        core::arch::asm!("csrw satp, {0}", in(reg) paddr);
    }
}