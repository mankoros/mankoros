use super::{
    pid::{alloc_pid, Pid, PidHandler},
    user_space::{UserSpace},
};
use crate::{
    arch::within_sum,
    consts::PAGE_SIZE,
    fs::{
        self,
        vfs::{filesystem::VfsNode, path::Path},
    },
    signal,
    sync::SpinNoIrqLock,
    syscall,
    tools::handler_pool::UsizePool,
    trap::context::UKContext, process::user_space::{THREAD_STACK_SIZE, init_stack}, memory::address::VirtAddr,
};
use alloc::{
    alloc::Global, boxed::Box, collections::BTreeMap, string::String, sync::Arc, sync::Weak,
    vec::Vec,
};
use core::{
    cell::SyncUnsafeCell,
    sync::atomic::{AtomicI32, Ordering},
};
use log::debug;
use riscv::register::sstatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessStatus {
    UNINIT,
    READY,
    RUNNING,
    STOPPED,
    ZOMBIE,
}

// 少打两个字?
type Shared<T> = Arc<SpinNoIrqLock<T>>;
fn new_shared<T>(t: T) -> Shared<T> {
    Arc::new(SpinNoIrqLock::new(t))
}

pub struct LightProcess {
    id: PidHandler,
    parent: Shared<Option<Weak<LightProcess>>>,
    context: SyncUnsafeCell<Box<UKContext, Global>>,
    stack_begin: VirtAddr,

    // 因为每个儿子自己跑来加 parent 的 children, 所以可能并发, 要加锁
    children: Arc<SpinNoIrqLock<Vec<Arc<LightProcess>>>>,
    // 因为同一个 Thread Group 里的进程可能会互相修改状态, 所以要加锁
    status: SpinNoIrqLock<SyncUnsafeCell<ProcessStatus>>,
    exit_code: AtomicI32,

    // 下面的数据可能被多个 LightProcess 共享
    group: Shared<ThreadGroup>,
    memory: Shared<UserSpace>,
    fsinfo: Shared<FsInfo>,
    fdtable: Shared<FdTable>,
    // TODO: use a signal manager
    signal: SpinNoIrqLock<signal::SignalSet>,
}

impl LightProcess {
    // ========================= 各类 Getter/Setter =========================
    pub fn id(&self) -> Pid {
        self.id.pid()
    }

    pub fn parent_id(&self) -> Pid {
        if let Some(p) = self.parent.lock(here!()).as_ref() {
            p.upgrade().unwrap().id()
        } else {
            // Return 1 if no parent
            1.into()
        }
    }

    pub fn parent(&self) -> Option<Weak<LightProcess>> {
        self.parent.lock(here!()).clone()
    }

    pub fn signal(&self) -> signal::SignalSet {
        self.signal.lock(here!()).clone()
    }

    pub fn tgid(&self) -> Pid {
        self.group.lock(here!()).tgid()
    }

    pub fn status(&self) -> ProcessStatus {
        unsafe { *self.status.lock(here!()).get() }
    }

    pub fn set_status(&self, status: ProcessStatus) {
        unsafe {
            *self.status.lock(here!()).get() = status;
        }
    }

    pub fn set_signal(self: Arc<Self>, signal: signal::SignalSet) {
        self.signal.lock(here!()).set(signal, true);
    }
    pub fn clear_signal(self: Arc<Self>, signal: signal::SignalSet) {
        self.signal.lock(here!()).set(signal, false);
    }

