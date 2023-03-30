use crate::{here, sync::SpinNoIrqLock, tools::handler_pool::UsizePool};

static PID_USIZE_POOL: SpinNoIrqLock<UsizePool> = SpinNoIrqLock::new(UsizePool::new());
static TID_USIZE_POOL: SpinNoIrqLock<UsizePool> = SpinNoIrqLock::new(UsizePool::new());

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

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Tid(usize);

pub struct TidHandler(Tid);

impl TidHandler {
    pub fn tid(&self) -> Tid {
        self.0
    }

    pub fn tid_usize(&self) -> usize {
        self.tid().0
    }
}

impl Drop for TidHandler {
    fn drop(&mut self) {
        TID_USIZE_POOL.lock(here!()).release(self.tid_usize());
    }
}

pub fn alloc_tid() -> TidHandler {
    let tid_usize = TID_USIZE_POOL.lock(here!()).get();
    TidHandler(Tid(tid_usize))
}
