use core::{cell::SyncUnsafeCell, sync::atomic::AtomicI32};

use alloc::{
    alloc::Global, boxed::Box, collections::BTreeMap, format, string::String, sync::Arc,
    sync::Weak, vec::Vec,
};
use riscv::register::sstatus;

use crate::{
    here,
    memory::address::PhysAddr,
    sync::SpinNoIrqLock,
    trap::context::UKContext,
    vfs::filesystem::VfsNode,
};

use super::{
    pid_tid::{alloc_pid, alloc_tid, Pid, PidHandler, Tid, TidHandler},
    user_space::{StackID, UserSpace},
    userloop,
};

/// 资源分配单位信息块 (其实就是进程信息块)
/// 应该交给 Arc 维护, 只要当前系统中存在对它的引用, 它就不能被释放
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
    pub fn pid(&self) -> Pid {
        self.pid.pid()
    }

    pub fn with_alive<T>(&self, f: impl FnOnce(&mut AliveProcessInfo) -> T) -> T {
        self.with_alive_or_dead(f).expect(
            format!(
                "process {} is dead when trying to access alive",
                self.pid.pid_usize()
            )
            .as_str(),
        )
    }

    pub fn with_alive_or_dead<T>(&self, f: impl FnOnce(&mut AliveProcessInfo) -> T) -> Option<T> {
        self.alive.lock(here!()).as_mut().map(f)
    }

    pub fn get_page_table_addr(&self) -> PhysAddr {
        self.with_alive(|alive| alive.user_space.page_table.root_paddr())
    }

    /// 创建一个新的空白进程, 不进行除了 Pid 和结构体本身的内存之外的任何分配
    pub fn new() -> Arc<Self> {
        let pid_handler = alloc_pid();
        let alive = SpinNoIrqLock::new(Some(AliveProcessInfo {
            parent: None,
            children: Vec::new(),
            threads: BTreeMap::new(),
            user_space: UserSpace::new(),
        }));
        Arc::new(ProcessInfo {
            pid: pid_handler,
            exit_code: AtomicI32::new(0),
            alive,
        })
    }

    /// 创建该进程的第一个线程
    pub fn create_first_thread(self: Arc<ProcessInfo>) -> Arc<ThreadInfo> {
        let thread = ThreadInfo::new(self.clone());
        // 开一个小小的堆
        // TODO: 将其改为完全的懒加载
        self.with_alive(|a| { a.user_space.alloc_heap(1); });
        thread
    }

    // process 里的方法只进行资源准备
    // thread 里的方法才进行 fork/clone/exec 等控制流相关的东西
}

// 这个结构目前有 pre 进程的大锁保护, 内部的信息暂时都不用加锁
// 这个结构整体都是可变的, 并且所有权永远排他地属于一个进程
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
    // 进程活着, 线程的信息就不能被释放, 但是由于活着的线程中存在对进程的引用,
    // 而进程如果活着又会占有这个, 所以这里必须使用 Weak 防止循环引用
    threads: BTreeMap<Tid, Weak<ThreadInfo>>,
    // === 进程地址空间数据 ===
    user_space: UserSpace,
    // TODO: FD Table
}

// ================ 线程 =================
// 这个结构需要使用 Arc 处理所有权
pub struct ThreadInfo {
    pub tid: TidHandler,
    // 线程信息还存在, 进程信息就得存在
    pub process: Arc<ProcessInfo>,
    // 线程局部信息, 只能该线程修改, 不用加锁, 但可变
    inner: SyncUnsafeCell<ThreadInfoInner>,
}

impl ThreadInfo {
    pub fn tid(&self) -> Tid {
        self.tid.tid()
    }

    pub fn context(&self) -> &mut UKContext {
        unsafe { &mut (&mut *self.inner.get()).uk_conext }
    }

    pub fn stack_id(&self) -> StackID {
        unsafe { (&*self.inner.get()).stack_id }
    }

    pub fn new(process: Arc<ProcessInfo>) -> Arc<Self> {
        // 分配新的 TID
        let tid_handler = alloc_tid();
        let tid = tid_handler.tid();

        // 分配新的栈
        // TODO: 拆分分配 stack id 和 stack 资源的过程, 让 init 完全不涉及处理 id 之外的资源的分配
        let stack_id = process.with_alive(|alive| alive.user_space.alloc_stack_id());

        // 构建初始 Thread 结构体
        let thread = Arc::new(ThreadInfo {
            tid: tid_handler,
            process: process.clone(),
            inner: SyncUnsafeCell::new(ThreadInfoInner {
                stack_id,
                uk_conext: unsafe { UKContext::new_uninit() },
            }),
        });

        // 把新的线程加入到进程的线程列表中
        process.with_alive(|alive| {
            alive.threads.insert(tid, Arc::downgrade(&thread));
        });

        thread
    }

    /// 线程的第一次 exec, 同时必须还得是进程的第一次 exec
    // Big-TODO: 考虑 remap, 这里默认进程之前没有 map 过文件
    pub fn exec_first(self: Arc<Self>, elf_file: Arc<dyn VfsNode>, args: Vec<String>, envp: Vec<String>) {
        // 把 elf 的 segment 映射到用户空间
        let (entry_point, auxv) =
            self.process.with_alive(|a| a.user_space.parse_and_map_elf_file(elf_file));

        // 分配栈
        let stack_id = self.stack_id();
        self.process.with_alive(|a| a.user_space.alloc_stack(stack_id));

        // 将参数, auxv 和环境变量放到栈上
        let (sp, argc, argv, envp) = stack_id.init_stack(args, envp, auxv);

        // 为线程初始化上下文
        let sepc: usize = entry_point.into();
        self.context().init_user(sp, sepc, sstatus::read(), argc, argv, envp);

        // 将线程打包为 Future, 并将打包好的 Future 丢入调度器中
        userloop::spawn(self);
    }
}

// 这里的东西大部分都是不变的, 不用加锁
pub struct ThreadInfoInner {
    // 线程所占据的栈空间的 ID, 不可变, 线程死掉对应的栈就要释放给进程管理器
    stack_id: StackID,
    // 在用户和内核态之间切换时用到的上下文
    uk_conext: Box<UKContext, Global>,
}
