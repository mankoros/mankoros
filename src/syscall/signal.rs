use log::{info, warn};

use crate::{
    executor::util_futures::yield_now,
    signal::SignalSet,
    tools::{errors::LinuxError, user_check::UserCheck},
};

use super::{Syscall, SyscallResult};

impl<'a> Syscall<'a> {
    pub async fn sys_sigwait(&self) -> SyscallResult {
        info!("Syscall: sigwait");
        let args = self.cx.syscall_args();
        let user_check = UserCheck::new_with_sum(&self.lproc);
        let waiting_sigset: SignalSet = user_check.checked_read(args[0] as _)?;

        let mut timeout = 100000;

        while timeout > 0 {
            let sig = self.lproc.signal();
            if sig.intersects(waiting_sigset) {
                let inter = sig.intersection(waiting_sigset).bits();
                let first_bit_set = inter & (1 << inter.trailing_zeros());
                return Ok(first_bit_set as usize);
            }
            timeout -= 1;
            yield_now().await;
        }
        warn!("sigwait: timeout");
        Err(LinuxError::EDEADLK)
    }
}
