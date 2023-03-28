//! PageTable infrastructure.
//! Derived from ArceOS and rCoreOS
//!
//! Copyright (c) 2023 MankorOS
//!

use crate::{
    consts, memory,
    memory::{
        address::{PhysAddr, VirtAddr},
        frame,
    },
};

use alloc::{vec, vec::Vec};
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
    intrm_tables: Vec<PhysAddr>,
}

impl PageTable {
    pub fn new() -> Self {
        // Allocate 1 page for the root page table
        let root_paddr: PhysAddr = Self::alloc_table();

        PageTable {
            root_paddr,
            intrm_tables: vec![root_paddr],
        }
    }

    pub const fn root_paddr(&self) -> PhysAddr {
        self.root_paddr
    }

    // map_page maps a physical page to a virtual address
    // PTE::V is guaranteed to be set, so no need to set PTE::V
    pub fn map_page(&mut self, vaddr: VirtAddr, paddr: PhysAddr, flags: PTEFlags) {
        let new_pte = pte::PageTableEntry::new(paddr, PTEFlags::V | flags);
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

    // map_region map a memory region from vaddr to paddr
    // PTE::V is guaranteed to be set, so no need to set PTE::V
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
    // the allocated page will be zeroed to ensure every PTE is not valid
    fn alloc_table() -> PhysAddr {
        let paddr = frame::alloc_frame().expect("failed to allocate page");
        // use kernel_vaddr here to work after kernel remapped
        let kernel_vaddr = memory::phys_to_virt(paddr);
        // Fill with zeros
        unsafe {
            core::ptr::write_bytes(kernel_vaddr as *mut u8, 0, consts::PAGE_SIZE as usize);
        }
        paddr.into()
    }
    fn table_of<'a>(&self, paddr: PhysAddr) -> &'a [PageTableEntry] {
        // use kernel_vaddr here to work after kernel remapped
        let kernel_vaddr = memory::phys_to_virt(paddr.into());
        unsafe { core::slice::from_raw_parts(kernel_vaddr as _, ENTRY_COUNT) }
    }

    fn table_of_mut<'a>(&self, paddr: PhysAddr) -> &'a mut [PageTableEntry] {
        // use kernel_vaddr here to work after kernel remapped
        let kernel_vaddr = memory::phys_to_virt(paddr.into());
        unsafe { core::slice::from_raw_parts_mut(kernel_vaddr as _, ENTRY_COUNT) }
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
            self.intrm_tables.push(paddr.into());
            *pte = PageTableEntry::new(paddr, PTEFlags::V);
            self.table_of_mut(paddr)
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

impl Drop for PageTable {
    fn drop(&mut self) {
        for frame in &self.intrm_tables {
            frame::dealloc_frame((*frame).into());
        }
    }
}
