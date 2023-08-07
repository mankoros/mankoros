use log::{info, warn};

use crate::{
    executor::util_futures::yield_now, memory::UserReadPtr, process::lproc_mgr::GlobalLProcManager,
    signal::SignalSet, tools::errors::LinuxError,
};

use super::{Syscall, SyscallResult};

#[derive(Debug, Copy, Clone)]
struct SigAction {
    sa_handler: usize,
    sa_sigaction: usize,
    sa_mask: usize,
    sa_flags: usize,
    sa_restorer: usize,
}

impl<'a> Syscall<'a> {
    pub async fn sys_sigwait(&self) -> SyscallResult {
        info!("Syscall: sigwait");
        let args = self.cx.syscall_args();
        let waiting_sigset = UserReadPtr::<SignalSet>::from(args[0]).read(&self.lproc)?;

        let mut timeout = 100000;

        while timeout > 0 {
            let sig = self.lproc.signal_pending();
            if sig.intersects(waiting_sigset) {
                let inter = sig.intersection(waiting_sigset).bits();
                let first_bit_set = inter & (1 << inter.trailing_zeros());
                return Ok(first_bit_set as usize);
            }
            timeout -= 1;
            yield_now().await;
        }
        warn!("sigwait: timeout");
        Err(LinuxError::EAGAIN)
    }

    pub fn sys_sigaction(&mut self) -> SyscallResult {
        info!("Syscall: sigaction");
        let args = self.cx.syscall_args();
        let signum = args[0] as usize;

        if args[1] != 0 {
            // Install a signal action
            let act = UserReadPtr::<SigAction>::from(args[1]).read(&self.lproc)?;
            log::debug!("sigaction: signum: {}, act: {:?}", signum, act);

            self.lproc.with_mut_signal(|s| {
                s.signal_handler.insert(signum, act.sa_handler.into());
            });
        }
        if args[2] != 0 {
            // Read the current signal action
            log::warn!("todo: read old sigaction");
        }

        Ok(0)
    }

    pub fn sys_kill(&self) -> SyscallResult {
        info!("Syscall: kill");
        let args = self.cx.syscall_args();
        let pid = args[0] as usize;
        let signum = args[1] as usize;
        log::debug!("kill: pid: {}, signum: {}", pid, signum);

        let proc = GlobalLProcManager::get(pid.into()).ok_or(LinuxError::ESRCH)?;

        proc.send_signal(signum);

        Ok(0)
    }
}
