use crate::consts;
use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};
use super::{impl_arithmetic_with_usize, impl_usize_convert};
use super::impl_fmt;
use super::kernel_phys_to_virt;


#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr4K(usize);

impl PhysAddr4K {
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

    pub unsafe fn as_page_slice(self) -> &'static [u8] {
        self.into().as_slice(consts::PAGE_SIZE)
    }
    pub unsafe fn as_mut_page_slice(self) -> &'static mut [u8] {
        self.into().as_mut_slice(consts::PAGE_SIZE)
    }
}

impl const From<usize> for PhysAddr4K {
    fn from(bits: usize) -> Self {
        Self::from(bits)
    }
}

impl Into<PhysAddr> for PhysAddr4K {
    fn into(self) -> PhysAddr {
        self.into()
    }
}

impl Into<PhysPageNum> for PhysAddr4K {
    fn into(self) -> PhysPageNum {
        self.page_num()
    }
}

impl_arithmetic_with_usize!(PhysAddr4K);
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

    pub const fn round_down(self) -> PhysAddr4K {
        PhysAddr4K(self.0 & !consts::PAGE_MASK)
    }
    pub const fn round_up(self) -> PhysAddr4K {
        #[allow(arithmetic_overflow)]
        PhysAddr4K((self.0 & !consts::PAGE_MASK) + consts::PAGE_SIZE)
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
    pub const fn addr(self) -> PhysAddr {
        PhysAddr(self.0 * consts::PAGE_SIZE)
    }
}

impl_arithmetic_with_usize!(PhysPageNum);
impl_fmt!(PhysPageNum, "PPN");
impl_usize_convert!(PhysPageNum);
