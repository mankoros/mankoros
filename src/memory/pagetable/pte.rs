// Copyright (c) Easton Man
//
// PTE data infrastructure
// Adpated from FTL OS
// https://gitlab.eduxiji.net/DarkAngelEX/oskernel2022-ftlos/-/blob/master/code/kernel/src/memory/page_table/mod.rs

use bitflags::bitflags;

use core::fmt;

use crate::consts;
use crate::memory::address::{PhysAddr, PhysPageNum, PhysAddr4K};
use crate::memory::frame;

// Define the PTEFlags bitflags structure
bitflags! {
    // riscv-privileged 4.3.1 P87
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct PTEFlags: u16 {
        const V = 1 << 0; // valid
        const R = 1 << 1; // readable
        const W = 1 << 2; // writable
        const X = 1 << 3; // executable
        const U = 1 << 4; // user mode
        const G = 1 << 5; // global mapping
        const A = 1 << 6; // access, set to 1 after r/w/x
        const D = 1 << 7; // dirty, set to 1 after write
        const SHARED = 1 << 8; // copy-on-write
        const RSW2 = 1 << 9; // software reserved 2
    }
}

// Implement methods for PTEFlags
impl PTEFlags {
    // Check if the flag is writable
    pub fn writable(self) -> bool {
        self.contains(Self::W)
    }
    // Check if the flag is executable
    pub fn executable(self) -> bool {
        self.contains(Self::X)
    }
    // Check if the flag is valid
    pub fn valid(self) -> bool {
        self.contains(Self::V)
    }
}

/// PTE data structure
#[derive(Copy, Clone)]
#[repr(C)]
pub struct PageTableEntry {
    bits: usize,
}

// Implement methods for PageTableEntry
impl PageTableEntry {
    // Create a new PageTableEntry with the given physical address and permissions
    pub fn new(paddr: PhysAddr4K, perm: PTEFlags) -> Self {
        PageTableEntry {
            bits: ((paddr.bits() >> 2) & consts::PTE_PPN_MASK_SV39)
                | perm.bits() as usize,
        }
    }
    // Define an empty PageTableEntry
    pub const EMPTY: Self = Self { bits: 0 };

    // Clear the PageTableEntry
    pub fn clear(&mut self) {
        *self = Self::EMPTY;
    }

    // Get the physical page number from the PageTableEntry
    pub fn ppn(&self) -> PhysPageNum {
        PhysPageNum::from((self.bits & consts::PTE_PPN_MASK_SV39) >> 10)
    }

    // Get the physical address from the PageTableEntry
    pub fn paddr(&self) -> PhysAddr4K {
        self.ppn().addr()
    }

    // Get the flags from the PageTableEntry
    pub fn flags(&self) -> PTEFlags {
        // Hardcoded form
        PTEFlags::from_bits((self.bits & consts::PTE_FLAGS_MASK) as u16)
            .expect("Convert PageTableEntry to PTEFlags failed")
    }

    // Check if the PageTableEntry is valid
    pub fn is_valid(&self) -> bool {
        self.flags().valid()
    }

    // Check if the PageTableEntry is a directory
    pub fn is_directory(&self) -> bool {
        let mask = PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U;
        self.is_valid() && (self.flags() & mask) == PTEFlags::empty()
    }

    // Check if the PageTableEntry is a leaf
    pub fn is_leaf(&self) -> bool {
        let mask = PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U;
        self.is_valid() && (self.flags() & mask) != PTEFlags::empty()
    }

    // Check if the PageTableEntry is readable
    pub fn readable(&self) -> bool {
        self.flags().contains(PTEFlags::R)
    }

    // Check if the PageTableEntry is writable
    pub fn writable(&self) -> bool {
        self.flags().contains(PTEFlags::W)
    }

    // Check if the PageTableEntry is executable
    pub fn executable(&self) -> bool {
        self.flags().contains(PTEFlags::X)
    }

