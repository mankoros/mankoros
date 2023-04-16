//! Address type infrastructure
//!

use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

use log::{trace, warn};

use crate::consts::{self, platform};

// Kernel Phy to Virt function
// Whenever kernel wants to access to a physical address
// it should use this function to translate physical address
// into virtual address.
#[inline]
pub fn phys_to_virt(addr: usize) -> usize {
    // Return if the address is obviously in HIGH address space
    if addr >= consts::address_space::K_SEG_BEG {
        warn!("Physical address 0x{:x} is in high address space", addr);
        return addr;
    }
    trace!("Kernel physical address 0x{:x} to virtual addr", addr);

    addr - consts::PHYMEM_START + consts::address_space::K_SEG_PHY_MEM_BEG
}
// Kernel Virt to Phy function
#[inline]
pub fn virt_to_phys(addr: usize) -> usize {
    // Return if the address is obviously in HIGH address space
    if addr <= consts::address_space::K_SEG_BEG {
        warn!("Virtual address 0x{:x} is in low address space", addr);
        return addr;
    }
    trace!("Kernel virtual address 0x{:x} to physical addr", addr);

    addr - consts::address_space::K_SEG_PHY_MEM_BEG + consts::PHYMEM_START
}
// Kernel Phyical device addr to Virt function
//
#[inline]
pub fn phys_dev_to_virt(addr: usize) -> usize {
    // Return if the address is obviously in HIGH address space
    if addr >= consts::address_space::K_SEG_BEG {
        warn!("Physical address 0x{:x} is in high address space", addr);
        return addr;
    }
    trace!(
        "Kernel device physical address 0x{:x} translated to virtual addr",
        addr
    );

    addr - platform::DEVICE_START + consts::address_space::K_SEG_HARDWARE_BEG
}
// Kernel Virt text to Phy address
#[inline]
pub fn virt_text_to_phys(addr: usize) -> usize {
    addr - consts::address_space::K_SEG_DATA_BEG + consts::PHYMEM_START
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysAddr(pub usize);

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtAddr(pub usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct PhysPageNum(pub usize);

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct VirtPageNum(pub usize);

// impl for PhysAddr
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
impl From<PhysPageNum> for PhysAddr {
    fn from(v: PhysPageNum) -> Self {
        Self(v.0 << consts::PAGE_SIZE_BITS)
    }
}

// impl for PhysPageNum
impl From<usize> for PhysPageNum {
    fn from(v: usize) -> Self {
        Self(v & ((1 << consts::PPN_WIDTH_SV39) - 1))
    }
}
impl Add<usize> for PhysPageNum {
    type Output = Self;
    fn add(self, rhs: usize) -> Self {
        Self(self.0 + rhs)
    }
}

impl From<PhysPageNum> for usize {
    fn from(v: PhysPageNum) -> Self {
        v.0
    }
}

impl From<PhysAddr> for PhysPageNum {
    fn from(v: PhysAddr) -> Self {
        assert_eq!(v.page_offset(), 0);
        v.page_num_down()
    }
}

// impl for VirtAddr
impl From<usize> for VirtAddr {
    fn from(v: usize) -> Self {
        Self(v & ((1 << consts::VA_WIDTH_SV39) - 1))
    }
}

impl From<VirtAddr> for usize {
    fn from(v: VirtAddr) -> Self {
        v.0
    }
}

// + - operators
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

impl SubAssign<usize> for VirtAddr {
    #[inline]
    fn sub_assign(&mut self, rhs: usize) {
        *self = *self - rhs;
    }
}

// Debug formatter print
impl fmt::Debug for PhysAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("PA:{:#x}", self.0))
    }
}

impl fmt::Debug for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
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

impl fmt::LowerHex for VirtAddr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#x}", self.0))
    }
}

impl fmt::UpperHex for VirtAddr {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_fmt(format_args!("VA:{:#X}", self.0))
    }
}
