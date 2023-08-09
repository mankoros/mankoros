use log::{info, warn};

use crate::{
    executor::util_futures::yield_now,
    memory::{address::VirtAddr, UserReadPtr, UserWritePtr},
    process::lproc_mgr::GlobalLProcManager,
    signal::SignalSet,
    tools::errors::LinuxError,
};

use super::{Syscall, SyscallResult};

#[derive(Debug, Copy, Clone)]
struct SigAction {
    sa_handler: usize,
    sa_flags: u32,
    sa_restorer: usize,
    sa_mask: usize,
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
            log::debug!("sigaction: signum: {:?}, act: {:x?}", signum, act);

            self.lproc.with_mut_signal(|s| {
                s.signal_handler.insert(signum, act.sa_handler.into());
            });
        }
        if args[2] != 0 {
            // Read the current signal action
            let act_ptr = UserWritePtr::<SigAction>::from(args[2]);
            let handler = self
                .lproc
                .with_mut_signal(|s| s.signal_handler.get(&signum).cloned().unwrap_or(0.into()));
            let act = SigAction {
                sa_handler: handler.bits(),
                sa_flags: 0,
                sa_restorer: 0,
                sa_mask: 0,
            };
            act_ptr.write(&self.lproc, act)?;
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

        if signum != 0 {
            proc.send_signal(signum);
        }

        Ok(0)
    }

    pub fn sys_sigreturn(&self) -> SyscallResult {
        info!("Syscall: sigreturn");
        *self.lproc.context() = self
            .lproc
            .with_mut_signal(|s| s.before_signal_context.get_mut().as_ref().clone());

        // Clear processing bit
        self.lproc.with_mut_signal(|s| {
            assert!(!s.signal_processing.is_empty());
            // signum - 1
            let signum_1 = s.signal_processing.bits().trailing_zeros();
            s.signal_processing.remove(SignalSet::from_bits_truncate(1 << signum_1));
        });

        Ok(self.lproc.context().user_rx[10])
    }
}
