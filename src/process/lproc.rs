use super::{
    pid::{alloc_pid, Pid, PidHandler},
    user_space::{StackID, UserSpace},
};
use crate::{
    fs::{
        self,
        vfs::{filesystem::VfsNode, path::Path},
    },
    sync::SpinNoIrqLock,
    tools::handler_pool::UsizePool,
    trap::context::UKContext, consts::PAGE_SIZE, arch::within_sum,
};
use alloc::{
    alloc::Global, boxed::Box, collections::BTreeMap, string::String, sync::Arc, sync::Weak,
    vec::Vec,
};
use core::{cell::SyncUnsafeCell, sync::atomic::AtomicI32};
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
    parent: Option<Weak<LightProcess>>,
    context: SyncUnsafeCell<Box<UKContext, Global>>,
    stack_id: StackID,

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
    // TODO: signal handler
}

impl LightProcess {
    // ========================= 各类 Getter/Setter =========================
    pub fn id(&self) -> Pid {
        self.id.pid()
    }

    pub fn parent_id(&self) -> Pid {
        if let Some(p) = self.parent.as_ref() {
            p.upgrade().unwrap().id()
        } else {
            // Return 1 if no parent
            1.into()
        }
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

    pub fn context(&self) -> &mut UKContext {
        unsafe { &mut *self.context.get() }
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
            parent: None,
            context: SyncUnsafeCell::new(unsafe { UKContext::new_uninit() }),
            stack_id: memory.alloc_stack_id(),
            children: new_shared(Vec::new()),
            status: SpinNoIrqLock::new(SyncUnsafeCell::new(ProcessStatus::UNINIT)),
            exit_code: AtomicI32::new(0),
            group: new_shared(ThreadGroup::new_empty()),
            memory: new_shared(memory),
            fsinfo: new_shared(FsInfo::new()),
            fdtable: new_shared(FdTable::new_with_std()),
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
        let stack_id = self.stack_id;
        self.with_mut_memory(|m| {
            m.areas_mut().insert_stack_at(stack_id);
            m.force_map_range(stack_id.stack_range());
        });

        debug!("Stack alloc done.");
        // 将参数, auxv 和环境变量放到栈上
        let (sp, argc, argv, envp) = within_sum(|| stack_id.init_stack(args, envp, auxv));

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

struct ThreadGroup {
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
