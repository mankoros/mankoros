use super::{
    pid::{alloc_pid, Pid, PidHandler},
    user_space::UserSpace,
};
use crate::{
    arch::{self, switch_page_table, within_sum},
    consts::PAGE_SIZE,
    fs::{
        self,
        new_vfs::{path::Path, top::VfsFileRef},
    },
    memory::address::VirtAddr,
    process::user_space::{init_stack, THREAD_STACK_SIZE},
    signal,
    sync::SpinNoIrqLock,
    syscall,
    timer::TimeStat,
    tools::handler_pool::UsizePool,
    trap::context::UKContext,
};
use alloc::{
    alloc::Global, boxed::Box, collections::BTreeMap, string::String, sync::Arc, sync::Weak,
    vec::Vec,
};
use core::{
    cell::SyncUnsafeCell,
    sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering},
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

// 少打两个字？
type Shared<T> = Arc<SpinNoIrqLock<T>>;
fn new_shared<T>(t: T) -> Shared<T> {
    Arc::new(SpinNoIrqLock::new(t))
}

pub struct PrivateInfo {
    // https://man7.org/linux/man-pages/man2/set_tid_address.2.html
    // Optional address when entering a new thread or exiting a thread.
    // When set, when spawning a new thread, the kernel sets the thread's tid to this address.
    pub set_child_tid: Option<usize>,
    // When set, when the thread exits, the kernel sets the thread's tid to this address, and wake up a futex waiting on this address.
    pub clear_child_tid: Option<usize>,
}

impl PrivateInfo {
    fn new() -> Self {
        Self {
            set_child_tid: None,
            clear_child_tid: None,
        }
    }
}

pub struct LightProcess {
    id: PidHandler,
    parent: Shared<Option<Weak<LightProcess>>>,
    context: SyncUnsafeCell<Box<UKContext, Global>>,

    // 因为每个儿子自己跑来加 parent 的 children, 所以可能并发，要加锁
    children: Arc<SpinNoIrqLock<Vec<Arc<LightProcess>>>>,
    // 因为同一个 Thread Group 里的进程可能会互相修改状态，所以要加锁
    status: SpinNoIrqLock<SyncUnsafeCell<ProcessStatus>>,
    timer: SpinNoIrqLock<TimeStat>,
    exit_code: AtomicI32,

    // Per thread information
    private_info: SpinNoIrqLock<PrivateInfo>,

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
        *self.signal.lock(here!())
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

    pub fn timer(&self) -> &SpinNoIrqLock<TimeStat> {
        &self.timer
    }

    pub fn set_signal(self: &Arc<Self>, signal: signal::SignalSet) {
        self.signal.lock(here!()).set(signal, true);
    }
    pub fn clear_signal(self: &Arc<Self>, signal: signal::SignalSet) {
        self.signal.lock(here!()).set(signal, false);
    }

    pub fn context(&self) -> &mut UKContext {
        unsafe { &mut *self.context.get() }
    }

    pub fn exit_code(&self) -> i32 {
        self.exit_code.load(Ordering::SeqCst)
    }

    pub fn set_exit_code(&self, code: i32) {
        self.exit_code.store(code, Ordering::SeqCst);
    }

    pub fn children(&self) -> Vec<Arc<LightProcess>> {
        self.children.lock(here!()).clone()
    }
    /// mostly for debug
    pub fn children_pid_usize(&self) -> Vec<usize> {
        self.children().iter().map(|c| c.id().into()).collect::<Vec<usize>>()
    }

    pub fn add_child(self: &Arc<Self>, child: Arc<LightProcess>) {
        self.children.lock(here!()).push(child);
    }

