use crate::{here, sync::SpinNoIrqLock, tools::handler_pool::UsizePool};

static PID_USIZE_POOL: SpinNoIrqLock<UsizePool> = SpinNoIrqLock::new(UsizePool::new());

#[derive(Debug, Copy, Clone, Ord, PartialOrd, Eq, PartialEq)]
pub struct Pid(usize);

impl PartialEq<usize> for Pid {
    fn eq(&self, other: &usize) -> bool {
        self.0 == *other
    }
}
impl From<usize> for Pid {
    fn from(pid: usize) -> Self {
        Pid(pid)
    }
}

impl From<Pid> for usize {
    fn from(value: Pid) -> Self {
        value.0
    }
}

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
