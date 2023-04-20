use crate::consts;
use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

use log::{trace, warn};

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);

impl PhysAddr {
    pub fn page_num_down(&self) -> PhysPageNum {
        PhysPageNum(self.0 / consts::PAGE_SIZE)
    }
    pub fn page_num_up(&self) -> PhysPageNum {
        PhysPageNum::from(self.page_num_down() + 1)
    }
    pub fn round_down(&self) -> PhysAddr {
        PhysAddr(self.0 & !consts::PAGE_MASK)
    }
    pub fn round_up(&self) -> PhysAddr {
        PhysAddr(self.0 & !consts::PAGE_MASK + consts::PAGE_SIZE)
    }
    pub fn page_offset(&self) -> usize {
        self.0 & consts::PAGE_MASK
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.0 as *const u8
    }
    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.0 as *mut u8
    }
}

// + offset, - offset for PhysAddr
// += offset, -= offset for PhysAddr
// PhysAddr - PhysAddr for offset
impl const Add<usize> for PhysAddr {
    type Output = Self;
    #[inline]
    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}
impl const AddAssign<usize> for PhysAddr {
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}
impl const Sub<usize> for PhysAddr {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: usize) -> Self {
        Self(self.0 - rhs)
    }
}
impl const SubAssign<usize> for PhysAddr {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}
impl const Sub<PhysAddr> for PhysAddr {
    type Output = usize;
    #[inline]
    fn sub(self, rhs: PhysAddr) -> usize {
        self.0 - rhs.0
    }
}

// debug fmt for PhysAddr
impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}
impl fmt::LowerHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}
impl fmt::UpperHex for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#X}", self.0))
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(pub usize);

// + offset, - offset for PPN
// += offset, -= offset for PPN
// PPN - PPN for offset
impl const Add<usize> for PhysPageNum {
    type Output = Self;
    #[inline]
    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}
impl const AddAssign<usize> for PhysPageNum {
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}
impl const Sub<usize> for PhysPageNum {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: usize) -> Self {
        Self(self.0 - rhs)
    }
}
impl const SubAssign<usize> for PhysPageNum {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}
impl const Sub<PhysPageNum> for PhysPageNum {
    type Output = usize;
    #[inline]
    fn sub(self, rhs: PhysPageNum) -> usize {
        self.0 - rhs.0
    }
}

// debug fmt for PPN
impl fmt::Debug for PhysPageNum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("PPN:{:#x}", self.0))
    }
}
impl fmt::LowerHex for PhysPageNum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("PPN:{:#x}", self.0))
    }
}
impl fmt::UpperHex for PhysPageNum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("PPN:{:#X}", self.0))
    }
}

// conversions between usize, PhysAddr, PPN:
//      usize <-> PhysAddr <-> PhysPageNum -> usize
impl From<PhysAddr> for usize {
    fn from(v: PhysAddr) -> Self {
        v.0
    }
}
impl From<usize> for PhysAddr {
    fn from(v: usize) -> Self {
        Self(v & ((1 << consts::PA_WIDTH_SV39) - 1))
    }
}

impl From<PhysAddr> for PhysPageNum {
    fn from(v: PhysAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.page_num_down()
    }
}
impl From<PhysPageNum> for PhysAddr {
    fn from(v: PhysPageNum) -> Self {
        Self(v.0 << consts::PAGE_SIZE_BITS)
    }
}

impl From<PhysPageNum> for usize {
    fn from(v: PhysPageNum) -> Self {
        v.0
    }
}
