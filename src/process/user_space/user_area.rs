use alloc::sync::Arc;
use bitflags::bitflags;


use super::{range_map::RangeMap};
use crate::memory::address::VirtAddr;

use crate::{
    fs::vfs::filesystem::VfsNode,
    memory::{
        address::{VirtPageNum},
        frame::{alloc_frame},
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
};

use riscv::register::{scause};
use core::fmt::Debug;

use crate::consts::address_space::{U_SEG_HEAP_BEG, U_SEG_FILE_END, U_SEG_FILE_BEG, U_SEG_STACK_BEG, U_SEG_STACK_END};

use crate::process::shared_frame_mgr::with_shared_frame_mgr;
use crate::arch::get_curr_page_table_addr;
use log::{debug, trace};
use core::ops::Range;
use crate::memory::frame::dealloc_frame;
use crate::consts::PAGE_SIZE;

pub type VirtAddrRange = Range<VirtAddr>;

#[inline(always)]
fn iter_vpn(range: VirtAddrRange, mut f: impl FnMut(VirtPageNum) -> ()) {
    let start_vpn = range.start.assert_4k().page_num();
    let end_vpn = range.end.round_up().page_num(); // End vaddr may not be 4k aligned
    let mut vpn = start_vpn;
    while vpn < end_vpn {
        f(vpn);
        vpn += 1;
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone)]
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

#[derive(Debug, Clone, Copy)]
pub enum PageFaultErr {
    NoSegment,
    PermUnmatch,
    KernelOOM,
}


unsafe impl Send for PageFaultErr {}
unsafe impl Sync for PageFaultErr {}

#[derive(Clone, Debug)]
pub struct UserArea {
    kind: UserAreaType,
    perm: UserAreaPerm,
}

impl UserArea {
    pub fn new_anonymous(perm: UserAreaPerm) -> Self {
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
        &self,
        page_table: &mut PageTable,
        range_begin: VirtAddr, // Allow unaligned mmap ?
        access_vpn: VirtPageNum,
        access_type: PageFaultAccessType,
    ) -> Result<(), PageFaultErr> {
        debug!("page fault: {:?}, {:?}, {:?} at page table {:?}", 
            self, access_vpn, access_type, page_table.root_paddr());

        if !access_type.can_access(self.perm()) {
            return Err(PageFaultErr::PermUnmatch);
        }

        // anyway we need a new frame
        let frame = alloc_frame()
            .ok_or(PageFaultErr::KernelOOM)?;
    
        // perpare the data for the new frame
        let pte = page_table.get_pte_copied_from_vpn(access_vpn);
        if let Some(pte) = pte && pte.is_valid() {
            // must be CoW
            let pte_flags = pte.flags();
            debug_assert!(pte_flags.contains(PTEFlags::SHARED));
            debug_assert!(!pte_flags.contains(PTEFlags::W));
            debug_assert!(self.perm().contains(UserAreaPerm::WRITE));

            // decrease the old frame's ref count
            let old_frame = pte.paddr();
            with_shared_frame_mgr(|mgr| {
                mgr.remove_ref(old_frame.page_num()); 
            });

            // copy the data
            // assert we are in process's page table now
            debug_assert!(page_table.root_paddr().bits() == get_curr_page_table_addr());
            unsafe {
                frame.as_mut_page_slice()
                    .copy_from_slice(old_frame.as_page_slice());
            }
        } else {
            // a lazy alloc or lazy load (demand paging)
            match &self.kind {
                // If lazy alloc, do nothing (or maybe memset it to zero?)
                UserAreaType::MmapAnonymous => {}
                // If lazy load, read from fs
                UserAreaType::MmapPrivate { file, offset } => {
                    let access_vaddr = access_vpn.addr();
                    let real_offset = offset + (access_vaddr.into() - range_begin);
                    let slice = unsafe { frame.as_mut_page_slice() };
                    let _read_length = file.sync_read_at(real_offset as u64, slice).expect("read file failed");
                    // Read length may be less than PAGE_SIZE, due to file mmap
                }
            }
        }

        // remap the frame
        page_table.map_page(access_vpn.addr(), frame, self.perm().into());
        Ok(())
    }

    fn split_and_make_left(&mut self, split_at: VirtAddr, range: VirtAddrRange) -> Self {
        use UserAreaType::*;
        // return left-hand-side area
        match &mut self.kind {
            MmapAnonymous => {
                UserArea::new_anonymous(self.perm)
            },
            MmapPrivate { file, offset } => {
                let old_offset = *offset;
                // change self to become the new right-hand-side area
                *offset += split_at - range.start;
                UserArea::new_private(self.perm, file.clone(), old_offset)
            },
        }
    }

    fn split_and_make_right(&mut self, split_at: VirtAddr, range: VirtAddrRange) -> Self {
        use UserAreaType::*;
        // change self to become the new left-hand-side area: nothing need to do
        // return right-hand-side area
        match &self.kind {
            MmapAnonymous => {
                UserArea::new_anonymous(self.perm)
            },
            MmapPrivate { file, offset } => {
                UserArea::new_private(self.perm, file.clone(), *offset + (split_at - range.start))
            },
        }
    }
}

/// 管理整个用户虚拟地址空间的虚拟地址分配
/// 包括堆和栈
#[derive(Clone)]
pub struct UserAreaManager {
    map: RangeMap<VirtAddr, UserArea>,    
}

impl UserAreaManager {
    const HEAP_BEG: VirtAddr = VirtAddr::from(U_SEG_HEAP_BEG);
    const STACK_RANGE: VirtAddrRange = VirtAddr::from(U_SEG_STACK_BEG)..VirtAddr::from(U_SEG_STACK_END);
    const MMAP_RANGE: VirtAddrRange = VirtAddr::from(U_SEG_FILE_BEG)..VirtAddr::from(U_SEG_FILE_END);

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

    pub fn iter(&self) -> impl Iterator<Item = (VirtAddrRange, &UserArea)> {
        self.map.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (VirtAddrRange, &mut UserArea)> {
        self.map.iter_mut()
    }

    /// 返回栈的开始地址 sp_init, [sp_init - size, sp_init] 都是栈的范围。
    /// sp_init 16 字节对齐
    pub fn alloc_stack(&mut self, size: usize) -> VirtAddr {
        let range = self.map
            .find_free_range(Self::STACK_RANGE, size, |va, n| (va + n).round_up().into())
            .expect("too many stack!");

        // 栈要 16 字节对齐
        let sp_init = VirtAddr::from((range.end.bits() - PAGE_SIZE) & !0xf);
        trace!("alloc stack: {:x?}, sp_init: {:x?}", range, sp_init);

        let area = UserArea::new_anonymous(
            UserAreaPerm::READ | UserAreaPerm::WRITE
        );
        self.map.try_insert(range, area).unwrap();
        
        sp_init
    }

    pub fn insert_heap(&mut self, init_size: usize) {
        let range = VirtAddrRange {
            start: Self::HEAP_BEG,
            end: Self::HEAP_BEG + init_size,
        };
        let area = UserArea::new_anonymous(UserAreaPerm::READ | UserAreaPerm::WRITE);
        self.map.try_insert(range, area).unwrap();
    }

    pub fn get_heap_break(&self) -> VirtAddr {
        let (Range { start: _, end }, _) = self.map.get(Self::HEAP_BEG)
            .expect("get heap break without heap");
        end
    }

    pub fn reset_heap_break(&mut self, new_brk: VirtAddr) -> Result<(), ()> {
        // TODO-PERF: 缓存 heap 的位置，减少一次查询
        let (Range { start, end }, _) = self.map.get(Self::HEAP_BEG)
            .expect("brk without heap");

        if end < new_brk {
            // when larger, create a new area [heap_end, new_brk), then merge it with current heap
            self.map.extend_back(start, new_brk)
        } else if new_brk < end {
            // when smaller, split the area into [heap_start, new_brk), [new_brk, heap_end), then remove the second one
            self.map.reduce_back(start, new_brk)
                .map(|_| ())
            // TODO: release page
        } else {
            // when equal, do nothing
            Ok(())
        }
    }

    fn find_free_mmap_area(&self, size: usize) -> Result<(VirtAddr, usize), ()> {
        self.map
            .find_free_range(Self::MMAP_RANGE, size, |va, n| (va + n).round_up().into())
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
        let area = UserArea::new_anonymous(perm);
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
        let (range, area) = self.map.get_mut(access_vpn.addr().into())
            .ok_or(PageFaultErr::NoSegment)?;
        area.page_fault(page_table, range.start, access_vpn, access_type)
    }

    pub fn force_map_range(&mut self, page_table: &mut PageTable, range: VirtAddrRange) {
        debug!("force map range: {:?}", range);

        let vpn_begin = range.start.assert_4k().page_num();
        let vpn_end = range.end.assert_4k().page_num();

        let mut vpn = vpn_begin;
        while vpn < vpn_end {
            self.page_fault(page_table, vpn, PageFaultAccessType::RO).unwrap();
            vpn += 1;
        }
    }

    pub fn unmap_range(&mut self, page_table: &mut PageTable, range: VirtAddrRange) {
        debug!("unmap range: {:?}", range);
        self.map.remove(range, 
            UserArea::split_and_make_left,
            UserArea::split_and_make_right, 
            |_area, range| Self::release_range(page_table, range) 
        );
    }

    /// 释放一个虚拟地址范围内的所有页
    /// 
    /// **注意**: 只释放物理页，不会管分段
    pub fn release_range(page_table: &mut PageTable, range: VirtAddrRange) {
        debug!("release range: {:?}", range);
        // 释放被删除的段
        with_shared_frame_mgr(|mgr| {
            iter_vpn(range, |vpn| {
                // TODO-PERF: 尝试在段中维护已映射的共享物理页，以减少查询次数
                let pte = page_table.get_pte_copied_from_vpn(vpn);
                if pte.is_none() {
                    return;
                }

                let ppn = pte.unwrap().ppn();
                // 如果是共享的，则只减少引用计数，否则释放
                if mgr.is_shared(ppn) {
                    mgr.remove_ref(ppn);
                } else {
                    dealloc_frame(ppn.addr());
                }
            })
        })
    }
}