    pub fn context(&self) -> &mut UKContext {
        unsafe { &mut *self.context.get() }
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::SeqCst)
    }

    pub fn children(&self) -> Vec<Arc<LightProcess>> {
        self.children.lock(here!()).clone()
    }

    pub fn add_child(self: Arc<Self>, child: Arc<LightProcess>) {
        self.children.lock(here!()).push(child);
    }

    pub fn remove_child(self: Arc<Self>, child: &Arc<LightProcess>) {
        let mut children = self.children.lock(here!());
        let index = children.iter().position(|c| Arc::ptr_eq(c, child)).unwrap();
        children.remove(index);
    }
    pub fn do_exit(self: Arc<Self>) {
        if let Some(parent) = self.parent() {
            let parent = parent.upgrade().unwrap().clone();
            // No remove from parent here, because it will be done in the parent's wait
            // Just send a signal to the parent
            parent.set_signal(signal::SignalSet::SIGCHLD);
            // Set self status
            self.set_status(ProcessStatus::STOPPED);
        }
        // Set children's parent to None
        let children = self.children.lock(here!());
        children.iter().for_each(|c| *c.parent.lock(here!()) = self.parent().clone());
    }

    pub fn with_group<T>(&self, f: impl FnOnce(&ThreadGroup) -> T) -> T {
        f(&self.group.lock(here!()))
    }

    pub fn with_mut_group<T>(&self, f: impl FnOnce(&mut ThreadGroup) -> T) -> T {
        f(&mut self.group.lock(here!()))
    }

    pub fn with_memory<T>(&self, f: impl FnOnce(&UserSpace) -> T) -> T {
        f(&self.memory.lock(here!()))
    }

    pub fn with_mut_memory<T>(&self, f: impl FnOnce(&mut UserSpace) -> T) -> T {
        f(&mut self.memory.lock(here!()))
    }

    pub fn with_fsinfo<T>(&self, f: impl FnOnce(&FsInfo) -> T) -> T {
        f(&self.fsinfo.lock(here!()))
    }

    pub fn with_mut_fsinfo<T>(&self, f: impl FnOnce(&mut FsInfo) -> T) -> T {
        f(&mut self.fsinfo.lock(here!()))
    }

    pub fn with_fdtable<T>(&self, f: impl FnOnce(&FdTable) -> T) -> T {
        f(&self.fdtable.lock(here!()))
    }

    pub fn with_mut_fdtable<T>(&self, f: impl FnOnce(&mut FdTable) -> T) -> T {
        f(&mut self.fdtable.lock(here!()))
    }

    pub fn is_exit(&self) -> bool {
        self.status() == ProcessStatus::ZOMBIE
    }

    // ========================= 进程创建 =========================
    pub fn new() -> Arc<Self> {
        let mut memory = UserSpace::new();
        Arc::new(Self {
            id: alloc_pid(),
            parent: new_shared(None),
            context: SyncUnsafeCell::new(unsafe { UKContext::new_uninit() }),
            stack_begin: memory.areas_mut().alloc_stack(THREAD_STACK_SIZE),
            children: new_shared(Vec::new()),
            status: SpinNoIrqLock::new(SyncUnsafeCell::new(ProcessStatus::UNINIT)),
            exit_code: AtomicI32::new(0),
            group: new_shared(ThreadGroup::new_empty()),
            memory: new_shared(memory),
            fsinfo: new_shared(FsInfo::new()),
            fdtable: new_shared(FdTable::new_with_std()),
            signal: SpinNoIrqLock::new(signal::SignalSet::empty()),
        })
    }

    /// 第一次 exec
    // Big-TODO: 考虑 remap, 这里默认进程之前没有 map 过文件
    pub fn exec_first(
        self: Arc<Self>,
        elf_file: Arc<dyn VfsNode>,
        args: Vec<String>,
        envp: Vec<String>,
    ) {
        debug!("Exec first");
        // 把 elf 的 segment 映射到用户空间
        let (entry_point, auxv) = self.with_mut_memory(|m| m.parse_and_map_elf_file(elf_file));

        debug!("Parse ELF file done.");

        // 分配栈
        self.with_mut_memory(|m| m.force_map_area(self.stack_begin));

        debug!("Stack alloc done.");
        // 将参数, auxv 和环境变量放到栈上
        let (sp, argc, argv, envp) = within_sum(
            || init_stack(self.stack_begin, args, envp, auxv));

        // 为线程初始化上下文
        debug!("Entry point: {:?}", entry_point);
        let sepc: usize = entry_point.into();
        self.context().init_user(sp, sepc, sstatus::read(), argc, argv, envp);

        // 分配堆
        // TODO: 改成彻底的 lazy alloc
        self.with_mut_memory(|m| m.areas_mut().insert_heap(PAGE_SIZE));
        debug!("Heap alloc done.");

        // 设置状态为 READY
        self.set_status(ProcessStatus::READY);
        debug!("User init done.");
    }

    pub fn do_clone(self: Arc<Self>, flags: syscall::CloneFlags, user_stack_begin: Option<VirtAddr>) -> Arc<Self> {
        use syscall::CloneFlags;

        let id = alloc_pid();
        let context = SyncUnsafeCell::new(Box::new(self.context().clone()));
        let status = SpinNoIrqLock::new(SyncUnsafeCell::new(self.status()));
        let exit_code = AtomicI32::new(self.exit_code());

        let parent;
        let children;
        let group;

        if flags.contains(CloneFlags::THREAD) {
            parent = self.parent.clone();
            children = self.children.clone();
            // remember to add the new lproc to group please!
            group = self.group.clone();
        } else {
            parent = new_shared(Some(Arc::downgrade(&self)));
            children = new_shared(Vec::new());
            group = new_shared(ThreadGroup::new_empty());
        }

        let memory;
        if flags.contains(CloneFlags::VM) {
            memory = self.memory.clone();
        } else {
            // TODO-PERF: 这里应该可以优化
            // 比如引入一个新的状态, 表示这个进程的内存是应该 CoW 的, 但是不真正去 CoW 本来的内存
            // 只是给它一个全是 Invaild 的页表, 然后如果它没有进行任何写入操作, 直接进入 syscall exec 的话,
            // 就可以直接来一个新的地址空间, 不用连累旧的进程的地址空间也来一次 CoW.
            // 反之如果在那种状态 page fault 了, 那么我们就要进行 "昂贵" 的 CoW 操作了
            let _raw_memory = self.with_mut_memory(|m| m.clone_cow());
            memory = new_shared(self.with_mut_memory(|m| m.clone_cow()));
        }

        let stack_begin;
        // 如果用户指定了栈, 那么就用用户指定的栈, 否则在新的地址空间里分配一个
        if let Some(sp) = user_stack_begin {
            stack_begin = sp; 
        } else {
            stack_begin = memory.lock(here!())
                .areas_mut().alloc_stack(THREAD_STACK_SIZE);
        }

        let fsinfo;
        if flags.contains(CloneFlags::FS) {
            fsinfo = self.fsinfo.clone();
        } else {
            fsinfo = new_shared(FsInfo::new());
        }

        let fdtable;
        if flags.contains(CloneFlags::FILES) {
            fdtable = self.fdtable.clone();
        } else {
            fdtable = new_shared(FdTable::new_with_std());
        }

        // TODO: signal handler

        let new = Self {
            id,
            parent,
            context,
            stack_begin,
            children,
            status,
            exit_code,
            group,
            memory,
            fsinfo,
            fdtable,
            signal: SpinNoIrqLock::new(signal::SignalSet::empty()),
        };
        let new = Arc::new(new);

        if flags.contains(CloneFlags::THREAD) {
            new.with_mut_group(|g| g.push(new.clone()));
        } else {
            new.with_mut_group(|g| g.push_leader(new.clone()));
            self.add_child(new.clone());
        }

        new
    }
}

