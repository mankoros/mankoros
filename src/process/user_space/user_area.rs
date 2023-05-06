use alloc::sync::Arc;
use bitflags::bitflags;


use super::{range_map::RangeMap};
use crate::memory::address::VirtAddr;

use crate::{
    consts,
    fs::vfs::filesystem::VfsNode,
    memory::{
        address::{VirtPageNum},
        frame::{alloc_frame},
        kernel_phys_to_virt,
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
};

use riscv::register::scause;
use core::ops::Range;
use core::fmt::Debug;

use crate::consts::address_space::{U_SEG_HEAP_BEG, U_SEG_FILE_END, U_SEG_FILE_BEG};

use crate::process::shared_frame_mgr::with_shared_frame_mgr;
use crate::arch::get_curr_page_table_addr;

pub type VirtAddrRange = Range<VirtAddr>;

bitflags! {
    pub struct UserAreaPerm: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

impl Into<PTEFlags> for UserAreaPerm {
    fn into(self) -> PTEFlags {
        let mut pte_flag = PTEFlags::V | PTEFlags::U;
        if self.contains(Self::READ) {
            pte_flag |= PTEFlags::R;
        }
        if self.contains(Self::WRITE) {
            pte_flag |= PTEFlags::W;
        }
        if self.contains(Self::EXECUTE) {
            pte_flag |= PTEFlags::X;
        }
        pte_flag
    }
}

impl From<xmas_elf::program::Flags> for UserAreaPerm {
    fn from(flags: xmas_elf::program::Flags) -> Self {
        let mut area_flags = UserAreaPerm::empty();

        if flags.is_read() {
            area_flags |= UserAreaPerm::READ;
        }
        if flags.is_write() {
            area_flags |= UserAreaPerm::WRITE;
        }
        if flags.is_execute() {
            area_flags |= UserAreaPerm::EXECUTE;
        }
    
        area_flags
    }
}

bitflags! {
    pub struct PageFaultAccessType: u8 {
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

impl PageFaultAccessType {
    // no write & no execute == read only
    pub const RO: Self = Self::empty();
    // can't use | (bits or) here
    // see https://github.com/bitflags/bitflags/issues/180
    pub const RW: Self = Self::WRITE;
    pub const RX: Self = Self::EXECUTE;

    pub fn from_exception(e: scause::Exception) -> Self {
        match e {
            scause::Exception::InstructionPageFault => Self::RX,
            scause::Exception::LoadPageFault => Self::RO,
            scause::Exception::StorePageFault => Self::RW,
            _ => panic!("unexcepted exception type for PageFaultAccessType"),
        }
    }

    /// 检查是否有足够的权限以该种访问方式访问该页
    pub fn can_access(self, flag: UserAreaPerm) -> bool {
        // 对不可写的页写入是非法的
        if self.contains(Self::WRITE) && !flag.contains(UserAreaPerm::WRITE) {
            return false;
        }

        // 对不可执行的页执行是非法的
        if self.contains(Self::EXECUTE) && !flag.contains(UserAreaPerm::EXECUTE) {
            return false;
        }

        return true;
    }
}

enum UserAreaType {
    /// 匿名映射区域
    MmapAnonymous,
    /// 私有映射区域
    MmapPrivate {
        file: Arc<dyn VfsNode>,
        offset: usize,
    },
    // TODO: 共享映射区域
    // MmapShared {
    //     file: Arc<dyn VfsNode>,
    //     offset: usize,
    // },
}

impl Debug for UserAreaType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UserAreaType::MmapAnonymous => write!(f, "MmapAnonymous"),
            UserAreaType::MmapPrivate { file, offset } => write!(
                f,
                "MmapPrivate {{ file ptr: {:?}, offset: {} }}",
                file.as_ref() as *const dyn VfsNode, offset
            ),
        }
    }
}

#[derive(Debug)]
pub enum PageFaultErr {
    NoSegment,
    PermUnmatch,
    KernelOOM,
}

#[derive(Debug)]
pub struct UserArea {
    kind: UserAreaType,
    perm: UserAreaPerm,
}

impl UserArea {
    pub fn new_anonymous(_range: VirtAddrRange, perm: UserAreaPerm) -> Self {
        Self {
            kind: UserAreaType::MmapAnonymous,
            perm,
        }
    }

    pub fn new_private(
        perm: UserAreaPerm,
        file: Arc<dyn VfsNode>,
        offset: usize,
    ) -> Self {
        Self {
            kind: UserAreaType::MmapPrivate { file, offset },
            perm,
        }
    }

    pub fn perm(&self) -> UserAreaPerm {
        self.perm
    }

    pub fn page_fault(
        &mut self,
        page_table: &mut PageTable,
        access_vpn: VirtPageNum,
        access_type: PageFaultAccessType,
    ) -> Result<(), PageFaultErr> {
        if !access_type.can_access(self.perm()) {
            return Err(PageFaultErr::PermUnmatch);
        }

        let pte = page_table.get_pte_copied_from_vpn(access_vpn);
        let frame = alloc_frame()
            .ok_or(PageFaultErr::KernelOOM)?;

        if let Some(pte) = pte && pte.is_valid() {
            // must be CoW
            let pte_flags = pte.flags();
            debug_assert!(pte_flags.contains(PTEFlags::SHARED));
            debug_assert!(!pte_flags.contains(PTEFlags::W));
            debug_assert!(self.perm().contains(UserAreaPerm::WRITE));

            // decrease the old frame's ref count
            let old_frame = pte.paddr();
            with_shared_frame_mgr(|mgr| {
                mgr.remove_ref(old_frame.into()); 
            });

            // copy the data
            // assert we are in process's page table now
            debug_assert!(page_table.root_paddr() == get_curr_page_table_addr().into());
            unsafe {
                frame.as_mut_page_slice().copy_from_slice(old_frame.as_page_slice());
            }

            // re-map new frame
            page_table.map_page(access_vpn.into(), frame, self.perm().into());

        } else {
            // a lazy alloc or lazy load (demand paging)
            match &self.kind {
                // If lazy alloc, map the allocate frame
                UserAreaType::MmapAnonymous => {}
                // If lazy load, read from fs
                UserAreaType::MmapPrivate { file, offset } => {
                    let slice = unsafe { frame.as_mut_page_slice() };
                    let read_length = file.read_at(*offset as u64, slice).expect("read file failed");
                    assert_eq!(read_length, consts::PAGE_SIZE);
                }
            }
            page_table.map_page(access_vpn.into(), frame, self.perm().into());
        }
        
        Ok(())
    }
}

/// 管理整个用户虚拟地址空间的虚拟地址分配
/// 包括堆和栈
pub struct UserAreaManager {
    map: RangeMap<VirtAddr, UserArea>,    
}

impl UserAreaManager {
    const HEAP_BEG: VirtAddr = U_SEG_HEAP_BEG.into();
    const MMAP_RANGE: Range<VirtAddr> = U_SEG_FILE_BEG.into()..U_SEG_FILE_END.into();

    pub fn new() -> Self {
        Self {
            map: RangeMap::new(),
        }
    }

    pub fn get_area(&self, vaddr: VirtAddr) -> Option<&UserArea> {
        self.get(vaddr).map(|(_, a)| a)
    }

    pub fn get(&self, vaddr: VirtAddr) -> Option<(VirtAddrRange, &UserArea)> {
        self.map.get(vaddr)
    }

    pub fn insert_stack_at(&mut self, begin_vaddr: VirtAddr, size: usize) {
        let range = VirtAddrRange {
            start: begin_vaddr,
            end: begin_vaddr + size,
        };
        let area = UserArea::new_anonymous(range.clone(), UserAreaPerm::READ | UserAreaPerm::WRITE);
        self.map.try_insert(range, area).unwrap();
    }

    pub fn insert_heap(&mut self, init_size: usize) {
        let range = VirtAddrRange {
            start: Self::HEAP_BEG,
            end: Self::HEAP_BEG + init_size,
        };
        let area = UserArea::new_anonymous(range.clone(), UserAreaPerm::READ | UserAreaPerm::WRITE);
        self.map.try_insert(range, area).unwrap();
    }

    pub fn reset_heap_break(&mut self, new_brk: VirtAddr) -> Result<(), ()> {
        // TODO-PERF: 缓存 heap 的位置, 减少一次查询
        let (Range { start, end }, _) = self.map.get(Self::HEAP_BEG)
            .expect("brk without heap");

        if end < new_brk {
            // when larger, create a new area [heap_end, new_brk), then merge it with current heap
            self.map.extend_back(start, new_brk)
        } else if new_brk < end {
            // when smaller, split the area into [heap_start, new_brk), [new_brk, heap_end), then remove the second one
            self.map.reduce_back(start, new_brk)
        } else {
            // when equal, do nothing
            Ok(())
        }
    }

    fn find_free_mmap_area(&self, size: usize) -> Result<(VirtAddr, usize), ()> {
        self.map
            .find_free_range(Self::MMAP_RANGE, size, |va, n| (va + n).round_up())
            .map(|r| (r.start, r.end - r.start))
            .ok_or(()) 
    }

    pub fn insert_mmap_anonymous(&mut self, size: usize, perm: UserAreaPerm) -> Result<(VirtAddrRange, &UserArea), ()> {
        let (begin, size) = self.find_free_mmap_area(size)?;
        self.insert_mmap_anonymous_at(begin, size, perm)
    }

    pub fn insert_mmap_private(
        &mut self, 
        size: usize, 
        perm: UserAreaPerm,
        file: Arc<dyn VfsNode>,
        offset: usize,
    ) -> Result<(VirtAddrRange, &UserArea), ()> {
        let (begin, size) = self.find_free_mmap_area(size)?;
        self.insert_mmap_private_at(begin, size, perm, file, offset)
    }

    pub fn insert_mmap_anonymous_at(
        &mut self, 
        begin_vaddr: VirtAddr, 
        size: usize, 
        perm: UserAreaPerm
    ) -> Result<(VirtAddrRange, &UserArea), ()> {
        let range = VirtAddrRange {
            start: begin_vaddr,
            end: begin_vaddr + size,
        };
        let area = UserArea::new_anonymous(range.clone(), perm);
        self.map.try_insert(range.clone(), area).map(|v| (range, &*v)).map_err(|_| ())
    }

    pub fn insert_mmap_private_at(
        &mut self, 
        begin_vaddr: VirtAddr, 
        size: usize, 
        perm: UserAreaPerm,
        file: Arc<dyn VfsNode>,
        offset: usize,
    ) -> Result<(VirtAddrRange, &UserArea), ()> {
        let range = VirtAddrRange {
            start: begin_vaddr,
            end: begin_vaddr + size,
        };
        let area = UserArea::new_private(perm, file, offset);
        self.map.try_insert(range.clone(), area).map(|v| (range, &*v)).map_err(|_| ())
    }

    pub fn page_fault(&mut self, page_table: &mut PageTable, access_vpn: VirtPageNum, access_type: PageFaultAccessType) -> Result<(), PageFaultErr> {
        let (_, area) = self.map.get_mut(access_vpn.into())
            .ok_or(PageFaultErr::NoSegment)?;
        area.page_fault(page_table, access_vpn, access_type)
    }

    pub fn force_map_range(&mut self, page_table: &mut PageTable, range: VirtAddrRange) {
        let vpn_begin = range.start.into();
        let vpn_end = range.end.round_up().into();

        let mut vpn = vpn_begin;
        while vpn < vpn_end {
            self.page_fault(page_table, vpn, PageFaultAccessType::RO).unwrap();
            vpn += 1;
        }
    }
}