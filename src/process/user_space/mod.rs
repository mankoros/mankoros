pub mod range_map;
pub mod shm_mgr;
pub mod user_area;

use alloc::{string::String, sync::Arc, vec::Vec};

use crate::{
    arch::flush_tlb,
    memory::{
        address::{iter_vpn, VirtAddr, VirtAddrRange},
        pagetable::pagetable::PageTable,
    },
    process::{elf::AuxElement, user_space::user_area::PageFaultAccessType},
    tools::errors::SysResult,
};

use super::{elf::AuxVector, pid::Pid};

use self::{
    shm_mgr::{Shm, ShmId},
    user_area::{PageFaultErr, UserAreaManager, UserAreaPerm},
};
use log::{debug, trace};

pub const THREAD_STACK_SIZE: usize = 16 * 1024;

// TODO-PERF: 拆锁
/// 一个线程的地址空间的相关信息，在 AliveProcessInfo 里受到进程大锁保护，不需要加锁
pub struct UserSpace {
    // 根页表
    pub page_table: PageTable,
    // 分段管理
    areas: UserAreaManager,
}

pub fn init_stack(
    sp_init: VirtAddr,
    args: Vec<String>,
    envp: Vec<String>,
    auxv: AuxVector,
) -> (usize, usize, usize, usize) {
    // spec says:
    //      In the standard RISC-V calling convention, the stack grows downward
    //      and the stack pointer is always kept 16-byte aligned.

    /*
    参考：https://www.cnblogs.com/likaiming/p/11193697.html
    初始化之后的栈应该长这样子：
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
    在构建栈的时候，我们从底向上塞各个东西
    */

    let mut sp = sp_init.bits();
    debug_assert!(sp & 0xf == 0);

    // 存放环境与参数的字符串本身
    fn push_str(sp: &mut usize, s: &str) -> usize {
        let len = s.len();
        *sp -= len + 1; // +1 for NUL ('\0')
        unsafe {
            // core::ptr::copy_nonoverlapping(s.as_ptr(), *sp as *mut u8, len);
            for (i, c) in s.bytes().enumerate() {
                trace!(
                    "push_str: {:x} ({:x}) <- {:?}",
                    *sp + i,
                    i,
                    core::str::from_utf8_unchecked(&[c])
                );
                *((*sp as *mut u8).add(i)) = c;
            }
            *(*sp as *mut u8).add(len) = 0u8;
        }
        *sp
    }

    let env_ptrs: Vec<usize> = envp.iter().rev().map(|s| push_str(&mut sp, s)).collect();
    let arg_ptrs: Vec<usize> = args.iter().rev().map(|s| push_str(&mut sp, s)).collect();

    // 随机对齐 (我们取 0 长度的随机对齐), 平台标识符，随机数与对齐
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
    // 注意推栈是 "倒着" 推的，所以先放 null, 再逆着放别的
    push_aux_elm(&mut sp, &AuxElement::NULL);
    for aux in auxv.into_iter().rev() {
        push_aux_elm(&mut sp, &aux);
    }

    // 存放 envp 与 argv 指针
    fn push_usize(sp: &mut usize, ptr: usize) {
        *sp -= core::mem::size_of::<usize>();
        debug!("addr: 0x{:x}, content: {:x}", *sp, ptr);
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

impl UserSpace {
    pub fn new() -> Self {
        Self {
            page_table: PageTable::new_with_kernel_seg(),
            areas: UserAreaManager::new(),
        }
    }

    pub fn areas(&self) -> &UserAreaManager {
        &self.areas
    }

    pub fn areas_mut(&mut self) -> &mut UserAreaManager {
        &mut self.areas
    }

    pub fn has_perm(&self, vaddr: VirtAddr, perm: UserAreaPerm) -> bool {
        self.areas.get_area(vaddr).map(|a| a.perm().contains(perm)).unwrap_or(false)
    }

    pub fn attach_shm(
        &mut self,
        vaddr: Option<VirtAddr>,
        pid: Pid,
        id: ShmId,
        shm: Arc<Shm>,
        perm: UserAreaPerm,
    ) -> SysResult<VirtAddr> {
        let (range, _) = match vaddr {
            Some(vaddr) => self.areas.insert_shm_at(vaddr, perm, id, shm.clone()),
            None => self.areas.insert_shm(perm, id, shm.clone()),
        }?;

        let mut vaddr4k = range.start.assert_4k();
        for frame in shm.attach(pid) {
            frame.page_num().increase();
            self.page_table.map_page(vaddr4k, *frame, perm.into());
            vaddr4k = vaddr4k.next_page();
        }

        Ok(range.start)
    }

    pub fn detach_shm(&mut self, vaddr: VirtAddr) -> SysResult {
        let range = self.areas.remove_shm(vaddr)?;
        iter_vpn(range, |vpn| {
            let paddr = self.page_table.unmap_page(vpn.addr());
            paddr.page_num().decrease_and_try_dealloc();
            flush_tlb(vaddr.bits());
        });
        Ok(())
    }

    pub fn handle_pagefault(
        &mut self,
        vaddr: VirtAddr,
        access_type: PageFaultAccessType,
    ) -> Result<(), PageFaultErr> {
        debug!(
            "Page fault at {:x?} with {:?} (pgt: {:x?})",
            vaddr,
            access_type,
            self.page_table.root_paddr()
        );
        self.areas.page_fault(&mut self.page_table, vaddr.page_num_down(), access_type)
    }

    pub fn force_map_range(&mut self, range: VirtAddrRange, perm: UserAreaPerm) {
        self.areas.force_map_range(&mut self.page_table, range, perm);
    }

    pub fn force_map_buf(&mut self, buf: &[u8], perm: UserAreaPerm) {
        if buf.len() == 0 {
            return;
        }
        let begin = VirtAddr::from(buf.as_ptr() as usize);
        let end = begin + buf.len();
        self.force_map_range(begin..end, perm)
    }

    /// 将 vaddr 所在的区域的所有页强制分配
    pub fn force_map_area(&mut self, vaddr: VirtAddr) {
        let (range, area) = self.areas.get(vaddr).unwrap();
        self.force_map_range(range, area.perm());
    }

    pub fn clone_cow(&mut self) -> Self {
        Self {
            page_table: self
                .page_table
                .copy_table_and_mark_self_cow(|frame_paddr| frame_paddr.page_num().increase()),
            areas: self.areas.clone(),
        }
    }

    pub fn unmap_range(&mut self, range: VirtAddrRange) {
        self.areas.unmap_range(&mut self.page_table, range);
    }
}

impl Drop for UserSpace {
    fn drop(&mut self) {
        let areas = &mut self.areas;
        let page_table = &mut self.page_table;
        debug!(
            "drop user space with page table at {:x?}",
            page_table.root_paddr()
        );

        for (range, _) in areas.iter() {
            UserAreaManager::release_range(page_table, range);
        }

        drop(areas);
        drop(page_table);
        log::debug!("drop user space done")
    }
}
