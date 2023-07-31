use bitflags::bitflags;

use super::range_map::RangeMap;
use crate::memory::address::VirtAddr;

use crate::memory::{
    address::VirtPageNum,
    frame::alloc_frame,
    pagetable::{pagetable::PageTable, pte::PTEFlags},
};

use core::fmt::Debug;
use riscv::register::scause;

use crate::consts::address_space::{
    U_SEG_FILE_BEG, U_SEG_FILE_END, U_SEG_HEAP_BEG, U_SEG_SHARE_BEG, U_SEG_SHARE_END,
    U_SEG_STACK_BEG, U_SEG_STACK_END,
};

use crate::arch::{flush_tlb, get_curr_page_table_addr};

use super::shm_mgr::{Shm, ShmId};
use crate::executor::block_on;
use crate::fs::new_vfs::top::{MmapKind, VfsFileRef};
use crate::tools::errors::{SysError, SysResult};
use alloc::sync::Arc;
use core::ops::Range;
use log::{debug, warn};

pub type VirtAddrRange = Range<VirtAddr>;

#[inline(always)]
///! 用于迭代虚拟地址范围内的所有页, 如果首尾不是页对齐的就 panic
pub(super) fn iter_vpn(range: VirtAddrRange, mut f: impl FnMut(VirtPageNum)) {
    let start_vpn = range.start.assert_4k().page_num();
    let end_vpn = range.end.round_up().page_num(); // End vaddr may not be 4k aligned
    let mut vpn = start_vpn;
    while vpn < end_vpn {
        f(vpn);
        vpn += 1;
    }
}

