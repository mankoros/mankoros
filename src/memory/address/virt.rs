use super::{impl_arithmetic_with_usize, impl_fmt, impl_usize_convert};
use crate::consts;
use core::fmt;
use core::ops::{Add, AddAssign, Range, Sub, SubAssign};

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr4K(usize);

impl VirtAddr4K {
    pub const fn bits(self) -> usize {
        self.0
    }
    pub const fn from(bits: usize) -> Self {
        debug_assert!((bits & consts::PAGE_MASK) == 0);
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

impl const From<usize> for VirtAddr4K {
    fn from(bits: usize) -> Self {
        Self::from(bits)
    }
}

impl From<VirtAddr4K> for VirtAddr {
    fn from(val: VirtAddr4K) -> Self {
        val.into()
    }
}

impl From<VirtAddr4K> for VirtPageNum {
    fn from(val: VirtAddr4K) -> Self {
        val.page_num()
    }
}

impl PartialEq<usize> for VirtAddr4K {
    fn eq(&self, other: &usize) -> bool {
        self.0 == *other
    }
}

impl PartialEq<VirtAddr> for VirtAddr4K {
    fn eq(&self, other: &VirtAddr) -> bool {
        self.0 == other.0
    }
}

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
    pub const fn floor(self) -> VirtAddr4K {
        self.round_down()
    }
    pub const fn ceil(self) -> VirtAddr4K {
        VirtAddr4K((self.0 + consts::PAGE_SIZE - 1) & !consts::PAGE_MASK)
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
impl_fmt!(VirtAddr, "VA");
impl_usize_convert!(VirtAddr);

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(usize);

impl VirtPageNum {
    pub const fn addr(self) -> VirtAddr4K {
        VirtAddr4K(self.0 * consts::PAGE_SIZE)
    }
}

impl_arithmetic_with_usize!(VirtPageNum);
impl_fmt!(VirtPageNum, "VPN");
impl_usize_convert!(VirtPageNum);

pub type VirtAddrRange = Range<VirtAddr>;

#[inline(always)]
///! 用于迭代虚拟地址范围内的所有页, 如果首尾不是页对齐的就 panic
pub fn iter_vpn(range: VirtAddrRange, mut f: impl FnMut(VirtPageNum)) {
    let range = round_range_vpn(range);
    let mut vpn = range.start;
    while vpn < range.end {
        f(vpn);
        vpn += 1;
    }
}

pub fn round_range(range: VirtAddrRange) -> VirtAddrRange {
    range.start.floor().into()..range.end.ceil().into()
}
pub fn round_range_4k(range: VirtAddrRange) -> Range<VirtAddr4K> {
    range.start.floor()..range.end.ceil()
}
pub fn round_range_vpn(range: VirtAddrRange) -> Range<VirtPageNum> {
    let range = round_range_4k(range);
    range.start.page_num()..range.end.page_num()
}