    pub fn remove_child(self: &Arc<Self>, child: &Arc<LightProcess>) {
        let mut children = self.children.lock(here!());
        let index = children.iter().position(|c| Arc::ptr_eq(c, child)).unwrap();
        children.remove(index);
    }
    pub fn do_exit(self: &Arc<Self>) {
        if let Some(parent) = self.parent() {
            let parent = parent.upgrade().unwrap();
            // No remove from parent here, because it will be done in the parent's wait
            // Just send a signal to the parent
            parent.set_signal(signal::SignalSet::SIGCHLD);
            // Set self status
            self.set_status(ProcessStatus::STOPPED);
        }
        // Set children's parent to self's parent
        let children = self.children.lock(here!());
        children.iter().for_each(|c| *c.parent.lock(here!()) = self.parent());
        drop(children);

        log::debug!(
            "do_exit: left children {:?} to parent {:?}",
            self.children_pid_usize(),
            self.parent()
        );
        // release all fd
        self.with_mut_fdtable(|f| f.release_all());
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

    pub fn with_private_info<T>(&self, f: impl FnOnce(&PrivateInfo) -> T) -> T {
        f(&self.private_info.lock(here!()))
    }

    pub fn with_mut_private_info<T>(&self, f: impl FnOnce(&mut PrivateInfo) -> T) -> T {
        f(&mut self.private_info.lock(here!()))
    }

    pub fn is_exit(&self) -> bool {
        self.status() == ProcessStatus::ZOMBIE
    }

    // ========================= 进程创建 =========================
    pub fn new() -> Arc<Self> {
        let new = Arc::new(Self {
            id: alloc_pid(),
            parent: new_shared(None),
            context: SyncUnsafeCell::new(unsafe { UKContext::new_uninit() }),
            children: new_shared(Vec::new()),
            status: SpinNoIrqLock::new(SyncUnsafeCell::new(ProcessStatus::UNINIT)),
            timer: SpinNoIrqLock::new(TimeStat::new()),
            exit_code: AtomicI32::new(0),
            group: new_shared(ThreadGroup::new_empty()),
            memory: new_shared(UserSpace::new()),
            fsinfo: new_shared(FsInfo::new()),
            fdtable: new_shared(FdTable::new_with_std()),
            signal: SpinNoIrqLock::new(signal::SignalSet::empty()),
            private_info: SpinNoIrqLock::new(PrivateInfo::new()),
        });
        // I am the group leader
        new.group.lock(here!()).push_leader(new.clone());
        new
    }

    pub fn do_exec(self: Arc<Self>, elf_file: VfsFileRef, args: Vec<String>, envp: Vec<String>) {
        // Create new userspace
        let new_userspace = UserSpace::new();

        let page_table_paddr = new_userspace.page_table.root_paddr();
        debug!(
            "Create new userspace with page table at {:?}",
            page_table_paddr
        );
        // Switch to new userspace immediately
        switch_page_table(page_table_paddr.bits());

        // Drop old userspace
        *self.memory.lock(here!()) = new_userspace;

        // 把 elf 的 segment 映射到用户空间
        let (entry_point, auxv) = self.with_mut_memory(|m| m.parse_and_map_elf_file(elf_file));

        debug!("Parse ELF file done.");

        // 分配栈
        let stack_begin = self.with_mut_memory(|m| {
            let stack_begin = m.areas_mut().alloc_stack(THREAD_STACK_SIZE);
            // Force map since kernel need to init it
            m.force_map_area(stack_begin);
            stack_begin
        });

        debug!("Stack alloc done.");
        // 将参数，auxv 和环境变量放到栈上
        let (sp, argc, argv, envp) = within_sum(|| init_stack(stack_begin, args, envp, auxv));

        // 为线程初始化上下文
        debug!("Entry point: {:?}", entry_point);
        let sepc = entry_point.bits();
        self.context().init_user(sp, sepc, sstatus::read(), argc, argv, envp);

        // 分配堆
        // TODO: 改成彻底的 lazy alloc
        self.with_mut_memory(|m| m.areas_mut().insert_heap(PAGE_SIZE));
        debug!("Heap alloc done.");

        // update the fd table
        self.with_mut_fdtable(|t| t.close_on_exec());

        // 设置状态为 READY
        self.set_status(ProcessStatus::READY);
        debug!("User init done.");
    }

    pub fn do_clone(
        self: Arc<Self>,
        flags: syscall::CloneFlags,
        user_stack_begin: Option<VirtAddr>,
    ) -> Arc<Self> {
        use syscall::CloneFlags;

        let id = alloc_pid();
        let mut context = SyncUnsafeCell::new(Box::new(self.context().clone()));
        let status = SpinNoIrqLock::new(SyncUnsafeCell::new(self.status()));
        let timer = SpinNoIrqLock::new(TimeStat::new());
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
            // Share memory
            memory = self.memory.clone();
        } else {
            // 这里应该可以优化
            // Noop, 这里不能优化，如果延迟cow，其他线程如果对vm做了修改，不能保证符合clone的语意
            memory = new_shared(self.with_mut_memory(|m| m.clone_cow()));
            unsafe { riscv::asm::sfence_vma_all() }; // Flush both old and new process
                                                     // TODO: avoid flushing global entries like kernel mappings
        }
        let old_memory = self.memory.lock(here!());
        let mut new_memory = memory.lock(here!());

        let new_stack_top;
        let new_sp;
        let old_sp = self.context().get_user_sp();
        let (old_stack_range, _) = old_memory.areas().get(old_sp.into()).unwrap();
        let old_stack_top: usize = (old_stack_range.end - 1).bits() & !0xF;
        // 如果用户指定了栈，那么就用用户指定的栈，否则在新的地址空间里分配一个
        if let Some(sp) = user_stack_begin {
            new_stack_top = sp;
            new_sp = new_stack_top - (old_stack_top - old_sp);
        } else if flags.contains(CloneFlags::VM) {
            new_stack_top = new_memory.areas_mut().alloc_stack(THREAD_STACK_SIZE);
            new_memory.force_map_area(new_stack_top);
            // We should in old pagetable now
            debug_assert!(
                arch::get_curr_page_table_addr() == old_memory.page_table.root_paddr().bits()
            );

            let stack_length = old_stack_top - old_sp;
            new_sp = new_stack_top - stack_length;
            // Copy old stack to new stack
            // [old_sp, old_stack_top] => [new_sp, new_stack_top]
            within_sum(|| {
                let new_stack = unsafe {
                    core::slice::from_raw_parts_mut(new_sp.bits() as *mut u8, stack_length)
                };
                let old_stack = unsafe { core::slice::from_raw_parts(old_sp as _, stack_length) };
                new_stack.copy_from_slice(old_stack);
            });
        } else {
            // CoW memory
            new_stack_top = old_stack_top.into();
            new_sp = old_sp.into();
        }
        debug!(
            "old stack top: 0x{:x}, old sp: 0x{:x}",
            old_stack_top, old_sp
        );
        debug!(
            "new stack top: 0x{:x}, new sp: 0x{:x}",
            new_stack_top, new_sp
        );

        context.get_mut().set_user_sp(new_sp.bits());
        drop(old_memory);
        drop(new_memory);

        let fsinfo;
        if flags.contains(CloneFlags::FS) {
            fsinfo = self.fsinfo.clone();
        } else {
            fsinfo = new_shared(FsInfo::new());
        }

        let fdtable;
        if flags.contains(CloneFlags::FILES) {
            // Increase refcnt
            fdtable = self.fdtable.clone();
        } else {
            // Copy the whole fdtable
            fdtable = new_shared(self.fdtable.lock(here!()).clone());
        }

        // TODO: signal handler

        let new = Self {
            id,
            parent,
            context,
            children,
            status,
            timer,
            exit_code,
            group,
            memory,
            fsinfo,
            fdtable,
            signal: SpinNoIrqLock::new(signal::SignalSet::empty()),
            private_info: SpinNoIrqLock::new(PrivateInfo::new()), // TODO: verify if new or need to check FLAG
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
    pub file: VfsFileRef,
    pub get_dents_progress: AtomicUsize, // indicates how many dentries we have read so far
    /// 当前文件标识符的偏移量记录
    pub curr: AtomicUsize,
    pub close_at_exec: AtomicBool,
}

impl FileDescriptor {
    pub fn new(file: VfsFileRef) -> Arc<Self> {
        Arc::new(Self {
            file,
            get_dents_progress: AtomicUsize::new(0),
            curr: AtomicUsize::new(0),
            close_at_exec: AtomicBool::new(false),
        })
    }

    pub fn set_close_on_exec(&self, val: bool) {
        self.close_at_exec.store(val, Ordering::SeqCst);
    }
    pub fn is_close_on_exec(&self) -> bool {
        self.close_at_exec.load(Ordering::SeqCst)
    }

    pub fn get_dents_progress(&self) -> usize {
        self.get_dents_progress.load(Ordering::SeqCst)
    }
    pub fn clear_dents_progress(&self) {
        self.set_dents_progress(0);
    }
    pub fn set_dents_progress(&self, offset: usize) {
        self.get_dents_progress.store(offset, Ordering::SeqCst);
    }

    pub fn curr(&self) -> usize {
        self.curr.load(Ordering::SeqCst)
    }
    pub fn add_curr(&self, offset: usize) {
        self.curr.fetch_add(offset, Ordering::SeqCst);
    }
    pub fn set_curr(&self, offset: usize) {
        self.curr.store(offset, Ordering::SeqCst);
    }
}

impl Clone for FileDescriptor {
    fn clone(&self) -> Self {
        Self {
            file: self.file.clone(),
            get_dents_progress: AtomicUsize::new(self.get_dents_progress()),
            curr: AtomicUsize::new(self.curr()),
            close_at_exec: AtomicBool::new(self.is_close_on_exec()),
        }
    }
}

#[derive(Clone)]
pub struct FdTable {
    pool: UsizePool,
    table: BTreeMap<usize, Arc<FileDescriptor>>,
}

pub enum NewFdRequirement {
    Exactly(usize),
    GreaterThan(usize),
    None,
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
        let fd = t.alloc(VfsFileRef::new(fs::stdio::Stdin));
        debug_assert_eq!(fd, 0);
        let fd = t.alloc(VfsFileRef::new(fs::stdio::Stdout));
        debug_assert_eq!(fd, 1);
        let fd = t.alloc(VfsFileRef::new(fs::stdio::Stderr));
        debug_assert_eq!(fd, 2);
        t
    }

    // alloc finds a fd and insert the file descriptor into the table
    pub fn alloc(&mut self, file: VfsFileRef) -> usize {
        let fd = self.pool.get();
        self.table.insert(fd, FileDescriptor::new(file));
        fd
    }
    pub fn dup(&mut self, new_fd_req: NewFdRequirement, old_fd: &Arc<FileDescriptor>) -> usize {
        let fd_no = match new_fd_req {
            NewFdRequirement::Exactly(new_fd) => new_fd,
            NewFdRequirement::GreaterThan(lower_bound) => {
                let mut skipped_fds = Vec::new();
                let new_fd = loop {
                    let fd = self.pool.get();
                    if fd >= lower_bound {
                        break fd;
                    } else {
                        skipped_fds.push(fd);
                    }
                };
                skipped_fds.into_iter().for_each(|fd| self.pool.release(fd));
                new_fd
            }
            NewFdRequirement::None => self.pool.get(),
        };
        let new_fd = Arc::new((**old_fd).clone());
        self.table.insert(fd_no, new_fd);
        fd_no
    }

    pub fn remove(&mut self, fd: usize) -> Option<Arc<FileDescriptor>> {
        self.pool.release(fd);
        self.table.remove(&fd)
    }

    pub fn get(&self, fd: usize) -> Option<Arc<FileDescriptor>> {
        self.table.get(&fd).cloned()
    }

    pub fn close_on_exec(&mut self) {
        self.table.retain(|no, fd| {
            if fd.is_close_on_exec() {
                self.pool.release(*no);
                false
            } else {
                true
            }
        });
    }
    pub fn release_all(&mut self) {
        self.table.retain(|no, _| {
            self.pool.release(*no);
            false
        })
    }
}

impl Drop for FdTable {
    fn drop(&mut self) {
        self.table.clear();
        log::debug!("FdTable dropped");
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
