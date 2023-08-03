use log::{info, warn};

use crate::{
    executor::util_futures::yield_now, memory::UserReadPtr, signal::SignalSet,
    tools::errors::LinuxError,
};

use super::{Syscall, SyscallResult};

impl<'a> Syscall<'a> {
    pub async fn sys_sigwait(&self) -> SyscallResult {
        info!("Syscall: sigwait");
        let args = self.cx.syscall_args();
        let waiting_sigset = UserReadPtr::<SignalSet>::from(args[0]).read(&self.lproc)?;

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
        Err(LinuxError::EAGAIN)
    }
}
