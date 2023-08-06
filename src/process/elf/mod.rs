use crate::{
    executor::hart_local::AutoSUM, fs::new_vfs::top::VfsFileRef, memory::address::VirtAddr,
    process::user_space::user_area::UserAreaPerm, tools::errors::SysResult,
};

mod aux_vector;
pub mod info;
use super::lproc::LightProcess;
use crate::tools::sync_ptr::SyncMutPtr;
pub use aux_vector::AuxElement;
pub use aux_vector::AuxVector;
use core::panic;

impl LightProcess {
    /// Return: entry_point, auxv
    pub async fn parse_and_map_elf_file_async(
        &self,
        elf_file: VfsFileRef,
    ) -> SysResult<(VirtAddr, AuxVector)> {
        use info::{parse, PhType};

        let elf = parse(&elf_file).await?;
        let mut elf_begin = VirtAddr::from(usize::MAX);

        for i in 0..elf.ph_count() {
            let ph = elf.program_header(i).await?;
            if ph.type_()? != PhType::Load {
                continue;
            }

            let mem_begin = VirtAddr::from(ph.virtual_addr as usize);
            let mem_end = VirtAddr::from((ph.virtual_addr + ph.mem_size) as usize);

            let aligned_mem_begin = mem_begin.floor().into();
            let aligned_mem_end = mem_end.ceil().into();
            let aligned_mem_size = aligned_mem_end - aligned_mem_begin;

            let area_perm: UserAreaPerm = ph.flags.into();
            let file_offset = ph.offset as usize;

            if ph.mem_size == ph.file_size {
                let align_begin_offset = mem_begin - aligned_mem_begin;
                let aligned_file_offset = file_offset - align_begin_offset;
                let file_clone = elf_file.clone();
                self.with_mut_memory(|m| {
                    m.areas_mut()
                        .insert_mmap_private_at(
                            aligned_mem_begin,
                            aligned_mem_size,
                            area_perm,
                            file_clone,
                            aligned_file_offset,
                        )
                        .unwrap();
                });
            } else {
                // Some LOAD segments may be empty (e.g. .bss sections).
                // or worse, the file size is smaller than the mem size but not zero.

                // amb---mb-------------------------------me---ame
                //       fb---------------fe

                // ensure the memory area [amb, ame) is mapped
                self.with_mut_memory(|m| {
                    m.areas_mut()
                        .insert_mmap_anonymous_at(aligned_mem_begin, aligned_mem_size, area_perm)
                        .unwrap();
                    m.force_map_area(aligned_mem_begin);
                });

                // fill file contents or zeros
                let _auto_sum = AutoSUM::new();
                unsafe {
                    let mut ptr = SyncMutPtr::<u8>::new_usize(mem_begin.bits());
                    log::debug!("Start load segment: {:#x}", ptr.get() as usize);

                    // fill [fb, fe) with file content
                    let len = ph.file_size as usize;
                    let slice = core::slice::from_raw_parts_mut(ptr.get(), len);
                    assert_eq!(elf_file.read_at(file_offset, slice).await?, len);
                    ptr = ptr.add(len);
                    log::debug!("Finish [mb/fb, fe): {:#x}", ptr.get() as usize);

                    // fill [fe, me) with zeros
                    let len = mem_end - (mem_begin + ph.file_size as usize);
                    ptr.write_bytes(0, len);
                    ptr = ptr.add(len);
                    log::debug!("Finish [fe, me): {:#x}", ptr.get() as usize);

                    // left [amb, mb) and [me, ame) untouched

                    debug_assert!(ptr.get() as usize == mem_end.bits());
                }

                const SHOULD_PRINT_HASH: bool = false;
                if SHOULD_PRINT_HASH {
                    use crate::memory::address::iter_vpn;
                    use crate::tools::exam_hash;
                    iter_vpn(aligned_mem_begin..aligned_mem_end, |vpn| {
                        log::info!(
                            "vpn: {:x}, hash: {:#x}",
                            vpn,
                            exam_hash(unsafe { vpn.addr().as_page_slice() })
                        )
                    })
                }
            }

            // 更新 elf 的起始地址
            if mem_begin < elf_begin {
                elf_begin = mem_begin;
            }
        }

        if elf_begin.bits() == usize::MAX {
            panic!("Elf has no loadable segment!");
        }

        let auxv = AuxVector::from_elf_analyzer(&elf, elf_begin);
        let entry_point = VirtAddr::from(elf.pt2.entry_point as usize);

        Ok((entry_point, auxv))
    }
}
