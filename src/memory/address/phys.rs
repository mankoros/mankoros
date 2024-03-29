use super::impl_fmt;
use super::kernel_phys_to_virt;
use super::{impl_arithmetic_with_usize, impl_usize_convert};
use crate::consts;
use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr4K(usize);

impl PhysAddr4K {
    pub const fn is_valid(self) -> bool {
        let b = self.bits();
        (b < 0xffff_ffff_0000_0000) && (b & consts::PAGE_MASK == 0)
    }

    pub const fn bits(self) -> usize {
        self.0
    }
    pub const fn from(bits: usize) -> Self {
        debug_assert!(bits & consts::PAGE_MASK == 0);
        PhysAddr4K(bits)
    }

    pub const fn into(self) -> PhysAddr {
        PhysAddr(self.0)
    }
    pub const fn page_num(self) -> PhysPageNum {
        PhysPageNum(self.0 / consts::PAGE_SIZE)
    }

    pub unsafe fn as_slice(self, len: usize) -> &'static [u8] {
        self.into().as_slice(len)
    }
    pub unsafe fn as_mut_slice(self, len: usize) -> &'static mut [u8] {
        self.into().as_mut_slice(len)
    }

    pub unsafe fn as_page_slice(self) -> &'static [u8] {
        self.into().as_slice(consts::PAGE_SIZE)
    }
    pub unsafe fn as_mut_page_slice(self) -> &'static mut [u8] {
        self.into().as_mut_slice(consts::PAGE_SIZE)
    }

    pub const fn next_page(self) -> Self {
        Self(self.0 + consts::PAGE_SIZE)
    }
    pub const fn prev_page(self) -> Self {
        Self(self.0 - consts::PAGE_SIZE)
    }
    pub fn offset_to_next_page(&mut self) {
        self.0 += consts::PAGE_SIZE;
    }
    pub fn offset_to_prev_page(&mut self) {
        self.0 -= consts::PAGE_SIZE;
    }
}

impl From<usize> for PhysAddr4K {
    fn from(bits: usize) -> Self {
        Self::from(bits)
    }
}

impl From<PhysAddr4K> for PhysAddr {
    fn from(val: PhysAddr4K) -> Self {
        val.into()
    }
}

impl From<PhysAddr4K> for PhysPageNum {
    fn from(val: PhysAddr4K) -> Self {
        val.page_num()
    }
}

impl PartialEq<usize> for PhysAddr4K {
    fn eq(&self, other: &usize) -> bool {
        self.0 == *other
    }
}

impl PartialEq<PhysAddr> for PhysAddr4K {
    fn eq(&self, other: &PhysAddr) -> bool {
        self.0 == other.0
    }
}

impl_fmt!(PhysAddr4K, "PA4K");

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(usize);

impl PhysAddr {
    pub const fn page_num_down(self) -> PhysPageNum {
        PhysPageNum(self.0 / consts::PAGE_SIZE)
    }
    pub const fn page_num_up(self) -> PhysPageNum {
        self.page_num_down() + 1
    }

    pub const fn floor(self) -> PhysAddr4K {
        PhysAddr4K(self.0 & !consts::PAGE_MASK)
    }
    pub const fn ceil(self) -> PhysAddr4K {
        PhysAddr4K((self.0 + consts::PAGE_SIZE - 1) & !consts::PAGE_MASK)
    }
    pub const fn assert_4k(self) -> PhysAddr4K {
        PhysAddr4K::from(self.0)
    }

    pub const fn page_offset(self) -> usize {
        self.0 & consts::PAGE_MASK
    }

    pub const fn as_ptr(self) -> *const u8 {
        self.0 as *const u8
    }
    pub const fn as_mut_ptr(self) -> *mut u8 {
        self.0 as *mut u8
    }

    pub unsafe fn as_slice(self, len: usize) -> &'static [u8] {
        let mapped_addr = kernel_phys_to_virt(self.0);
        core::slice::from_raw_parts(mapped_addr as *const u8, len)
    }
    pub unsafe fn as_mut_slice(self, len: usize) -> &'static mut [u8] {
        let mapped_addr = kernel_phys_to_virt(self.0);
        core::slice::from_raw_parts_mut(mapped_addr as *mut u8, len)
    }
}

impl_arithmetic_with_usize!(PhysAddr);
impl_fmt!(PhysAddr, "PA");
impl_usize_convert!(PhysAddr);

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(usize);

impl PhysPageNum {
    pub const fn addr(self) -> PhysAddr4K {
        PhysAddr4K(self.0 * consts::PAGE_SIZE)
    }
}

impl_arithmetic_with_usize!(PhysPageNum);
impl_fmt!(PhysPageNum, "PPN");
impl_usize_convert!(PhysPageNum);