pub struct FsInfo {
    pub cwd: Path,
}

impl FsInfo {
    pub fn new() -> Self {
        Self {
            cwd: Path::from_str("/").unwrap(),
        }
    }
}

pub struct FileDescriptor {
    pub file: Arc<dyn VfsNode>,
}

impl FileDescriptor {
    pub fn new(file: Arc<dyn VfsNode>) -> Arc<Self> {
        Arc::new(Self { file })
    }
}

pub struct FdTable {
    pool: UsizePool,
    table: BTreeMap<usize, Arc<FileDescriptor>>,
}

impl FdTable {
    pub fn new_empty() -> Self {
        Self {
            pool: UsizePool::new(0),
            table: BTreeMap::new(),
        }
    }

    pub fn new_with_std() -> Self {
        let mut t = Self::new_empty();
        debug_assert_eq!(t.alloc(Arc::new(fs::stdio::Stdin)), 0);
        debug_assert_eq!(t.alloc(Arc::new(fs::stdio::Stdout)), 1);
        debug_assert_eq!(t.alloc(Arc::new(fs::stdio::Stderr)), 2);
        t
    }

    // alloc finds a fd and insert the file descriptor into the table
    pub fn alloc(&mut self, file: Arc<dyn VfsNode>) -> usize {
        let fd = self.pool.get();
        self.table.insert(fd, FileDescriptor::new(file));
        fd
    }
    // insert inserts the file descriptor into the table using specified fd
    pub fn insert(&mut self, fd: usize, file: Arc<dyn VfsNode>) {
        self.table.insert(fd, FileDescriptor::new(file));
    }

    pub fn remove(&mut self, fd: usize) -> Option<Arc<FileDescriptor>> {
        self.pool.release(fd);
        self.table.remove(&fd)
    }

    pub fn get(&self, fd: usize) -> Option<Arc<FileDescriptor>> {
        self.table.get(&fd).map(|f| f.clone())
    }
}

pub struct ThreadGroup {
    members: BTreeMap<Pid, Arc<LightProcess>>,
    leader: Option<Weak<LightProcess>>,
}

impl ThreadGroup {
    pub fn new_empty() -> Self {
        Self {
            members: BTreeMap::new(),
            leader: None,
        }
    }

    pub fn push_leader(&mut self, leader: Arc<LightProcess>) {
        debug_assert!(self.leader.is_none());
        debug_assert!(self.members.is_empty());

        self.leader = Some(Arc::downgrade(&leader));
        self.members.insert(leader.id(), leader);
    }

    pub fn is_leader(&self, lproc: &LightProcess) -> bool {
        self.tgid() == lproc.id()
    }

    pub fn push(&mut self, lproc: Arc<LightProcess>) {
        debug_assert!(self.leader.is_some());
        self.members.insert(lproc.id(), lproc);
    }

    pub fn remove(&mut self, thread: &LightProcess) {
        debug_assert!(self.leader.is_some());
        self.members.remove(&thread.id());
    }

    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    pub fn tgid(&self) -> Pid {
        self.leader.as_ref().unwrap().upgrade().unwrap().id.pid()
    }
}
