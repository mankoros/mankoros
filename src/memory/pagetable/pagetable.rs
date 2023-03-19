//! PageTable infrastructure.
//! Derived from ArceOS and rCoreOS
//!
//! Copyright (c) 2023 MankorOS
//!

use crate::{
    consts,
    memory::{
        address::{PhysAddr, VirtAddr},
        frame,
    },
};

use log::trace;

use super::pte::{self, PTEFlags, PageTableEntry};

// Entries count in each page table level
const ENTRY_COUNT: usize = 512;

fn p4_index(vaddr: VirtAddr) -> usize {
    (usize::from(vaddr) >> (12 + 27)) & (ENTRY_COUNT - 1)
}

fn p3_index(vaddr: VirtAddr) -> usize {
    (usize::from(vaddr) >> (12 + 18)) & (ENTRY_COUNT - 1)
}

fn p2_index(vaddr: VirtAddr) -> usize {
    (usize::from(vaddr) >> (12 + 9)) & (ENTRY_COUNT - 1)
}

fn p1_index(vaddr: VirtAddr) -> usize {
    (usize::from(vaddr) >> 12) & (ENTRY_COUNT - 1)
}

pub struct PageTable {
    root_paddr: PhysAddr,
}

impl PageTable {
    pub fn new(root_paddr: PhysAddr) -> Self {
        // Allocate 1 page for the root page table
        let mut root_paddr: PhysAddr = Self::alloc_table();

        // Fill with zeros
        unsafe {
            core::ptr::write_bytes(root_paddr.as_mut_ptr(), 0, consts::PAGE_SIZE as usize);
        }
        PageTable { root_paddr }
    }

    pub const fn root_paddr(&self) -> PhysAddr {
        self.root_paddr
    }

    pub fn map_page(&mut self, vaddr: VirtAddr, paddr: PhysAddr, flags: PTEFlags) {
        let new_pte = pte::PageTableEntry::new(paddr, flags);
        // Get entry by vaddr
        let entry = self.get_entry_mut_or_create(vaddr);
        debug_assert!(!entry.is_valid(), "Remapping a valid page table entry");
        *entry = new_pte;
    }
    pub fn unmap_page(&mut self, vaddr: VirtAddr) {
        let entry = self.get_entry_mut(vaddr);
        debug_assert!(entry.is_valid(), "Unmapping a invalid page table entry");
        entry.clear();
    }

    pub fn map_region(&mut self, vaddr: VirtAddr, paddr: PhysAddr, size: usize, flags: PTEFlags) {
        trace!(
            "map_region({:#x}): [{:#x}, {:#x}) -> [{:#x}, {:#x}) ({:#?})",
            self.root_paddr(),
            vaddr,
            vaddr + size,
            paddr,
            paddr + size,
            flags,
        );
        let mut vaddr = vaddr;
        let mut paddr = paddr;
        let mut size = size;
        while size > 0 {
            self.map_page(vaddr, paddr, flags);
            vaddr += consts::PAGE_SIZE;
            paddr += consts::PAGE_SIZE;
            size -= consts::PAGE_SIZE;
        }
    }

    pub fn unmap_region(&mut self, vaddr: VirtAddr, size: usize) {
        trace!(
            "unmap_region({:#x}) [{:#x}, {:#x})",
            self.root_paddr(),
            vaddr,
            vaddr + size,
        );
        let mut vaddr = vaddr;
        let mut size = size;
        while size > 0 {
            self.unmap_page(vaddr);
            vaddr += consts::PAGE_SIZE;
            size -= consts::PAGE_SIZE;
        }
    }
}

// Private impl
impl PageTable {
    // Allocates a page for a table
    fn alloc_table() -> PhysAddr {
        frame::alloc_frame().expect("failed to allocate page").into()
    }
    fn table_of<'a>(&self, paddr: PhysAddr) -> &'a [PageTableEntry] {
        let ptr = paddr.as_ptr() as _;
        unsafe { core::slice::from_raw_parts(ptr, ENTRY_COUNT) }
    }

    fn table_of_mut<'a>(&self, paddr: PhysAddr) -> &'a mut [PageTableEntry] {
        let ptr = paddr.as_mut_ptr() as _;
        unsafe { core::slice::from_raw_parts_mut(ptr, ENTRY_COUNT) }
    }

    // Next level page table
    // Return a slice of the next level page table
    fn next_table_mut<'a>(&self, pte: &PageTableEntry) -> &'a mut [PageTableEntry] {
        debug_assert!(pte.is_valid());
        self.table_of_mut(pte.paddr())
    }

    // Next level page table
    // Return a slice of the next level page table
    // Create if not exist
    fn next_table_mut_or_create<'a>(
        &mut self,
        pte: &mut PageTableEntry,
    ) -> &'a mut [PageTableEntry] {
        if !pte.is_valid() {
            let paddr = Self::alloc_table();
            !todo!();
        } else {
            self.next_table_mut(pte)
        }
    }

    fn get_entry_mut(&self, vaddr: VirtAddr) -> &mut PageTableEntry {
        let p3 = self.table_of_mut(self.root_paddr);
        let p3e = &mut p3[p3_index(vaddr)];
        let p2 = self.next_table_mut(p3e);
        let p2e = &mut p2[p2_index(vaddr)];
        let p1 = self.next_table_mut(p2e);
        let p1e = &mut p1[p1_index(vaddr)];
        p1e
    }

    fn get_entry_mut_or_create(&mut self, vaddr: VirtAddr) -> &mut PageTableEntry {
        let p3 = self.table_of_mut(self.root_paddr);
        let p3e = &mut p3[p3_index(vaddr)];
        let p2 = self.next_table_mut_or_create(p3e);
        let p2e = &mut p2[p2_index(vaddr)];
        let p1 = self.next_table_mut_or_create(p2e);
        let p1e = &mut p1[p1_index(vaddr)];
        p1e
    }
}
