//! PageTable infrastructure.
//! Derived from ArceOS and rCoreOS
//!
//! Copyright (c) 2023 MankorOS
//!

use crate::{
    arch, boot,
    consts::{
        self, address_space::K_SEG_PHY_MEM_BEG, HUGE_PAGE_SIZE, MAX_PHYSICAL_MEMORY, PHYMEM_START,
    },
    memory::{self, address::VirtPageNum},
    memory::{
        address::{PhysAddr, VirtAddr},
        frame,
    },
};

use alloc::{vec, vec::Vec};
use log::{debug, trace};

use super::pte::{self, PTEFlags, PageTableEntry};

// Entries count in each page table level
pub const ENTRY_COUNT: usize = 512;

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

/// Map kernel physical memory segment into virtual address space.
///
pub fn map_kernel_phys_seg() {
    let boot_pagetable = boot::boot_pagetable();

    // Map kernel physical memory
    for i in (0..MAX_PHYSICAL_MEMORY).step_by(HUGE_PAGE_SIZE) {
        let paddr: usize = i + PHYMEM_START;
        let vaddr = VirtAddr::from(i + K_SEG_PHY_MEM_BEG);
        trace!("p3 index: {}", p3_index(vaddr));
        boot_pagetable[p3_index(vaddr)] = (paddr >> 2) | 0xcf;
    }
}

/// Unmap the lower segment used for booting
pub fn unmap_boot_seg() {
    let boot_pagetable = boot::boot_pagetable();
    boot_pagetable[0] = 0;
    boot_pagetable[2] = 0;
}

