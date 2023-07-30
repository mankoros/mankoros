//! PageTable infrastructure.
//! Derived from ArceOS and rCoreOS
//!
//! Copyright (c) 2023 MankorOS
//!

use crate::{
    arch::{self},
    boot,
    consts::{
        self, address_space::K_SEG_PHY_MEM_BEG, device::MAX_PHYSICAL_MEMORY, device::PHYMEM_START,
        HUGE_PAGE_SIZE,
    },
    memory::{self, address::VirtPageNum},
    memory::{
        address::{PhysAddr, PhysAddr4K, VirtAddr, VirtAddr4K},
        frame,
    },
};

use alloc::{vec, vec::Vec};
use log::{trace, warn};

use super::pte::{self, PTEFlags, PageTableEntry};

// Entries count in each page table level
pub const ENTRY_COUNT: usize = 512;

fn p4_index(vaddr: VirtAddr) -> usize {
    (vaddr.bits() >> (12 + 27)) & (ENTRY_COUNT - 1)
}

pub fn p3_index(vaddr: VirtAddr) -> usize {
    (vaddr.bits() >> (12 + 18)) & (ENTRY_COUNT - 1)
}

pub fn p2_index(vaddr: VirtAddr) -> usize {
    (vaddr.bits() >> (12 + 9)) & (ENTRY_COUNT - 1)
}

fn p1_index(vaddr: VirtAddr) -> usize {
    (vaddr.bits() >> 12) & (ENTRY_COUNT - 1)
}

/// Map kernel physical memory segment into virtual address space.
///
pub fn map_kernel_phys_seg() {
    let boot_pagetable = boot::boot_pagetable();

    // Map kernel physical memory
    for i in (0..unsafe { MAX_PHYSICAL_MEMORY }).step_by(HUGE_PAGE_SIZE) {
        let paddr: usize = i + unsafe { PHYMEM_START };
        let vaddr = VirtAddr::from(i + K_SEG_PHY_MEM_BEG);

        // DA WRV
        boot_pagetable[p3_index(vaddr)] = PageTableEntry::from((paddr >> 2) | 0xc7);
    }
}

/// Unmap the lower segment used for booting
pub fn unmap_boot_seg() {
    let boot_pagetable = boot::boot_pagetable();
    for i in 0..ENTRY_COUNT / 2 {
        // Lower half is user space
        boot_pagetable[i] = PageTableEntry::EMPTY;
    }
    arch::flush_tlb_all();
}

/// Switch to global kernel boot pagetable
pub fn enable_boot_pagetable() {
    let boot_pagetable = boot::boot_pagetable_paddr();
    arch::switch_page_table(boot_pagetable);
}

pub struct PageTable {
    root_paddr: PhysAddr4K,
    intrm_tables: Vec<PhysAddr4K>,
}

impl PageTable {
    pub fn new() -> Self {
        // Allocate 1 page for the root page table
        let root_paddr = Self::alloc_table();

        PageTable {
            root_paddr,
            intrm_tables: vec![root_paddr],
        }
    }

    pub fn new_with_kernel_seg() -> Self {
        // Allocate 1 page for the root page table
        let root_paddr = Self::alloc_table();
        let boot_root_paddr = PhysAddr::from(boot::boot_pagetable_paddr()).assert_4k();

        // Copy kernel segment
        unsafe { root_paddr.as_mut_page_slice().copy_from_slice(boot_root_paddr.as_page_slice()) }

        PageTable {
            root_paddr,
            intrm_tables: vec![root_paddr],
        }
    }

    pub fn new_with_paddr(paddr: PhysAddr4K) -> Self {
        PageTable {
            root_paddr: paddr,
            intrm_tables: vec![paddr],
        }
    }

    pub const fn root_paddr(&self) -> PhysAddr4K {
        self.root_paddr
    }

    /// map_page maps a physical page to a virtual address
    /// PTE::V is guaranteed to be set, so no need to set PTE::V
    pub fn map_page(&mut self, vaddr: VirtAddr4K, paddr: PhysAddr4K, flags: PTEFlags) {
        debug_assert!(paddr.is_valid());
        let new_pte = pte::PageTableEntry::new(paddr, PTEFlags::V | flags);
        // Get entry by vaddr
        let entry = self.get_entry_mut_or_create(vaddr.into());
        debug_assert!(!entry.is_valid(), "Remapping a valid page table entry");
        *entry = new_pte;
    }
    /// remap_page allows remapping valid page
    pub fn remap_page(&mut self, vaddr: VirtAddr4K, paddr: PhysAddr4K, flags: PTEFlags) {
        debug_assert!(paddr.is_valid());
        let new_pte = pte::PageTableEntry::new(paddr, PTEFlags::V | flags);
        // Get entry by vaddr
        let entry = self.get_entry_mut_or_create(vaddr.into());
        *entry = new_pte;
    }
    pub fn unmap_page(&mut self, vaddr: VirtAddr4K) -> PhysAddr4K {
        let entry = self.get_entry_mut(vaddr.into());
        let paddr = entry.paddr();
        debug_assert!(entry.is_valid(), "Unmapping a invalid page table entry");
        entry.clear();
        paddr
    }

