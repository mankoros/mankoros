// Copyright (c) Easton Man
//
// PTE data infrastructure
// Adpated from FTL OS
// https://gitlab.eduxiji.net/DarkAngelEX/oskernel2022-ftlos/-/blob/master/code/kernel/src/memory/page_table/mod.rs

use bitflags::bitflags;

use crate::consts;
use crate::memory::address::{PhysAddr, PhysPageNum};
use crate::memory::frame;

bitflags! {
    // riscv-privileged 4.3.1 P87
    pub struct PTEFlags: u16 {
        const V = 1 << 0; // valid
        const R = 1 << 1; // readable
        const W = 1 << 2; // writalbe
        const X = 1 << 3; // executable
        const U = 1 << 4; // user mode
        const G = 1 << 5; // global mapping
        const A = 1 << 6; // access, set to 1 after r/w/x
        const D = 1 << 7; // dirty, set to 1 after write
        const SHARED = 1 << 8; // copy-on-write
    }
}

impl PTEFlags {
    pub fn writable(self) -> bool {
        self.contains(Self::W)
    }
    pub fn executable(self) -> bool {
        self.contains(Self::X)
    }
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

impl PageTableEntry {
    pub fn new(paddr: PhysAddr, perm: PTEFlags) -> Self {
        PageTableEntry {
            bits: (usize::from(paddr.round_down()) >> 2) & consts::PPN_MASK_SV39
                | perm.bits as usize,
        }
    }
    pub const EMPTY: Self = Self { bits: 0 };
    /// Clear
    pub fn reset(&mut self) {
        *self = Self::EMPTY;
    }
    pub fn ppn(&self) -> PhysPageNum {
        PhysPageNum::from((self.bits & consts::PPN_MASK_SV39) >> 10)
    }
    pub fn paddr(&self) -> PhysAddr {
        self.ppn().into()
    }
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u16).unwrap()
    }

    pub fn is_valid(&self) -> bool {
        self.flags().valid()
    }
    pub fn is_directory(&self) -> bool {
        let mask = PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U;
        self.is_valid() && (self.flags() & mask) == PTEFlags::empty()
    }
    pub fn is_leaf(&self) -> bool {
        let mask = PTEFlags::R | PTEFlags::W | PTEFlags::X | PTEFlags::U;
        self.is_valid() && (self.flags() & mask) != PTEFlags::empty()
    }
    pub fn readable(&self) -> bool {
        self.flags().contains(PTEFlags::R)
    }
    pub fn writable(&self) -> bool {
        self.flags().contains(PTEFlags::W)
    }
    pub fn executable(&self) -> bool {
        self.flags().contains(PTEFlags::X)
    }
    pub fn is_user(&self) -> bool {
        self.flags().contains(PTEFlags::U)
    }
    pub fn shared(&self) -> bool {
        self.flags().contains(PTEFlags::SHARED)
    }
    pub fn rsw_8(&self) -> bool {
        (self.bits & 1usize << 8) != 0
    }
    pub fn rsw_9(&self) -> bool {
        (self.bits & 1usize << 9) != 0
    }
    pub fn reserved_bits(&self) -> usize {
        self.bits & (((1usize << 10) - 1) << 54)
    }
    pub fn set_rwx(&mut self, flag: PTEFlags) {
        let mask = (PTEFlags::R | PTEFlags::W | PTEFlags::X).bits() as usize;
        let flag = flag.bits() as usize & mask;
        self.bits = (self.bits & !mask) | flag;
    }
    pub fn set_writable(&mut self) {
        self.bits |= PTEFlags::W.bits() as usize;
    }
    pub fn clear_writable(&mut self) {
        self.bits &= !(PTEFlags::W.bits() as usize);
    }
    pub fn set_shared(&mut self) {
        self.bits |= PTEFlags::SHARED.bits() as usize;
    }
    pub fn clear_shared(&mut self) {
        self.bits &= !PTEFlags::SHARED.bits() as usize;
    }
    pub fn become_shared(&mut self, shared_writable: bool) {
        debug_assert!(!self.shared());
        self.set_shared();
        if !shared_writable {
            self.clear_writable();
        }
    }
    pub fn become_unique(&mut self, unique_writable: bool) {
        debug_assert!(self.shared());
        self.clear_shared();
        if unique_writable {
            self.set_writable();
        }
    }
    /// 用来分配非叶节点，不能包含 URW 标志位
    pub fn alloc_non_leaf(&mut self, perm: PTEFlags) {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        debug_assert!(!perm.intersects(PTEFlags::U | PTEFlags::R | PTEFlags::W));
        let pa = frame::alloc_frame().unwrap(); // TODO: add error checking
        *self = Self::new(PhysAddr::from(pa), perm | PTEFlags::V);
    }

    /// 为这个页节点分配实际物理页，不会填充任何数据！需要手动初始化内存
    pub fn alloc(&mut self, perm: PTEFlags) {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        let pa = frame::alloc_frame().unwrap(); // TODO: add error checking
        *self = Self::new(
            PhysAddr::from(pa),
            perm | PTEFlags::D | PTEFlags::A | PTEFlags::V,
        );
    }
    pub fn map_frame(&mut self, perm: PTEFlags, pa: PhysAddr) {
        debug_assert!(!self.is_valid(), "try alloc to a valid pte");
        *self = Self::new(pa, perm | PTEFlags::D | PTEFlags::A | PTEFlags::V);
    }
    /// this function will clear V flag.
    pub unsafe fn dealloc(&mut self) {
        debug_assert!(self.is_valid() && self.is_leaf());
        frame::dealloc_frame(self.paddr().into());
        *self = Self::EMPTY;
    }
    /// this function will clear V flag.
    pub unsafe fn dealloc_non_leaf(&mut self) {
        debug_assert!(self.is_valid() && self.is_directory());
        frame::dealloc_frame(self.paddr().into());
        *self = Self::EMPTY;
    }
}
