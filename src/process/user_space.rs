use alloc::{string::String, sync::Arc, vec::Vec};
use bitflags::bitflags;

use crate::{
    consts::{
        address_space::{U_SEG_HEAP_BEG, U_SEG_STACK_END},
        PAGE_SIZE,
    },
    memory::{
        address::{PhysAddr, VirtAddr, VirtPageNum},
        frame::{alloc_frame, dealloc_frame},
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
    process::aux_vector::AuxElement,
    tools::handler_pool::UsizePool,
    vfs::filesystem::VfsNode,
};

use super::{aux_vector::AuxVector, share_page_mgr::SharedPageManager};

use riscv::register::scause;

pub const THREAD_STACK_SIZE: usize = 4 * 1024 * 1024;
/// 一个线程的地址空间的相关信息, 在 AliveProcessInfo 里受到进程大锁保护, 不需要加锁
pub struct UserSpace {
    // 根页表
    pub page_table: PageTable,
    // 分段管理
    areas: Vec<UserArea>,
    // 共享页管理
    shared_page_mgr: SharedPageManager,
    // 栈管理
    // 一个进程可能有很多栈 (各个线程都一个), 该池子维护可用的 StackID
    stack_id_pool: UsizePool,
    // 堆管理
    heap_page_cnt: usize,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StackID(usize);

impl StackID {
    pub fn stack_bottom(&self) -> VirtAddr {
        // 栈是倒着长的 (从高地址往低地址)
        VirtAddr(U_SEG_STACK_END - self.0 * THREAD_STACK_SIZE)
    }

    pub fn init_stack(
        self,
        args: Vec<String>,
        envp: Vec<String>,
        auxv: AuxVector,
    ) -> (usize, usize, usize, usize) {
        /*
        参考: https://www.cnblogs.com/likaiming/p/11193697.html
        初始化之后的栈应该长这样子:
        content                         size(bytes) + comment
        -----------------------------------------------------------------------------

        [argc = number of args]         8
        [argv[0](pointer)]              8
        [argv[1](pointer)]              8
        [argv[...](pointer)]            8 * x
        [argv[n-1](pointer)]            8
        [argv[n](pointer)]              8 (=NULL)

        [envp[0](pointer)]              8
        [envp[1](pointer)]              8
        [envp[..](pointer)]             8 * x
        [envp[term](pointer)]           8 (=NULL)

        [auxv[0](Elf64_auxv_t)]         16
        [auxv[1](Elf64_auxv_t)]         16
        [auxv[..](Elf64_auxv_t)]        16 * x
        [auxv[term](Elf64_auxv_t)]      16 (=NULL)

        [padding]                       >= 0
        [rand bytes]                    16
        [String identifying platform]   >= 0
        [padding for align]             >= 0 (sp - (get_random_int() % 8192)) & (~0xf)

        [argument ASCIIZ strings]       >= 0
        [environment ASCIIZ str]        >= 0
        --------------------------------------------------------------------------------
        在构建栈的时候, 我们从底向上塞各个东西
        */

        let mut sp = self.stack_bottom().0;

        // 存放环境与参数的字符串本身
        fn push_str(sp: &mut usize, s: &str) -> usize {
            let len = s.len();
            *sp -= len + 1; // +1 for NUL ('\0')
            unsafe {
                core::ptr::copy_nonoverlapping(s.as_ptr(), *sp as *mut u8, len);
                *(*sp as *mut u8).add(len) = 0;
            }
            *sp
        }

        let env_ptrs: Vec<usize> = envp.iter().rev().map(|s| push_str(&mut sp, s)).collect();
        let arg_ptrs: Vec<usize> = args.iter().rev().map(|s| push_str(&mut sp, s)).collect();

        // 随机对齐 (我们取 0 长度的随机对齐), 平台标识符, 随机数与对齐
        fn align16(sp: &mut usize) {
            *sp = (*sp - 1) & !0xf;
        }

        let rand_size = 0;
        let platform = "RISC-V64";
        let rand_bytes = "Meow~ O4 here;D"; // 15 + 1 char for 16bytes

        sp -= rand_size;
        push_str(&mut sp, platform);
        push_str(&mut sp, rand_bytes);
        align16(&mut sp);

        // 存放 auxv
        fn push_aux_elm(sp: &mut usize, elm: &AuxElement) {
            *sp -= core::mem::size_of::<AuxElement>();
            unsafe {
                core::ptr::write(*sp as *mut AuxElement, *elm);
            }
        }
        // 注意推栈是 "倒着" 推的, 所以先放 null, 再逆着放别的
        push_aux_elm(&mut sp, &AuxElement::NULL);
        for aux in auxv.into_iter().rev() {
            push_aux_elm(&mut sp, &aux);
        }

        // 存放 envp 与 argv 指针
        fn push_usize(sp: &mut usize, ptr: usize) {
            *sp -= core::mem::size_of::<usize>();
            unsafe {
                core::ptr::write(*sp as *mut usize, ptr);
            }
        }

        push_usize(&mut sp, 0);
        env_ptrs.iter().for_each(|ptr| push_usize(&mut sp, *ptr));
        let env_ptr_ptr = sp;

        push_usize(&mut sp, 0);
        arg_ptrs.iter().for_each(|ptr| push_usize(&mut sp, *ptr));
        let arg_ptr_ptr = sp;

        // 存放 argc
        let argc = args.len();
        push_usize(&mut sp, argc);

        // 返回值
        (
            sp,          // 栈顶
            argc,        // argc
            arg_ptr_ptr, // argv
            env_ptr_ptr, // envp
        )
    }
}

impl UserSpace {
    pub fn new() -> Self {
        let page_table = PageTable::new();
        // TODO: Map kernel huge page in
        let stack_id_pool = UsizePool::new();
        Self {
            page_table,
            areas: Vec::new(),
            shared_page_mgr: SharedPageManager::new(),
            stack_id_pool,
            heap_page_cnt: 0,
        }
    }

    /// 加入对应的区域映射, 实际写入页表中.
    pub fn add_area(&mut self, mut map_area: UserArea) {
        map_area.map(&mut self.page_table);
        self.areas.push(map_area);
    }

    pub fn add_area_with_file_content(
        &mut self,
        mut map_area: UserArea,
        file: &Arc<dyn VfsNode>,
        offset: usize,
    ) {
        map_area.map_with_file_content(&mut self.page_table, file, offset);
        self.areas.push(map_area);
    }

    /// 只将区域映射加入管理, 不实际写入页表
    pub fn add_area_delay(&mut self, map_area: UserArea) {
        self.areas.push(map_area);
    }

    pub fn remove_whole_area_containing(&mut self, vaddr: VirtAddr) {
        if let Some((idx, area)) =
            self.areas.iter_mut().enumerate().find(|(_, area)| area.range().contains(vaddr))
        {
            area.unmap(&mut self.page_table);
            self.areas.remove(idx);
        }
    }

    /// 为线程分配一个栈空间 ID
    /// 该 id 只意味着某段虚拟地址的使用权被分配出去了, 不会产生真的物理页分配
    pub fn alloc_stack_id(&mut self) -> StackID {
        StackID(self.stack_id_pool.get())
    }

    /// 分配一个栈
    /// 实际将某个 StackID 代表的虚拟地址空间映射到物理页上, 会进行物理页分配
    pub fn alloc_stack(&mut self, stack_id: StackID) {
        let area = UserArea::new_framed(
            VirtAddrRange::new_beg_size(stack_id.stack_bottom(), THREAD_STACK_SIZE),
            UserAreaPerm::READ | UserAreaPerm::WRITE,
        );
    
        self.add_area(area);
    }

    pub fn dealloc_stack(&mut self, stack_id: StackID) {
        // 释放栈空间
        self.remove_whole_area_containing(stack_id.stack_bottom());
        // 释放栈号
        self.stack_id_pool.release(stack_id.0);
    }

    pub fn alloc_heap(&mut self, page_cnt: usize) {
        let size = page_cnt * PAGE_SIZE;

        let area = UserArea::new_framed(
            VirtAddrRange::new_beg_size(U_SEG_HEAP_BEG.into(), size),
            UserAreaPerm::READ | UserAreaPerm::WRITE,
        );

        self.add_area(area);
    }

    pub fn dealloc_heap(&mut self) {
        self.remove_whole_area_containing(U_SEG_HEAP_BEG.into());
    }

    /// Return: entry_point, auxv
    pub fn parse_and_map_elf_file(&mut self, elf_file: Arc<dyn VfsNode>) -> (VirtAddr, AuxVector) {
        const HEADER_LEN: usize = 1024;
        let mut header_data = [0u8; HEADER_LEN];
        elf_file.read_at(0, header_data.as_mut()).expect("failed to read elf header");

        let elf = xmas_elf::ElfFile::new(&header_data.as_slice()).expect("failed to parse elf");
        let elf_header = elf.header;

        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");

        // 将 elf 的各个段载入新的页中, 同时找到最开头的段, 将其地址作为 elf 的起始地址
        let mut elf_begin_opt = Option::None;

        for ph in elf.program_iter() {
            let ph_type = ph.get_type().expect("failed to get ph type");

            if ph_type != xmas_elf::program::Type::Load {
                todo!();
            }

            let offset = ph.offset() as usize;

            let vaddr_beg = VirtAddr(ph.virtual_addr() as usize);
            let seg_size = ph.mem_size() as usize;

            let area_range = VirtAddrRange::new_beg_size(vaddr_beg, seg_size);
            let area_flags = elf_flags_to_area(ph.flags());

            let lazy = ph.file_size() == ph.mem_size();
            if lazy {
                let area = UserArea::new_file(area_range, area_flags, elf_file.clone(), offset);
                // 懒加载这段文件
                self.add_area_delay(area);
            } else {
                let area = UserArea::new_framed(area_range, area_flags);
                self.add_area_with_file_content(area, &elf_file, offset);
            }

            // 尝试更新 elf 的起始地址
            // TODO: 这样对吗? 怎么感觉不太对劲, 这 elf 真的是地址小的放在前面吗?
            match elf_begin_opt {
                Some(elf_begin) => {
                    if vaddr_beg < elf_begin {
                        elf_begin_opt = Some(vaddr_beg);
                    }
                }
                None => {
                    elf_begin_opt = Some(vaddr_beg);
                }
            }
        }

        let elf_begin = elf_begin_opt.expect("Elf has no loadable segment!");
        let auxv = AuxVector::from_elf(&elf, elf_begin);
        let entry_point = VirtAddr(elf.header.pt2.entry_point() as usize);

        (entry_point, auxv)
    }
}

fn elf_flags_to_area(flags: xmas_elf::program::Flags) -> UserAreaPerm {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtAddrRange {
    begin: VirtAddr,
    end: VirtAddr,
}

pub struct VARangeVPNIter {
    range: VirtAddrRange,
    curr: VirtPageNum,
}

impl Iterator for VARangeVPNIter {
    type Item = VirtPageNum;
    fn next(&mut self) -> Option<Self::Item> {
        if self.curr < self.range.end().into() {
            let ret = self.curr;
            self.curr = self.curr + 1;
            Some(ret)
        } else {
            None
        }
    }
}

impl VirtAddrRange {
    /// Left Inclusive, Right Exclusive range
    pub fn new_lire(begin: VirtAddr, end: VirtAddr) -> Self {
        debug_assert!(begin <= end);
        Self { begin, end }
    }

    pub fn new_beg_size(begin: VirtAddr, size: usize) -> Self {
        Self {
            begin,
            end: begin + size,
        }
    }

    pub fn begin(&self) -> VirtAddr {
        self.begin
    }

    pub fn end(&self) -> VirtAddr {
        self.end
    }

    pub fn size(&self) -> usize {
        self.end.0 - self.begin.0
    }

    pub fn contains(&self, addr: VirtAddr) -> bool {
        self.begin <= addr && addr < self.end
    }

    pub fn empty(&self) -> bool {
        self.begin == self.end
    }

    pub fn vpn_iter(&self) -> VARangeVPNIter {
        VARangeVPNIter {
            range: self.clone(),
            curr: self.begin.into(),
        }
    }

    pub fn from_begin(&self, vpn: VirtPageNum) -> usize {
        let vaddr: VirtAddr = vpn.into();
        vaddr - self.begin
    }
}

bitflags! {
    pub struct UserAreaPerm: u8 {
        const READ = 1 << 0;
        const WRITE = 1 << 1;
        const EXECUTE = 1 << 2;
    }
}

impl UserAreaPerm {
    pub fn to_normal_pte_flag(self) -> PTEFlags {
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

enum UserAreaType {
    /// 正常的, 从虚拟地址到一个可能不同的物理地址的模式
    Framed,
    /// 文件映射区域
    File {
        // TODO: add file mapping area data
        file: Arc<dyn VfsNode>,
        offset: usize,
    },
}
pub struct UserArea {
    kind: UserAreaType,
    range: VirtAddrRange,
    perm: UserAreaPerm,
}

impl UserArea {
    pub fn new_framed(range: VirtAddrRange, perm: UserAreaPerm) -> Self {
        Self {
            kind: UserAreaType::Framed,
            range,
            perm,
        }
    }

    pub fn new_file(
        range: VirtAddrRange,
        perm: UserAreaPerm,
        file: Arc<dyn VfsNode>,
        offset: usize,
    ) -> Self {
        Self {
            kind: UserAreaType::File { file, offset },
            range,
            perm,
        }
    }

    pub fn range(&self) -> &VirtAddrRange {
        &self.range
    }

    pub fn perm(&self) -> UserAreaPerm {
        self.perm
    }

    /// 将自己所表示的范围内的所有页映射到页表中
    /// 只会进行新物理页的分配
    pub fn map(&mut self, page_table: &mut PageTable) {
        for vpn in self.range.vpn_iter() {
            self.map_one(vpn, page_table);
        }
    }

    /// 将自己所表示的范围内的所有页映射到页表中
    /// 会将文件内容读入到物理页中
    pub fn map_with_file_content(
        &mut self,
        page_table: &mut PageTable,
        file: &Arc<dyn VfsNode>,
        offset: usize,
    ) {
        for vpn in self.range.vpn_iter() {
            let frame = self.map_one(vpn, page_table);
            let slice = unsafe { frame.as_mut_page_slice() };
            file.read_at((offset + self.range.from_begin(vpn)) as u64, slice)
                .expect("read file failed");
        }
    }

    /// map 单个页, 效果详见 [`map()`]
    pub fn map_one(&mut self, vpn: VirtPageNum, page_table: &mut PageTable) -> PhysAddr {
        let frame = alloc_frame().expect("alloc frame failed");
        page_table.map_page(vpn.into(), frame, self.perm().to_normal_pte_flag());
        frame
    }

    /// 将自己所表示的范围内的所有页的映射从页表中删除
    /// 如果是 Framed, 会进行物理页的释放
    pub fn unmap(&mut self, page_table: &mut PageTable) {
        for vpn in self.range.vpn_iter() {
            self.unmap_one(vpn, page_table);
        }
    }

    /// unmap 单个页, 效果详见 [`unmap()`]
    pub fn unmap_one(&mut self, vpn: VirtPageNum, page_table: &mut PageTable) {
        let paddr = page_table.unmap_page(vpn.into());
        dealloc_frame(paddr);
        // TODO: check share page
    }

    pub fn page_fault(
        &mut self,
        page_table: &mut PageTable,
        access_vpn: VirtPageNum,
        access_type: PageFaultAccessType,
    ) {
        if !access_type.can_access(self.perm()) {
            todo!("kill the program")
        }

        let pte = page_table.get_pte_copied_from_vpn(access_vpn.into());
        if let None = pte {
            todo!("kill the program")
        }
        let _pte = pte.unwrap();

        // TODO: use pte to check whether it is under CoW

        // now assume it is not CoW, just lazy alloc
        let frame = alloc_frame().expect("alloc frame failed");
        match &self.kind {
            UserAreaType::Framed => {}
            UserAreaType::File { file, offset } => {
                let access_vaddr: VirtAddr = access_vpn.into();
                let page_offset = offset + (access_vaddr - self.range.begin());

                let slice = unsafe { frame.as_mut_page_slice() };
                file.read_at(page_offset as u64, slice).expect("read file failed");
            }
        }
        page_table.map_page(access_vpn.into(), frame, self.perm().to_normal_pte_flag());
    }
}
