use log::info;

use crate::memory::UserWritePtr;

use super::{Syscall, SyscallResult};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct CpuSet {
    /// cpu set
    pub set: usize,
    /// for padding
    pub dummy: [usize; 15],
}
impl CpuSet {
    /// alloc a cpu set
    /// you should pass the max number of cpus which you want to set
    pub fn new(cpus: usize) -> Self {
        Self {
            set: (1 << cpus - 1),
            dummy: [0; 15],
        }
    }
}

impl<'a> Syscall<'a> {
    pub fn sys_sched_setscheduler(&mut self) -> SyscallResult {
        info!("Syscall: sched_setscheduler");
        Ok(0)
    }

    pub fn sys_sched_getscheduler(&mut self) -> SyscallResult {
        info!("Syscall: sched_getscheduler");
        Ok(0)
    }

    pub fn sys_sched_getparam(&mut self) -> SyscallResult {
        info!("Syscall: sched_getparam");
        Ok(0)
    }

    pub fn sys_sched_setaffinity(&mut self) -> SyscallResult {
        info!("Syscall: sched_setaffinity");
        Ok(0)
    }

    pub fn sys_sched_getaffinity(&mut self) -> SyscallResult {
        info!("Syscall: sched_getaffinity");
        let args = self.cx.syscall_args();
        let mask = UserWritePtr::<CpuSet>::from(args[1]);
        mask.write(&self.lproc, CpuSet::new(1))?;

        Ok(0)
    }
}
