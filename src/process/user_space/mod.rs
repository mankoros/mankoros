pub mod user_area;
pub mod range_map;

use alloc::{string::String, sync::Arc, vec::Vec};



use crate::{
    consts::{
        address_space::{U_SEG_STACK_END},
    },
    fs::vfs::filesystem::VfsNode,
    memory::{
        address::{VirtAddr},
        kernel_phys_to_virt,
        pagetable::pagetable::PageTable,
    },
    process::{aux_vector::AuxElement, user_space::user_area::{PageFaultAccessType}},
    tools::handler_pool::UsizePool, arch::get_curr_page_table_addr,
};

use super::{aux_vector::AuxVector};


use self::{user_area::{UserAreaManager, PageFaultErr, VirtAddrRange}};
use log::debug;

pub const THREAD_STACK_SIZE: usize = 4 * 1024;

// TODO-PERF: 拆锁
/// 一个线程的地址空间的相关信息, 在 AliveProcessInfo 里受到进程大锁保护, 不需要加锁
pub struct UserSpace {
    // 根页表
    pub page_table: PageTable,
    // 分段管理
    areas: UserAreaManager,
    // 一个进程可能有很多栈 (各个线程都一个), 该池子维护可用的 StackID
    // 一个已分配的 stack id 代表了栈地址区域内的一段 THREAD_STACK_SIZE 长的段
    stack_id_pool: UsizePool,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct StackID(usize);

impl StackID {
    pub fn stack_bottom(&self) -> VirtAddr {
        // 栈是倒着长的 (从高地址往低地址)
        VirtAddr(U_SEG_STACK_END - self.0 * THREAD_STACK_SIZE)
    }

    pub fn stack_range(&self) -> VirtAddrRange {
        let bottom = self.stack_bottom();
        VirtAddrRange {
            start: bottom, 
            end: bottom + THREAD_STACK_SIZE,
        }
    }

    pub fn init_stack(
        self,
        args: Vec<String>,
        envp: Vec<String>,
        auxv: AuxVector,
    ) -> (usize, usize, usize, usize) {
        // spec says:
        //      In the standard RISC-V calling convention, the stack grows downward 
        //      and the stack pointer is always kept 16-byte aligned.

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
        debug!("AAAAAAAAAAAAAAAAAAAAAAAAAA");
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
        (sp, argc, arg_ptr_ptr, env_ptr_ptr)
    }
}

impl UserSpace {
    pub fn new() -> Self {
        let page_table = PageTable::new_with_kernel_seg();
        let stack_id_pool = UsizePool::new(1);
        Self {
            page_table,
            areas: UserAreaManager::new(),
            stack_id_pool,
        }
    }

    pub fn areas(&self) -> &UserAreaManager {
        &self.areas
    }

    pub fn areas_mut(&mut self) -> &mut UserAreaManager {
        &mut self.areas
    }

    /// 为线程分配一个栈空间 ID
    /// 该 id 只意味着某段虚拟地址的使用权被分配出去了, 不会产生真的物理页分配
    pub fn alloc_stack_id(&mut self) -> StackID {
        StackID(self.stack_id_pool.get())
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
                continue;
            }

            let offset = ph.offset() as usize;

            let area_begin = VirtAddr(ph.virtual_addr() as usize);
            let area_perm = ph.flags().into();
            let area_size = ph.mem_size() as usize;

            if ph.file_size() == ph.mem_size() {
                // 如果该段在文件中的大小与其被载入内存后应有的大小相同, 
                // 我们可以直接采用类似 mmap private 的方式来加载它
                // 此时, 该段的内容将会被懒加载
                self.areas_mut()
                    .insert_mmap_private_at(area_begin, area_size, area_perm, elf_file.clone(), offset)
                    .expect("failed to map elf file in a mmap-private-like way");
            } else {
                // 否则, 我们就采用类似 mmap anonymous 的方式来创建一个空白的匿名区域
                // 然后将文件中的内容复制到其中 (可能只占分配出来的空白区域的一部分)
                self.areas_mut()
                    .insert_mmap_anonymous_at(area_begin, area_size, area_perm)
                    .expect("failed to map elf file in a mmap-anonymous-like way");
                // copy data
                debug_assert!(self.page_table.root_paddr() == get_curr_page_table_addr().into());
                let area_slice = unsafe { area_begin.as_mut_slice(area_size) };
                elf_file.read_at(offset as u64, area_slice)
                    .expect("failed to copy elf data");
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
        let entry_point = VirtAddr(elf.header.pt2.entry_point() as usize);

        (entry_point, auxv)
    }

    pub fn handle_pagefault(&mut self, vaddr: VirtAddr, access_type: PageFaultAccessType) -> Result<(), PageFaultErr> {
        self.areas.page_fault(&mut self.page_table, vaddr.into(), access_type)
    }

    pub fn force_map_range(&mut self, range: VirtAddrRange) {
        self.areas.force_map_range(&mut self.page_table, range);
    }
}
