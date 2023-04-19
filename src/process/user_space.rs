use alloc::{format, string::String, vec::Vec};

use crate::{
    consts::{
        address_space::{U_SEG_HEAP_BEG, U_SEG_STACK_BEG, U_SEG_STACK_END},
        PAGE_SIZE, PAGE_SIZE_BITS,
    },
    memory::{
        address::{PhysAddr, VirtAddr},
        frame::alloc_frame_contiguous,
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
    process::aux_vector::AuxElement,
    tools::handler_pool::UsizePool,
};

use super::aux_vector::AuxVector;

pub const THREAD_STACK_SIZE: usize = 4 * 1024 * 1024;
/// 一个线程的地址空间的相关信息, 在 AliveProcessInfo 里受到进程大锁保护, 不需要加锁
pub struct UserSpace {
    // 根页表
    pub page_table: PageTable,
    // 栈管理
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
        let stack_id_pool = UsizePool::new();
        Self {
            page_table,
            stack_id_pool,
            heap_page_cnt: 0,
        }
    }

    /// 为线程分配一个栈空间, pid 为调试需要
    pub fn alloc_stack(&mut self) -> StackID {
        // 获得当前可用的一块地址空间用于放该线程的栈
        let stack_id_usize = self.stack_id_pool.get();
        let stack_id = StackID(stack_id_usize);

        // 分配一个栈这么多的连续的物理页
        // TODO: 在栈末尾插入金丝雀页以检测 stack overflow
        let stack_frames = alloc_frame_contiguous(THREAD_STACK_SIZE, PAGE_SIZE_BITS)
            .expect(format!("alloc stack failed, (stack_id: {:?})", stack_id_usize).as_str());

        // 把物理页映射到对应的虚拟地址去
        self.page_table.map_region(
            stack_id.stack_bottom(),
            stack_frames,
            THREAD_STACK_SIZE,
            PTEFlags::V | PTEFlags::R | PTEFlags::W | PTEFlags::U,
        );

        // 返回栈 id
        stack_id
    }

    pub fn dealloc_stack(&mut self, stack_id: StackID) {
        // 释放栈空间
        self.page_table.unmap_region(stack_id.stack_bottom(), THREAD_STACK_SIZE);
        // 释放栈号
        self.stack_id_pool.release(stack_id.0);
    }

    pub fn alloc_heap(&mut self, page_cnt: usize) -> VirtAddr {
        let size = page_cnt * PAGE_SIZE;

        // 分配一块连续的物理页
        let heap_frames = alloc_frame_contiguous(size, PAGE_SIZE_BITS)
            .expect(format!("alloc heap failed, (size: {:?})", size).as_str());

        // 把物理页映射到对应的虚拟地址去
        let heap_addr = VirtAddr(U_SEG_HEAP_BEG + self.heap_page_cnt * PAGE_SIZE);
        self.page_table.map_region(
            heap_addr,
            heap_frames,
            size,
            PTEFlags::V | PTEFlags::R | PTEFlags::W | PTEFlags::U,
        );

        // 更新堆页数
        self.heap_page_cnt += page_cnt;

        // 返回堆地址
        heap_addr
    }

    pub fn dealloc_heap(&mut self, page_cnt: usize) -> Result<(), &str> {
        if page_cnt > self.heap_page_cnt {
            Err("dealloc heap failed, page_cnt > self.heap_page_cnt")
        } else {
            // 释放堆空间
            let heap_addr = VirtAddr(U_SEG_HEAP_BEG + (self.heap_page_cnt - page_cnt) * PAGE_SIZE);
            self.page_table.unmap_region(heap_addr, page_cnt * PAGE_SIZE);

            // 更新堆页数
            self.heap_page_cnt -= page_cnt;
            Ok(())
        }
    }
}
