use alloc::format;

use crate::{
    consts::{
        address_space::{U_SEG_HEAP_BEG, U_SEG_STACK_BEG},
        PAGE_SIZE, PAGE_SIZE_BITS,
    },
    memory::{
        address::{PhysAddr, VirtAddr},
        frame::alloc_frame_contiguous,
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
    tools::handler_pool::UsizePool,
};

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
    pub fn addr(&self) -> VirtAddr {
        VirtAddr(self.0 * THREAD_STACK_SIZE + U_SEG_STACK_BEG)
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
        let stack_frames = alloc_frame_contiguous(THREAD_STACK_SIZE, PAGE_SIZE_BITS)
            .expect(format!("alloc stack failed, (stack_id: {:?})", stack_id_usize).as_str());

        // 把物理页映射到对应的虚拟地址去
        self.page_table.map_region(
            stack_id.addr(),
            PhysAddr(stack_frames),
            THREAD_STACK_SIZE,
            PTEFlags::V | PTEFlags::R | PTEFlags::W | PTEFlags::U,
        );

        // 返回栈 id
        stack_id
    }

    pub fn dealloc_stack(&mut self, stack_id: StackID) {
        // 释放栈空间
        self.page_table.unmap_region(stack_id.addr(), THREAD_STACK_SIZE);
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
            PhysAddr(heap_frames),
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
