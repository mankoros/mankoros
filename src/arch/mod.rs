use core::arch::asm;

/// Returns the current frame pointer or stack base pointer
#[inline(always)]
pub fn fp() -> usize {
    let ptr: usize;
    unsafe {
        asm!("mv {}, s0", out(reg) ptr);
    }
    ptr
}

/// Returns the current link register or return address
#[inline(always)]
pub fn lr() -> usize {
    let ptr: usize;
    unsafe {
        asm!("mv {}, ra", out(reg) ptr);
    }
    ptr
}

/// Returns the current stack pointer
#[inline(always)]
pub fn sp() -> usize {
    let ptr: usize;
    unsafe {
        asm!("mv {}, sp", out(reg) ptr);
    }
    ptr
}