#[inline(always)]
///! 用于迭代虚拟地址范围内的所有页, 如果首尾不是页对齐的, 会自动向前或向后扩展到页对齐.
///! 不知道有没有用, 先留着吧
pub(super) fn iter_vpn_extend(range: VirtAddrRange, mut f: impl FnMut(VirtPageNum)) {
    let start_vpn = range.start.round_down().page_num();
    let end_vpn = range.end.round_up().page_num();
    let mut vpn = start_vpn;
    while vpn <= end_vpn {
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

impl From<UserAreaPerm> for PTEFlags {
    fn from(val: UserAreaPerm) -> Self {
        let mut pte_flag = PTEFlags::V | PTEFlags::U;
        if val.contains(UserAreaPerm::READ) {
            pte_flag |= PTEFlags::R | PTEFlags::A; // Some hardware does not support setting A bit.
        }
        if val.contains(UserAreaPerm::WRITE) {
            pte_flag |= PTEFlags::W | PTEFlags::D; // Some hardware does not support setting D bit.
        }
        if val.contains(UserAreaPerm::EXECUTE) {
            pte_flag |= PTEFlags::X;
        }
        pte_flag
    }
}

impl From<UserAreaPerm> for PageFaultAccessType {
    fn from(val: UserAreaPerm) -> Self {
        if val.intersects(UserAreaPerm::WRITE) {
            return PageFaultAccessType::RW;
        }
        if val.intersects(UserAreaPerm::EXECUTE) {
            return PageFaultAccessType::RX;
        }
        PageFaultAccessType::RO
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

        true
    }
}

#[derive(Clone)]
enum UserAreaType {
    /// 匿名映射区域
    MmapAnonymous,
    /// 私有映射区域
    MmapPrivate {
        file: VfsFileRef,
        offset: usize,
    },
    // TODO: 共享映射区域
    // MmapShared {
    //     file: VfsFileRef,
    //     offset: usize,
    // },
    Shm {
        id: ShmId,
        shm: Arc<Shm>,
    },
}

impl Debug for UserAreaType {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            UserAreaType::MmapAnonymous => write!(f, "MmapAnonymous"),
            UserAreaType::MmapPrivate { file: _, offset } => {
                write!(f, "MmapPrivate {{ offset: {} }}", offset)
            }
            UserAreaType::Shm { id, shm: _ } => write!(f, "Shm {{ id: {} }}", id),
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

    pub fn new_private(perm: UserAreaPerm, file: VfsFileRef, offset: usize) -> Self {
        Self {
            kind: UserAreaType::MmapPrivate { file, offset },
            perm,
        }
    }

    pub fn new_shm(perm: UserAreaPerm, id: ShmId, shm: Arc<Shm>) -> Self {
        Self {
            kind: UserAreaType::Shm { id, shm },
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
        debug!(
            "page fault: {:?}, {:?}, {:?} at page table {:?}",
            self,
            access_vpn,
            access_type,
            page_table.root_paddr()
        );

        if !access_type.can_access(self.perm()) {
            return Err(PageFaultErr::PermUnmatch);
        }

        // anyway we need a new frame
        let mut frame = 0.into();
        debug_assert!(frame == 0); // depress the warning of unused value

        // perpare the data for the new frame
        let pte = page_table.get_pte_mut_from_vpn(access_vpn);
        if let Some(pte) = pte {
            // PTE valid is ensured

            // must be CoW
            let pte_flags = pte.flags();
            debug!("flags: {:?}", pte_flags);
            debug_assert!(pte_flags.contains(PTEFlags::SHARED));
            debug_assert!(!pte_flags.contains(PTEFlags::W));
            debug_assert!(self.perm().contains(UserAreaPerm::WRITE));

            // decrease the old frame's ref count
            let old_frame = pte.ppn();

            if old_frame.is_shared() {
                // must not be the last one
                old_frame.decrease();
                debug_assert!(!old_frame.is_free());

                // copy the data
                // assert we are in process's page table now
                debug_assert!(page_table.root_paddr().bits() == get_curr_page_table_addr());
                frame = alloc_frame().ok_or(PageFaultErr::KernelOOM)?;
                unsafe {
                    frame.as_mut_page_slice().copy_from_slice(old_frame.addr().as_page_slice());
                }
            } else {
                // Not shared, just set the pte to writable
                frame = old_frame.addr();
            }
        } else {
            // a lazy alloc or lazy load (demand paging)
            match &self.kind {
                // If lazy alloc, do nothing (or maybe memset it to zero?)
                UserAreaType::MmapAnonymous => {
                    frame = alloc_frame().ok_or(PageFaultErr::KernelOOM)?;
                    // https://man7.org/linux/man-pages/man2/mmap.2.html
                    // MAP_ANONYMOUS
                    // The mapping is not backed by any file; its contents are
                    // initialized to zero.
                    unsafe { frame.as_mut_page_slice().fill(0) };
                }
                // If lazy load, read from fs
                UserAreaType::MmapPrivate { file, offset } => {
                    let access_vaddr = access_vpn.addr();
                    let real_offset = offset + (access_vaddr.into() - range_begin);
                    // TODO-PERF: block on read_at
                    frame = block_on(file.get_page(real_offset, MmapKind::Private))
                        .expect("read file failed");
                    // Read length may be less than PAGE_SIZE, due to file mmap
                }
                UserAreaType::Shm { id: _, shm: _ } => {
                    panic!("shm should be mapped immediately, will never page fault")
                }
            }
        }

        debug_assert!(frame != 0);
        // remap the frame
        page_table.remap_page(access_vpn.addr(), frame, self.perm().into());
        flush_tlb(access_vpn.addr().bits());
        Ok(())
    }

    fn split_and_make_left(&mut self, split_at: VirtAddr, range: VirtAddrRange) -> Self {
        use UserAreaType::*;
        // return left-hand-side area
        match &mut self.kind {
            MmapAnonymous => UserArea::new_anonymous(self.perm),
            MmapPrivate { file, offset } => {
                let old_offset = *offset;
                // change self to become the new right-hand-side area
                *offset += split_at - range.start;
                UserArea::new_private(self.perm, file.clone(), old_offset)
            }
            Shm { id: _, shm: _ } => panic!("shm should never be split"),
        }
    }

    fn split_and_make_right(&mut self, split_at: VirtAddr, range: VirtAddrRange) -> Self {
        use UserAreaType::*;
        // change self to become the new left-hand-side area: nothing need to do
        // return right-hand-side area
        match &self.kind {
            MmapAnonymous => UserArea::new_anonymous(self.perm),
            MmapPrivate { file, offset } => {
                UserArea::new_private(self.perm, file.clone(), *offset + (split_at - range.start))
            }
            Shm { id: _, shm: _ } => panic!("shm should never be split"),
        }
    }

    /// debug only
    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            UserAreaType::MmapAnonymous => "anonymous",
            UserAreaType::MmapPrivate { .. } => "private",
            UserAreaType::Shm { .. } => "shm",
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
    const STACK_RANGE: VirtAddrRange =
        VirtAddr::from(U_SEG_STACK_BEG)..VirtAddr::from(U_SEG_STACK_END);
    const MMAP_RANGE: VirtAddrRange =
        VirtAddr::from(U_SEG_FILE_BEG)..VirtAddr::from(U_SEG_FILE_END);
    const SHARE_RANGE: VirtAddrRange =
        VirtAddr::from(U_SEG_SHARE_BEG)..VirtAddr::from(U_SEG_SHARE_END);

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
        let range = self
            .map
            .find_free_range(Self::STACK_RANGE, size, |va, n| (va + n).round_up().into())
            .expect("too many stack!");

        // 栈要 16 字节对齐
        let sp_init = VirtAddr::from((range.end.bits() - 1) & !0xf);
        debug!("alloc stack: {:x?}, sp_init: {:x?}", range, sp_init);

        let area = UserArea::new_anonymous(UserAreaPerm::READ | UserAreaPerm::WRITE);
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
        let (Range { start: _, end }, _) =
            self.map.get(Self::HEAP_BEG).expect("get heap break without heap");
        end
    }

    pub fn reset_heap_break(&mut self, new_brk: VirtAddr) -> SysResult<()> {
        // TODO-PERF: 缓存 heap 的位置，减少一次查询
        let (Range { start, end }, _) = self.map.get(Self::HEAP_BEG).expect("brk without heap");

        if end < new_brk {
            // when larger, create a new area [heap_end, new_brk), then merge it with current heap
            self.map.extend_back(start, new_brk).map_err(|_| SysError::ENOMEM)
        } else if new_brk < end {
            // when smaller, split the area into [heap_start, new_brk), [new_brk, heap_end), then remove the second one
            self.map.reduce_back(start, new_brk).map(|_| ()).map_err(|_| SysError::ENOMEM)
            // TODO: release page
        } else {
            // when equal, do nothing
            Ok(())
        }
    }

    /// for mmap private / mmap anonymous
    fn find_free_mmap_area(&self, size: usize) -> SysResult<(VirtAddr, usize)> {
        self.map
            .find_free_range(Self::MMAP_RANGE, size, |va, n| (va + n).round_up().into())
            .map(|r| (r.start, r.end - r.start))
            .ok_or(SysError::ENOMEM)
    }

    /// for mmap shared / shm
    fn find_free_share_area(&self, size: usize) -> SysResult<(VirtAddr, usize)> {
        self.map
            .find_free_range(Self::SHARE_RANGE, size, |va, n| (va + n).round_up().into())
            .map(|r| (r.start, r.end - r.start))
            .ok_or(SysError::ENOMEM)
    }

    pub fn insert_mmap_anonymous(
        &mut self,
        size: usize,
        perm: UserAreaPerm,
    ) -> SysResult<(VirtAddrRange, &UserArea)> {
        let (begin, size) = self.find_free_mmap_area(size)?;
        self.insert_mmap_anonymous_at(begin, size, perm)
    }

    pub fn insert_mmap_private(
        &mut self,
        size: usize,
        perm: UserAreaPerm,
        file: VfsFileRef,
        offset: usize,
    ) -> SysResult<(VirtAddrRange, &UserArea)> {
        let (begin, size) = self.find_free_mmap_area(size)?;
        self.insert_mmap_private_at(begin, size, perm, file, offset)
    }

    pub fn insert_shm(
        &mut self,
        perm: UserAreaPerm,
        id: ShmId,
        shm: Arc<Shm>,
    ) -> SysResult<(VirtAddrRange, &UserArea)> {
        let (begin, _) = self.find_free_share_area(shm.size())?;
        self.insert_shm_at(begin, perm, id, shm)
    }

    fn insert_at(
        &mut self,
        begin: VirtAddr,
        size: usize,
        area: UserArea,
    ) -> SysResult<(VirtAddrRange, &UserArea)> {
        let range = VirtAddrRange {
            start: begin,
            end: begin + size,
        };

        log::debug!(
            "try insert_at: {:?}, perm: {:?}, type: {}",
            range,
            area.perm(),
            area.kind_str()
        );

        self.map
            .try_insert(range.clone(), area)
            .map(|v| (range, &*v))
            .map_err(|_| SysError::ENOMEM)
    }

    pub fn insert_mmap_anonymous_at(
        &mut self,
        begin_vaddr: VirtAddr,
        size: usize,
        perm: UserAreaPerm,
    ) -> SysResult<(VirtAddrRange, &UserArea)> {
        self.insert_at(begin_vaddr, size, UserArea::new_anonymous(perm))
    }

    pub fn insert_mmap_private_at(
        &mut self,
        begin_vaddr: VirtAddr,
        size: usize,
        perm: UserAreaPerm,
        file: VfsFileRef,
        offset: usize,
    ) -> SysResult<(VirtAddrRange, &UserArea)> {
        self.insert_at(begin_vaddr, size, UserArea::new_private(perm, file, offset))
    }

    pub fn insert_shm_at(
        &mut self,
        begin_vaddr: VirtAddr,
        perm: UserAreaPerm,
        id: ShmId,
        shm: Arc<Shm>,
    ) -> SysResult<(VirtAddrRange, &UserArea)> {
        assert!(
            Self::SHARE_RANGE.contains(&begin_vaddr),
            "shm must be in share range"
        );
        self.insert_at(begin_vaddr, shm.size(), UserArea::new_shm(perm, id, shm))
    }

    pub fn page_fault(
        &mut self,
        page_table: &mut PageTable,
        access_vpn: VirtPageNum,
        access_type: PageFaultAccessType,
    ) -> Result<(), PageFaultErr> {
        let (range, area) =
            self.map.get_mut(access_vpn.addr().into()).ok_or(PageFaultErr::NoSegment)?;
        area.page_fault(page_table, range.start, access_vpn, access_type)
    }

    pub fn force_map_range(
        &mut self,
        page_table: &mut PageTable,
        range: VirtAddrRange,
        perm: UserAreaPerm,
    ) {
        debug!("force map range: {:?}, perm: {:?}", range, perm);

        let vpn_begin = range.start.assert_4k().page_num();
        let vpn_end = range.end.assert_4k().page_num();

        let mut vpn = vpn_begin;
        while vpn < vpn_end {
            self.page_fault(page_table, vpn, perm.into()).unwrap();
            vpn += 1;
        }
    }

    pub fn remove_shm(&mut self, vaddr: VirtAddr) -> SysResult<VirtAddrRange> {
        let (range, _) = self.map.get(vaddr).ok_or(SysError::EINVAL)?;
        self.map.force_remove_one(range.clone());
        Ok(range)
    }

    pub fn unmap_range(&mut self, page_table: &mut PageTable, range: VirtAddrRange) {
        debug!("unmap range: {:?}", range);
        self.map.remove(
            range,
            UserArea::split_and_make_left,
            UserArea::split_and_make_right,
            |_area, range| Self::release_range(page_table, range),
        );
    }

    /// 释放一个虚拟地址范围内的所有页
    ///
    /// **注意**: 只释放物理页，不会管分段
    pub fn release_range(page_table: &mut PageTable, range: VirtAddrRange) {
        debug!("release range: {:?}", range);
        let mut range = range;
        if range.start.round_down().bits() != range.start.bits() {
            warn!("Not aligned range start: {:x?}", range.start);
            range = Range {
                start: range.start.round_down().bits().into(),
                end: range.end.round_up().bits().into(),
            }
        }
        // 释放被删除的段
        iter_vpn(range, |vpn| {
            log::trace!("release vpn: {:x?}", vpn);
            let pte = page_table.get_pte_copied_from_vpn(vpn);
            if pte.is_none() {
                return;
            }
            let pte = pte.unwrap();
            // Remove the page from the page table.
            page_table.unmap_page(vpn.addr());
            // Decrement the reference count of the page and try to deallocate it.
            pte.ppn().decrease_and_try_dealloc();
            flush_tlb(vpn.addr().bits());
        })
    }
}