    // map_region map a memory region from vaddr to paddr
    // PTE::V is guaranteed to be set, so no need to set PTE::V
    pub fn map_region(
        &mut self,
        vaddr: VirtAddr4K,
        paddr: PhysAddr4K,
        size: usize,
        flags: PTEFlags,
    ) {
        trace!(
            "map_region({:#x}): [{:#x}, {:#x}) -> [{:#x}, {:#x}) ({:#?})",
            self.root_paddr(),
            vaddr,
            vaddr.into() + size,
            paddr,
            paddr.into() + size,
            flags,
        );
        let mut vaddr = vaddr;
        let mut paddr = paddr;
        let mut size = size;
        while size > 0 {
            self.map_page(vaddr, paddr, flags);
            vaddr.offset_to_next_page();
            paddr.offset_to_next_page();
            size -= consts::PAGE_SIZE;
        }
    }

    pub fn unmap_region(&mut self, vaddr: VirtAddr4K, size: usize) {
        trace!(
            "unmap_region({:#x}) [{:#x}, {:#x})",
            self.root_paddr(),
            vaddr,
            vaddr.into() + size,
        );
        let mut vaddr = vaddr;
        let mut size = size;
        while size > 0 {
            self.unmap_page(vaddr);
            vaddr.offset_to_next_page();
            size -= consts::PAGE_SIZE;
        }
    }

    pub fn get_pte_copied_from_vpn(&mut self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.get_entry_mut_opt(vpn.addr().into()).as_deref().copied()
    }
    pub fn get_pte_mut_from_vpn(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        self.get_entry_mut_opt(vpn.addr().into())
    }

    pub fn get_paddr_from_vaddr(&self, vaddr: VirtAddr) -> PhysAddr {
        self.get_entry_mut(vaddr).paddr().into() + vaddr.page_offset()
    }

    pub fn copy_table_and_mark_self_cow(&mut self, do_with_frame: impl Fn(PhysAddr4K)) -> Self {
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
            let op2t = old.next_table_mut_opt(op1);
            if op2t.is_none() {
                continue;
            }
            let op2_iter = op2t.unwrap().iter_mut();
            let np2_iter = new.next_table_mut_or_create(np1).iter_mut();

            for (op2, np2) in Iterator::zip(op2_iter, np2_iter) {
                if op2.is_leaf() {
                    // Huge Page
                    *np2 = *op2;
                    continue;
                }
                let op3t = old.next_table_mut_opt(op2);
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
                            op3.clear_writable();
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
    fn alloc_table() -> PhysAddr4K {
        let paddr = frame::alloc_frame().expect("failed to allocate page");
        // Fill with zeros
        unsafe {
            paddr.as_mut_page_slice().fill(0);
        }
        paddr
    }
    fn table_of<'a>(&self, paddr: PhysAddr4K) -> &'a [PageTableEntry] {
        // use kernel_vaddr here to work after kernel remapped
        let kernel_vaddr = memory::kernel_phys_to_virt(paddr.bits());
        unsafe { core::slice::from_raw_parts(kernel_vaddr as _, ENTRY_COUNT) }
    }

    fn table_of_mut<'a>(&self, paddr: PhysAddr4K) -> &'a mut [PageTableEntry] {
        // use kernel_vaddr here to work after kernel remapped
        let kernel_vaddr = memory::kernel_phys_to_virt(paddr.bits());
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
            self.intrm_tables.push(paddr);
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

        &mut p1[p1_index(vaddr)]
    }

    fn get_entry_mut_or_create(&mut self, vaddr: VirtAddr) -> &mut PageTableEntry {
        let p3 = self.table_of_mut(self.root_paddr);
        let p3e = &mut p3[p3_index(vaddr)];
        let p2 = self.next_table_mut_or_create(p3e);
        let p2e = &mut p2[p2_index(vaddr)];
        let p1 = self.next_table_mut_or_create(p2e);

        &mut p1[p1_index(vaddr)]
    }
}

impl Drop for PageTable {
    fn drop(&mut self) {
        // First
        // shared kernel segment pagetable is not in intrm_tables
        // so no extra things should be done
        for (i, frame) in self.intrm_tables.iter().enumerate() {
            log::trace!("Drop pagetable page {:#x} ({})", frame, i);
            cfg_if::cfg_if! {
                // Debug sanity check
                if #[cfg(debug_assertions)] {
                    let ref_cnt = frame.page_num().get_ref_cnt();
                    if ref_cnt != 1 {
                        warn!("Pagetable page {:#x} still has {} references", frame, ref_cnt);
                        panic!("Pagetable page should not have references");
                    }

                    let page = self.table_of(*frame);
                    for pte in page.iter() {
                        if pte.is_valid() && pte.is_leaf() && pte.is_user() {
                            warn!("Pagetable page {:#x} still valid user page", frame);
                            panic!("Pagetable page should not be valid");
                        }
                    }
                    // Clear dealloc page when in debug
                    unsafe { frame.as_mut_page_slice().fill(0x5) };
                }
            }
            debug_assert!(frame.is_valid());
            frame.page_num().decrease();
            frame::dealloc_frame(*frame);
        }
    }
}
