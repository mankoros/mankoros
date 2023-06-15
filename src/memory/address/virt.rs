use crate::consts;
use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};
use super::{impl_arithmetic_with_usize, impl_usize_convert, impl_fmt};

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr4K(usize);

impl VirtAddr4K {
    pub const fn bits(self) -> usize {
        self.0
    }
    pub const fn from(bits: usize) -> Self {
        debug_assert!(bits & consts::PAGE_MASK == 0);
        VirtAddr4K(bits)
    }

    pub const fn into(self) -> VirtAddr {
        VirtAddr(self.0)
    }
    pub const fn page_num(self) -> VirtPageNum {
        VirtPageNum(self.0 / consts::PAGE_SIZE)
    }

    pub unsafe fn as_page_slice(self) -> &'static [u8] {
        self.into().as_slice(consts::PAGE_SIZE)
    }
    pub unsafe fn as_mut_page_slice(self) -> &'static mut [u8] {
        self.into().as_mut_slice(consts::PAGE_SIZE)
    }
}

impl Into<VirtAddr> for VirtAddr4K {
    fn into(self) -> VirtAddr {
        self.into()
    }
}

impl Into<VirtPageNum> for VirtAddr4K {
    fn into(self) -> VirtPageNum {
        self.page_num()
    }
}

impl_arithmetic_with_usize!(VirtAddr4K);
impl_fmt!(VirtAddr4K, "VA4K");

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(usize);

impl VirtAddr {
    pub const fn page_num_down(self) -> VirtPageNum {
        VirtPageNum(self.0 / consts::PAGE_SIZE)
    }
    pub const fn page_num_up(self) -> VirtPageNum {
        self.page_num_down() + 1
    }

    pub const fn round_down(self) -> VirtAddr4K {
        VirtAddr4K(self.0 & !consts::PAGE_MASK)
    }
    pub const fn round_up(self) -> VirtAddr4K {
        #[allow(arithmetic_overflow)]
        VirtAddr4K((self.0 & !consts::PAGE_MASK) + consts::PAGE_SIZE)
    }
    pub const fn assert_4k(self) -> VirtAddr4K {
        VirtAddr4K::from(self.0)
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
        core::slice::from_raw_parts(self.as_ptr(), len)
    }
    pub unsafe fn as_mut_slice(self, len: usize) -> &'static mut [u8] {
        core::slice::from_raw_parts_mut(self.as_mut_ptr(), len)
    }
}

impl_arithmetic_with_usize!(VirtAddr);
impl_fmt!(VirtAddr, "PA");
impl_usize_convert!(VirtAddr);

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(usize);

impl VirtPageNum {
    pub const fn addr(self) -> VirtAddr {
        VirtAddr(self.0 * consts::PAGE_SIZE)
    }
}

impl_arithmetic_with_usize!(VirtPageNum);
impl_fmt!(VirtPageNum, "PPN");
impl_usize_convert!(VirtPageNum);