    // Check if the PageTableEntry is in user mode
    pub fn is_user(&self) -> bool {
        self.flags().contains(PTEFlags::U)
    }

    // Check if the PageTableEntry is shared
    pub fn shared(&self) -> bool {
        self.flags().contains(PTEFlags::SHARED)
    }

    // Check if the RSW bit 8 is set
    pub fn rsw_8(&self) -> bool {
        (self.bits & 1usize << 8) != 0
    }

    // Check if the RSW bit 9 is set
    pub fn rsw_9(&self) -> bool {
        (self.bits & 1usize << 9) != 0
    }

    // Get the reserved bits from the PageTableEntry
    pub fn reserved_bits(&self) -> usize {
        self.bits & (((1usize << 10) - 1) << 54)
    }

    // Set the rwx flags for the PageTableEntry
    pub fn set_rwx(&mut self, flag: PTEFlags) {
        let mask = (PTEFlags::R | PTEFlags::W | PTEFlags::X).bits() as usize;
        let flag = flag.bits() as usize & mask;
        self.bits = (self.bits & !mask) | flag;
    }

    // Set the writable flag for the PageTableEntry
    pub fn set_writable(&mut self) {
        self.bits |= PTEFlags::W.bits() as usize;
    }

    // Clear the writable flag for the PageTableEntry
    pub fn clear_writable(&mut self) {
        self.bits &= !(PTEFlags::W.bits() as usize);
    }

    // Set the shared flag for the PageTableEntry
    pub fn set_shared(&mut self) {
        self.bits |= PTEFlags::SHARED.bits() as usize;
    }

    // Clear the shared flag for the PageTableEntry
    pub fn clear_shared(&mut self) {
        self.bits &= !PTEFlags::SHARED.bits() as usize;
    }

    // Make the PageTableEntry shared
    pub fn become_shared(&mut self, shared_writable: bool) {
        debug_assert!(!self.shared());
        self.set_shared();
        if !shared_writable {
            self.clear_writable();
        }
    }

    // Make the PageTableEntry unique
    pub fn become_unique(&mut self, unique_writable: bool) {
        debug_assert!(self.shared());
        self.clear_shared();
        if unique_writable {
            self.set_writable();
        }
    }

    /// Allocate a non-leaf PageTableEntry with the given permissions
    pub fn alloc_non_leaf(&mut self, perm: PTEFlags) {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        debug_assert!(!perm.intersects(PTEFlags::U | PTEFlags::R | PTEFlags::W));
        let pa = frame::alloc_frame().unwrap(); // TODO: add error checking
        *self = Self::new(pa, perm | PTEFlags::V);
    }

    /// Allocate a physical page for the PageTableEntry with the given permissions
    pub fn alloc(&mut self, perm: PTEFlags) {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        let pa = frame::alloc_frame().unwrap(); // TODO: add error checking
        *self = Self::new(pa, perm | PTEFlags::D | PTEFlags::A | PTEFlags::V);
    }

    // Map a frame to the PageTableEntry with the given permissions and physical address
    pub fn map_frame(&mut self, perm: PTEFlags, pa: PhysAddr4K) {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        *self = Self::new(pa, perm | PTEFlags::D | PTEFlags::A | PTEFlags::V);
    }

    /// Deallocate the PageTableEntry and clear the valid flag
    pub unsafe fn dealloc(&mut self) {
        debug_assert!(self.is_valid() && self.is_leaf());
        frame::dealloc_frame(self.paddr());
        *self = Self::EMPTY;
    }

    /// Deallocate the non-leaf PageTableEntry and clear the valid flag
    pub unsafe fn dealloc_non_leaf(&mut self) {
        debug_assert!(self.is_valid() && self.is_directory());
        frame::dealloc_frame(self.paddr());
        *self = Self::EMPTY;
    }
}

impl fmt::Debug for PageTableEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut f = f.debug_struct("PageTableEntry");
        f.field("raw", &self.bits)
            .field("paddr", &self.paddr())
            .field("flags", &self.flags())
            .finish()
    }
}
