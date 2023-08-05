use super::user_space::UserSpace;
use crate::{
    arch::get_curr_page_table_addr,
    consts::{PAGE_MASK, PAGE_SIZE},
    executor::{block_on, hart_local::within_sum},
    fs::new_vfs::top::VfsFileRef,
    memory::{
        address::{PhysAddr, VirtAddr},
        frame::alloc_frame,
        kernel_phys_to_virt,
        pagetable::pte::PTEFlags,
    },
};

mod aux_vector;
pub use aux_vector::AuxElement;
pub use aux_vector::AuxVector;

impl UserSpace {
    /// Return: entry_point, auxv
    pub fn parse_and_map_elf_file(&mut self, elf_file: VfsFileRef) -> (VirtAddr, AuxVector) {
        const HEADER_LEN: usize = 1024;
        let mut header_data = [0u8; HEADER_LEN];

        // TODO-PERF: async here
        block_on(elf_file.read_at(0, header_data.as_mut_slice()))
            .expect("failed to read elf header");

        let elf = xmas_elf::ElfFile::new(header_data.as_slice()).expect("failed to parse elf");
        let elf_header = elf.header;

        let magic = elf_header.pt1.magic;
        assert_eq!(magic, [0x7f, 0x45, 0x4c, 0x46], "invalid elf!");

        // 将 elf 的各个段载入新的页中，同时找到最开头的段，将其地址作为 elf 的起始地址
        let mut elf_begin_opt = Option::None;

        for ph in elf.program_iter() {
            let ph_type = ph.get_type().expect("failed to get ph type");

            if ph_type != xmas_elf::program::Type::Load {
                continue;
            }

            let offset = ph.offset() as usize;

            let area_begin = VirtAddr::from(ph.virtual_addr() as usize);
            let area_perm = ph.flags().into();
            let area_size = ph.mem_size() as usize;
            let file_size = ph.file_size() as usize;

            if ph.file_size() == ph.mem_size() {
                // 如果该段在文件中的大小与其被载入内存后应有的大小相同，
                // 我们可以直接采用类似 mmap private 的方式来加载它
                // 此时，该段的内容将会被懒加载
                self.areas_mut()
                    .insert_mmap_private_at(
                        area_begin,
                        area_size,
                        area_perm,
                        elf_file.clone(),
                        offset,
                    )
                    .expect("failed to map elf file in a mmap-private-like way");
            } else {
                // 否则，我们就采用类似 mmap anonymous 的方式来创建一个空白的匿名区域
                // 然后将文件中的内容复制到其中 (可能只占分配出来的空白区域的一部分)
                self.areas_mut()
                    .insert_mmap_anonymous_at(area_begin, area_size, area_perm)
                    .expect("failed to map elf file in a mmap-anonymous-like way");
                // Allocate memory
                debug_assert!(
                    self.page_table.root_paddr() == PhysAddr::from(get_curr_page_table_addr())
                );
                let begin: usize = area_begin.round_down().bits();
                let begin_offset = area_begin.bits() - begin;
                let begin_residual = PAGE_SIZE - begin_offset;
                let file_end = area_begin + file_size;
                let end = (area_begin + area_size - 1).round_up().bits();
                let end_residual = (area_begin.bits() + file_size) & PAGE_MASK;
                let mut read_size = 0;
                for vaddr in (begin..end).step_by(PAGE_SIZE) {
                    let paddr = alloc_frame().expect("Out of memory");
                    self.page_table.map_page(
                        VirtAddr::from(vaddr).assert_4k(),
                        paddr,
                        PTEFlags::rw() | PTEFlags::U,
                    );
                    // Copy data
                    if vaddr < area_begin.bits() {
                        read_size += within_sum(|| {
                            // First page
                            let slice = unsafe {
                                core::slice::from_raw_parts_mut(
                                    kernel_phys_to_virt(paddr.bits() + begin_offset) as _,
                                    begin_residual,
                                )
                            };
                            block_on(elf_file.read_at(offset, slice))
                                .expect("failed to copy elf data")
                        });
                    } else if vaddr < file_end.bits() {
                        if vaddr < file_end.round_down().bits() {
                            // Normal read page
                            let slice = unsafe { paddr.as_mut_page_slice() };
                            read_size += within_sum(|| {
                                block_on(elf_file.read_at(offset + read_size, slice))
                                    .expect("failed to copy elf data")
                            });
                        } else {
                            // Last residual
                            let slice = unsafe {
                                core::slice::from_raw_parts_mut(
                                    kernel_phys_to_virt(paddr.bits()) as _,
                                    end_residual,
                                )
                            };
                            read_size += within_sum(|| {
                                block_on(elf_file.read_at(offset + read_size, slice))
                                    .expect("failed to copy elf data")
                            });
                        }
                    }
                }
                assert_eq!(read_size, file_size);

                // Set the rest to zero
                let bss_slice =
                    unsafe { (area_begin + file_size).as_mut_slice(area_size - file_size) };
                within_sum(|| {
                    bss_slice.fill(0);
                });
            }

            // 更新 elf 的起始地址
            match elf_begin_opt {
                Some(elf_begin) => {
                    if area_begin < elf_begin {
                        elf_begin_opt = Some(area_begin);
                    }
                }
                None => {
                    elf_begin_opt = Some(area_begin);
                }
            }
        }

        let elf_begin = elf_begin_opt.expect("Elf has no loadable segment!");
        let auxv = AuxVector::from_elf(&elf, elf_begin);
        let entry_point = VirtAddr::from(elf.header.pt2.entry_point() as usize);

        (entry_point, auxv)
    }
}
