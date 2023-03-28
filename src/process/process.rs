use core::{mem, sync::atomic::AtomicI32};

use alloc::{boxed::Box, collections::BTreeMap, format, rc::Weak, sync::Arc, vec::Vec};

use crate::{
    consts::{address_space::U_SEG_STACK_BEG, PAGE_SIZE_BITS},
    here,
    memory::{
        address::{PhysAddr, VirtAddr},
        frame::{alloc_frame, alloc_frame_contiguous, dealloc_frame, FRAME_ALLOCATOR},
        pagetable::{pagetable::PageTable, pte::PTEFlags},
    },
    sync::{mutex::Mutex, SpinNoIrqLock},
    tools::handler_pool::UsizePool,
};

static PID_USIZE_POOL: SpinNoIrqLock<UsizePool> = SpinNoIrqLock::new(UsizePool::new());

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Pid(usize);
pub struct PidHandler(Pid);
impl PidHandler {
    pub fn pid(&self) -> Pid {
        self.0
    }

    pub fn pid_usize(&self) -> usize {
        self.pid().0
    }
}
impl Drop for PidHandler {
    fn drop(&mut self) {
        PID_USIZE_POOL.lock(here!()).release(self.pid_usize());
    }
}
pub fn alloc_pid() -> PidHandler {
    let pid_usize = PID_USIZE_POOL.lock(here!()).get();
    PidHandler(Pid(pid_usize))
}

/// 资源分配单位信息块
/// 其实就是进程信息块
pub struct ProcessInfo {
    // 进程就算是死了, 其它人可能也需要拿着 pid 去查找它的状态
    // 这里的数据必须等到这个进程完全没人要了, 才能释放
    pid: PidHandler,
    // 进程可能并发地寄, 比如考虑一个进程的 pid 被多个线程持有, 它分别地尝试 kill 这个进程
    // TODO: 然后就怎么样来着?
    exit_code: AtomicI32,
    // 而这里的数据一旦进程死了 (exit) 了, 就可以丢掉了
    // 因为数据可能被多个线程访问, 所以要加锁
    // 因为进程可能不是活着, 所以要加 Option
    alive: SpinNoIrqLock<Option<AliveProcessInfo>>,
}

impl ProcessInfo {
    pub fn with_mut_alive<T>(&mut self, f: impl FnOnce(&mut AliveProcessInfo) -> T) -> Option<T> {
        self.alive.lock(here!()).as_mut().map(f)
    }
}

// 这个结构目前有 pre 进程的大锁保护, 内部的信息暂时都不用加锁
pub struct AliveProcessInfo {
    // === 进程树数据 ===
    // 进程可能没有父进程 (init), 所以要 Option
    // 进程的父进程的所有权不应该被子进程持有 (要不然就循环引用了), 所以要 Weak
    // 进程的父进程在 fork 的时候被设置 (即构造时决定), 设置完了就不可变, 只读, 所以不需要加锁
    parent: Option<Weak<ProcessInfo>>,
    // 进程要持有它的子进程信息, 因为一旦进程活着, 它就有可能去访问它的子进程的退出码什么的
    // 所以父进程的 alive 会保证子进程的信息都不被释放; 而当父进程 exit 了, 这些信息就可以丢掉了
    // 子进程可能并发地被创建, 所以要加锁
    // 子进程可以有很多个, 所以要 Vec
    // 进程信息块可能被很多东西持有 (比如线程), 所以需要使用 Arc 维护
    children: Vec<Arc<ProcessInfo>>,

    // 进程所持有的线程
    // 进程活着, 线程的信息就不能被释放, 所以直接 Thread
    // 线程去世之后, 这个 map 就要更新以删除线程, 而线程可能并发去世, 所以要加锁
    threads: BTreeMap<Tid, ThreadInfo>,
    // 管理 TID 的池子, 因为有可能并发创造 thread, 所以要加锁
    tid_usize_pool: UsizePool,
    // === 进程地址空间数据 ===
    user_space: UserSpace,
    // TODO: FD Table
}

impl AliveProcessInfo {
    pub fn create_init_thread(&mut self) -> ThreadInfo {
        todo!()
    }
}

// ================ 线程 =================

pub struct Tid(usize);
pub struct ThreadInfo {
    tid: Tid,
    alive: SpinNoIrqLock<Option<AliveThreadInfo>>,
}

// 这个结构目前有 pre 进程的大锁保护, 内部的信息暂时都不用加锁
pub struct AliveThreadInfo {
    // 线程活着, 进程就不能死
    process: Arc<ProcessInfo>,
    // 线程所占据的栈空间的 ID, 不可变, 线程死掉对应的栈就要释放给进程管理器
    stack_id: StackID,
}

impl Drop for AliveThreadInfo {
    // 线程去世以后需要做的事情
    fn drop(&mut self) {
        self.process.with_mut_alive(|alive| {
            // 释放栈空间和栈号
            alive.user_space.dealloc_stack(self.stack_id);
        });
    }
}

// ================ 地址空间 =================
pub const THREAD_STACK_SIZE: usize = 4 * 1024 * 1024;
/// 一个线程的地址空间的相关信息, 在 AliveProcessInfo 里受到进程大锁保护, 不需要加锁
pub struct UserSpace {
    // 根页表
    page_table: PageTable,
    // 栈管理器
    stack_id_pool: UsizePool,
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
}
