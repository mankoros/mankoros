use crate::consts;
use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(pub usize);

impl VirtAddr {
    pub fn page_num_down(&self) -> VirtPageNum {
        VirtPageNum(self.0 / consts::PAGE_SIZE)
    }
    pub fn page_num_up(&self) -> VirtPageNum {
        VirtPageNum::from(self.page_num_down() + 1)
    }
    pub fn round_down(&self) -> VirtAddr {
        VirtAddr(self.0 & !consts::PAGE_MASK)
    }
    pub fn round_up(&self) -> VirtAddr {
        VirtAddr(self.0 & !consts::PAGE_MASK + consts::PAGE_SIZE)
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

    pub unsafe fn as_mut_page_slice(&self) -> &mut [u8] {
        core::slice::from_raw_parts_mut(self.as_mut_ptr(), consts::PAGE_SIZE)
    }
}

// + offset, - offset for VirtAddr
// += offset, -= offset for VirtAddr
// VirtAddr - VirtAddr for offset
impl const Add<usize> for VirtAddr {
    type Output = Self;
    #[inline]
    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}
impl const AddAssign<usize> for VirtAddr {
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}
impl const Sub<usize> for VirtAddr {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: usize) -> Self {
        Self(self.0 - rhs)
    }
}
impl const SubAssign<usize> for VirtAddr {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}
impl const Sub<VirtAddr> for VirtAddr {
    type Output = usize;
    #[inline]
    fn sub(self, rhs: VirtAddr) -> usize {
        self.0 - rhs.0
    }
}

// debug fmt for VirtAddr
impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}
impl fmt::LowerHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}
impl fmt::UpperHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#X}", self.0))
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(pub usize);

// + offset, - offset for VPN
// += offset, -= offset for VPN
// VPN - VPN for offset
impl const Add<usize> for VirtPageNum {
    type Output = Self;
    #[inline]
    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}
impl const AddAssign<usize> for VirtPageNum {
    #[inline]
    fn add_assign(&mut self, rhs: usize) {
        *self = *self + rhs;
    }
}
impl const Sub<usize> for VirtPageNum {
    type Output = Self;
    #[inline]
    fn sub(self, rhs: usize) -> Self {
        Self(self.0 - rhs)
    }
}
impl const SubAssign<usize> for VirtPageNum {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}
impl const Sub<VirtPageNum> for VirtPageNum {
    type Output = usize;
    #[inline]
    fn sub(self, rhs: VirtPageNum) -> usize {
        self.0 - rhs.0
    }
}

// debug fmt for VPN
impl fmt::Debug for VirtPageNum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VPN:{:#x}", self.0))
    }
}
impl fmt::LowerHex for VirtPageNum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VPN:{:#x}", self.0))
    }
}
impl fmt::UpperHex for VirtPageNum {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VPN:{:#X}", self.0))
    }
}

// conversions between usize, VirtAddr, VPN:
//      usize <-> VirtAddr <-> VirtPageNum -> usize
impl From<VirtAddr> for usize {
    fn from(v: VirtAddr) -> Self {
        v.0
    }
}
impl From<usize> for VirtAddr {
    fn from(v: usize) -> Self {
        Self(v & ((1 << consts::PA_WIDTH_SV39) - 1))
    }
}

impl From<VirtAddr> for VirtPageNum {
    fn from(v: VirtAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.page_num_down()
    }
}
impl From<VirtPageNum> for VirtAddr {
    fn from(v: VirtPageNum) -> Self {
        Self(v.0 << consts::PAGE_SIZE_BITS)
    }
}

impl From<VirtPageNum> for usize {
    fn from(v: VirtPageNum) -> Self {
        v.0
    }
}
