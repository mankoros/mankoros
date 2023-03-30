use core::sync::atomic::AtomicI32;

use alloc::{collections::BTreeMap, format, rc::Weak, string::String, sync::Arc, vec::Vec};

use crate::{here, sync::SpinNoIrqLock};

use super::{
    elf_loader::{map_elf_segment, parse_elf},
    pid_tid::{alloc_pid, alloc_tid, PidHandler, Tid, TidHandler},
    user_space::{StackID, UserSpace},
};

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

    pub fn with_mut_thread<T>(
        &mut self,
        tid: Tid,
        f: impl FnOnce(&mut ThreadInfo) -> T,
    ) -> Option<T> {
        self.with_mut_alive(|alive| alive.threads.get_mut(&tid).map(f)).flatten()
    }

    pub fn new_empty_process() -> Arc<Self> {
        let pid_handler = alloc_pid();
        let pid = pid_handler.pid();
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

    // pub fn exec_with_bytes(self: Arc<Self>, elf_data: &[u8], args: Vec<String>, envp: Vec<String>) {
    //     let elf = parse_elf(elf_data);

    //     // 把 elf 的 segment 映射到用户空间
    //     self.with_mut_alive(|alive| {
    //         map_elf_segment(&elf, &mut alive.user_space.page_table);
    //     });

    //     // 开一个小小的堆
    //     self.with_mut_alive(|alive| {
    //         alive.user_space.alloc_heap(1);
    //     });

    //     // 初始化新的线程
    //     let tid_handler = self.create_empty_thread();

    //     // 将参数, auxv 和环境变量放到栈上
    //     // 为线程初始化上下文
    //     // 将线程打包为 Future
    //     // 将打包好的 Future 丢入调度器中
    // }

    // pub fn create_empty_thread(self: Arc<Self>) -> TidHandler {
    //     // 分配新的 TID
    //     let tid_handler = alloc_tid();
    //     let tid = tid_handler.tid();

    //     // 分配新的栈
    //     let stack_id = self
    //         .with_mut_alive(|alive| alive.user_space.alloc_stack())
    //         .expect(format!("alloc stack failed, pid: {}", self.pid.pid_usize()).as_str());

    //     // 构建初始 Thread 结构体
    //     let thread = ThreadInfo {
    //         tid: tid_handler,
    //         alive: SpinNoIrqLock::new(Some(AliveThreadInfo {
    //             process: self,
    //             stack_id,
    //         })),
    //     };

    //     // 把新的线程加入到进程的线程列表中
    //     self.with_mut_alive(|alive| {
    //         alive.threads.insert(tid, thread);
    //     });

    //     tid_handler
    // }

    // pub fn create_init_thread(&mut self) -> Arc<ThreadInfo> {}
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
    // === 进程地址空间数据 ===
    user_space: UserSpace,
    // TODO: FD Table
}

// ================ 线程 =================

pub struct ThreadInfo {
    tid: TidHandler,
    alive: SpinNoIrqLock<Option<AliveThreadInfo>>,
}

// 这个结构目前有 pre 进程的大锁保护, 内部的信息暂时都不用加锁
pub struct AliveThreadInfo {
    // 线程活着, 进程就不能死
    process: Arc<ProcessInfo>,
    // 线程所占据的栈空间的 ID, 不可变, 线程死掉对应的栈就要释放给进程管理器
    stack_id: StackID,
}

// ================ 地址空间 =================
