use alloc::sync::Arc;
use bitflags::bitflags;
use log::debug;

use crate::{
    consts,
    fs::vfs::filesystem::VfsNode,
    memory::{
        address::{PhysAddr, VirtPageNum},
        frame::{alloc_frame, dealloc_frame},
        kernel_phys_to_virt,
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
};

use super::range::VirtAddrRange;

use riscv::register::scause;

pub fn elf_flags_to_area(flags: xmas_elf::program::Flags) -> UserAreaPerm {
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
pub struct UserArea {
    kind: UserAreaType,
    range: VirtAddrRange,
    perm: UserAreaPerm,
}

impl UserArea {
    pub fn new_framed(range: VirtAddrRange, perm: UserAreaPerm) -> Self {
        Self {
            kind: UserAreaType::MmapAnonymous,
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
            kind: UserAreaType::MmapPrivate { file, offset },
            range,
            perm,
        }
    }

    pub fn range(&self) -> &VirtAddrRange {
        &self.range
    }
    pub fn range_mut(&mut self) -> &mut VirtAddrRange {
        &mut self.range
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
        debug!("Mapping {:#x} to {:#x}", vpn, frame);
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

        let _pte = page_table.get_pte_copied_from_vpn(access_vpn.into());
        // if let None = pte {
        //     todo!("kill the program")
        // }
        // let _pte = pte.unwrap();

        // TODO: use pte to check whether it is under CoW

        // now assume it is not CoW, just lazy alloc
        let frame = alloc_frame().expect("alloc frame failed");
        match &self.kind {
            // If lazy alloc, map the allocate frame
            UserAreaType::MmapAnonymous => {}
            // If is file, read from fs
            UserAreaType::MmapPrivate { file, offset } => {
                let slice = unsafe {
                    core::slice::from_raw_parts_mut(
                        kernel_phys_to_virt(frame.into()) as *mut u8,
                        consts::PAGE_SIZE,
                    )
                };
                let read_length = file.read_at(*offset as u64, slice).expect("read file failed");
                assert_eq!(read_length, consts::PAGE_SIZE);
            }
        }
        page_table.map_page(access_vpn.into(), frame, self.perm().to_normal_pte_flag());
    }
}
