use xmas_elf::ElfFile;

use crate::{
    consts::PAGE_SIZE_BITS,
    memory::{
        address::VirtAddr,
        frame::alloc_frame_contiguous,
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
};

pub fn parse_elf(elf_data: &[u8]) -> ElfFile {
    ElfFile::new(elf_data).expect("elf parse failed")
}

/// 将 elf 文件载入内存, 设置对应关系, 并返回 elf 的入口地址
pub fn map_elf_segment(elf: &ElfFile, page_table: &mut PageTable) -> Option<VirtAddr> {
    // 将 elf 的各个段载入新的页中, 同时找到最开头的段, 将其地址作为 elf 的起始地址
    let mut elf_begin_opt = Option::None;

    for segment in elf.program_iter() {
        let seg_type = segment.get_type().expect("get segment type failed");
        if seg_type == xmas_elf::program::Type::Load {
            // 分配物理页
            let seg_size = segment.mem_size() as usize;
            let phy_frames = alloc_frame_contiguous(seg_size, PAGE_SIZE_BITS)
                .expect("fail to alloc frame for elf segment");

            // 将这 ELF 段的数据复制到分配好的物理页
            // 分配器分配出来的地址都在地址空间的 "物理内存映射段", 所以即使在用户页表也可以直接写入
            let offset = segment.offset() as usize;
            let filesz = segment.file_size() as usize;
            let data = &elf.input[offset..(offset + filesz)];
            unsafe {
                let phy_frames: usize = phy_frames.into();
                (phy_frames as *mut u8).copy_from_nonoverlapping(data.as_ptr(), data.len());
            }

            // TODO: 检查起始和结束地址是否满足 p_align 的要求
            // TODO: 检查 p_align 的要求是否满足 4K 的页对齐要求
            // 将准备好的物理页映射到用户地址空间中
            let vaddr_beg = VirtAddr(segment.virtual_addr() as usize);
            let pte_flags = elf_flags_to_pte(&segment.flags());
            page_table.map_region(vaddr_beg, phy_frames, seg_size, pte_flags);

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
    }

    elf_begin_opt
}

fn elf_flags_to_pte(elf_flags: &xmas_elf::program::Flags) -> PTEFlags {
    let mut flags = PTEFlags::U;
    if elf_flags.is_read() {
        flags |= PTEFlags::R;
    }
    if elf_flags.is_write() {
        flags |= PTEFlags::W;
    }
    if elf_flags.is_execute() {
        flags |= PTEFlags::X;
    }
    flags
}

pub fn get_entry_point(elf: &ElfFile) -> VirtAddr {
    VirtAddr(elf.header.pt2.entry_point() as usize)
}
