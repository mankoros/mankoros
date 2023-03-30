use core::sync::atomic::AtomicI32;

use alloc::{
    alloc::Global, boxed::Box, collections::BTreeMap, format, string::String, sync::Arc,
    sync::Weak, vec::Vec,
};
use riscv::register::sstatus;

use crate::{here, sync::SpinNoIrqLock};

use super::{
    aux_vector::AuxVector,
    context::UKContext,
    elf_loader::{get_entry_point, map_elf_segment, parse_elf},
    pid_tid::{alloc_pid, alloc_tid, PidHandler, Tid, TidHandler},
    user_space::{StackID, UserSpace},
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

    pub fn new_empty_process() -> Arc<Self> {
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

    // TODO: exec 应该是 "对已有 thread 进行替换的", 这里更加偏向于 "创建一个新的 thread + 加载程序的混合体", 找时间要拆开
    pub fn init_thread(self: Arc<Self>, elf_data: &[u8], args: Vec<String>, envp: Vec<String>) {
        let elf = parse_elf(elf_data);

        // 把 elf 的 segment 映射到用户空间
        let begin_addr = self
            .with_alive(|alive| map_elf_segment(&elf, &mut alive.user_space.page_table))
            .expect("map elf failed");

        // 开一个小小的堆
        self.with_alive(|alive| {
            alive.user_space.alloc_heap(1);
        });

        // 创一个新的空线程
        let thread = self.create_empty_thread();

        // 将参数, auxv 和环境变量放到栈上
        let auxv = AuxVector::from_elf(&elf, begin_addr);
        let stack_id = thread.with_alive(|alive| alive.stack_id);
        let (sp, argc, argv, envp) = stack_id.init_stack(args, envp, auxv);

        // 为线程初始化上下文
        let sepc = get_entry_point(&elf).0;
        thread.with_alive(|alive| {
            alive.uk_conext.init_user(sp, sepc, sstatus::read(), argc, argv, envp)
        });

        // TODO: 思考什么时候切页表
        // 将线程打包为 Future

        // 将打包好的 Future 丢入调度器中
        // userloop::spawn(future);
    }

    pub fn create_empty_thread(self: &Arc<Self>) -> Arc<ThreadInfo> {
        // 分配新的 TID
        let tid_handler = alloc_tid();
        let tid = tid_handler.tid();

        // 分配新的栈
        let stack_id = self.with_alive(|alive| alive.user_space.alloc_stack());

        // 构建初始 Thread 结构体
        let thread = Arc::new(ThreadInfo {
            tid: tid_handler,
            alive: SpinNoIrqLock::new(Some(AliveThreadInfo {
                process: self.clone(),
                stack_id,
                uk_conext: unsafe { UKContext::new_uninit() },
            })),
        });

        // 把新的线程加入到进程的线程列表中
        self.with_alive(|alive| {
            alive.threads.insert(tid, Arc::downgrade(&thread));
        });

        thread
    }
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
    tid: TidHandler,
    alive: SpinNoIrqLock<Option<AliveThreadInfo>>,
}

impl ThreadInfo {
    pub fn with_alive<T>(&self, f: impl FnOnce(&mut AliveThreadInfo) -> T) -> T {
        self.with_alive_or_dead(f).expect(
            format!(
                "thread {} is dead when trying to access alive",
                self.tid.tid_usize()
            )
            .as_str(),
        )
    }

    pub fn with_alive_or_dead<T>(&self, f: impl FnOnce(&mut AliveThreadInfo) -> T) -> Option<T> {
        self.alive.lock(here!()).as_mut().map(f)
    }
}

// 这个结构目前有 pre 进程的大锁保护, 内部的信息暂时都不用加锁
pub struct AliveThreadInfo {
    // 线程活着, 进程就不能死
    process: Arc<ProcessInfo>,
    // 线程所占据的栈空间的 ID, 不可变, 线程死掉对应的栈就要释放给进程管理器
    stack_id: StackID,
    // 在用户和内核态之间切换时用到的上下文
    uk_conext: Box<UKContext, Global>,
}

// ================ 地址空间 =================