/// Switch to global kernel boot pagetable
pub fn enable_boot_pagetable() {
    let boot_pagetable = boot::boot_pagetable_paddr();
    arch::switch_page_table(boot_pagetable);
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

    pub fn new_with_kernel_seg() -> Self {
        // Allocate 1 page for the root page table
        let root_paddr: PhysAddr = Self::alloc_table();
        let boot_root_paddr: PhysAddr = boot::boot_pagetable_paddr().into();

        // Copy kernel segment
        unsafe { root_paddr.as_mut_page_slice().copy_from_slice(boot_root_paddr.as_page_slice()) }

        PageTable {
            root_paddr,
            intrm_tables: vec![root_paddr],
        }
    }

    pub fn new_with_paddr(paddr: PhysAddr) -> Self {
        PageTable {
            root_paddr: paddr,
            intrm_tables: vec![paddr],
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
    pub fn unmap_page(&mut self, vaddr: VirtAddr) -> PhysAddr {
        let entry = self.get_entry_mut(vaddr);
        let paddr = entry.paddr();
        debug_assert!(entry.is_valid(), "Unmapping a invalid page table entry");
        entry.clear();
        paddr
    }
    pub fn unmap_and_dealloc_page(&mut self, vaddr: VirtAddr) -> PhysAddr {
        let entry = self.get_entry_mut(vaddr);
        let paddr = entry.paddr();
        debug_assert!(entry.is_valid(), "Unmapping a invalid page table entry");
        entry.clear();
        frame::dealloc_frame(paddr);
        paddr
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

    pub fn unmap_region(&mut self, vaddr: VirtAddr, size: usize, dealloc: bool) {
        trace!(
            "unmap_region({:#x}) [{:#x}, {:#x})",
            self.root_paddr(),
            vaddr,
            vaddr + size,
        );
        let mut vaddr = vaddr;
        let mut size = size;
        while size > 0 {
            if dealloc {
                self.unmap_and_dealloc_page(vaddr);
            } else {
                self.unmap_page(vaddr);
            }
            vaddr += consts::PAGE_SIZE;
            size -= consts::PAGE_SIZE;
        }
    }

    pub fn get_pte_copied_from_vpn(&mut self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.get_entry_mut_opt(vpn.into()).as_deref().copied()
    }

    pub fn get_paddr_from_vaddr(&self, vaddr: VirtAddr) -> PhysAddr {
        self.get_entry_mut(vaddr).paddr() + vaddr.page_offset()
    }

    pub fn copy_table_and_mark_self_cow(&mut self, do_with_frame: impl Fn(PhysAddr) -> ()) -> Self {
        let old = self;
        let mut new = Self::new();

        let op1_iter = old.table_of_mut(old.root_paddr).iter_mut();
        let np1_iter = new.table_of_mut(new.root_paddr).iter_mut();

        for (_idx, (op1, np1)) in Iterator::zip(op1_iter, np1_iter).enumerate() {
            if op1.is_leaf() {
                // Huge Page
                *np1 = *op1;
                continue;
            }
            let op2t = old.next_table_mut_opt(&op1);
            if op2t.is_none() {
                continue;
            }
            let op2_iter = op2t.unwrap().iter_mut();
            let np2_iter = new.next_table_mut_or_create(np1).iter_mut();

            for (op2, np2) in Iterator::zip(op2_iter, np2_iter) {
                let op3t = old.next_table_mut_opt(&op2);
                if op3t.is_none() {
                    continue;
                }
                let op3_iter = op3t.unwrap().iter_mut();
                let np3_iter = new.next_table_mut_or_create(np2).iter_mut();

                for (op3, np3) in Iterator::zip(op3_iter, np3_iter) {
                    if op3.is_valid() {
                        debug_assert!(op3.is_leaf());
                        if op3.is_user() {
                            // Only user page need CoW
                            do_with_frame(op3.paddr());
                            op3.set_shared(); // Allow sharing already shared page
                        }
                        *np3 = *op3;
                    }
                }
            }
        }

        new
    }
}

// Private impl
impl PageTable {
    // Allocates a page for a table
    // the allocated page will be zeroed to ensure every PTE is not valid
    fn alloc_table() -> PhysAddr {
        let paddr: PhysAddr = frame::alloc_frame().expect("failed to allocate page");
        // Fill with zeros
        unsafe {
            paddr.as_mut_page_slice().fill(0);
        }
        paddr
    }
    fn table_of<'a>(&self, paddr: PhysAddr) -> &'a [PageTableEntry] {
        // use kernel_vaddr here to work after kernel remapped
        let kernel_vaddr = memory::kernel_phys_to_virt(paddr.into());
        unsafe { core::slice::from_raw_parts(kernel_vaddr as _, ENTRY_COUNT) }
    }

    fn table_of_mut<'a>(&self, paddr: PhysAddr) -> &'a mut [PageTableEntry] {
        // use kernel_vaddr here to work after kernel remapped
        let kernel_vaddr = memory::kernel_phys_to_virt(paddr.into());
        unsafe { core::slice::from_raw_parts_mut(kernel_vaddr as _, ENTRY_COUNT) }
    }

    // Next level page table
    // Return a slice of the next level page table
    // Return None if not exist
    fn next_table_mut_opt<'a>(&self, pte: &PageTableEntry) -> Option<&'a mut [PageTableEntry]> {
        if pte.is_valid() {
            debug_assert!(pte.is_directory()); // Must be a directory
            Some(self.table_of_mut(pte.paddr()))
        } else {
            None
        }
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

    fn get_entry_mut_opt(&self, vaddr: VirtAddr) -> Option<&mut PageTableEntry> {
        let p3 = self.table_of_mut(self.root_paddr);
        let p3e = &mut p3[p3_index(vaddr)];
        let p2 = self.next_table_mut_opt(p3e)?;
        let p2e = &mut p2[p2_index(vaddr)];
        let p1 = self.next_table_mut_opt(p2e)?;
        let p1e = &mut p1[p1_index(vaddr)];
        if p1e.is_valid() {
            Some(p1e)
        } else {
            None
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
        // shared kernel segment pagetable is not in intrm_tables
        // so no extra things should be done
        for frame in &self.intrm_tables {
            frame::dealloc_frame((*frame).into());
        }
    }
}
