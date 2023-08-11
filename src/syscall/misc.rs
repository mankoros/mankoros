//! Misc syscall
//!

use log::{debug, info};

use crate::{
    executor::util_futures::yield_now,
    here,
    memory::{UserReadPtr, UserWritePtr},
    process::lproc_mgr::GlobalLProcManager,
    signal::SignalSet,
    sync::SpinNoIrqLock,
    timer::{self, get_time_ms, wake_after, Rusage, TimeSpec, TimeVal, Tms},
    tools::{errors::SysError, handler_pool::UsizePool},
};

use super::{Syscall, SyscallResult};

static TIMER_ID_POOL: SpinNoIrqLock<UsizePool> = SpinNoIrqLock::new(UsizePool::new(1));

// copy from sys/utsname.h
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct UtsName {
    /// Operating system name (e.g., "Linux")
    pub sysname: [u8; 65],
    /// Name within "some implementation-defined network"
    pub nodename: [u8; 65],
    /// Operating system release
    /// (e.g., "2.6.28")
    pub release: [u8; 65],
    /// Operating system version
    pub version: [u8; 65],
    /// Hardware identifier
    pub machine: [u8; 65],
    /// NIS or YP domain name
    pub domainname: [u8; 65],
}

impl UtsName {
    pub fn default() -> Self {
        Self {
            sysname: Self::from_str("MankorOS"),
            nodename: Self::from_str("MankorOS-VF2"),
            release: Self::from_str("6.1.0-7-riscv64"),
            version: Self::from_str("unknown"),
            machine: Self::from_str("riscv64"),
            domainname: Self::from_str("localhost"),
        }
    }

    fn from_str(info: &str) -> [u8; 65] {
        let mut data: [u8; 65] = [0; 65];
        data[..info.len()].copy_from_slice(info.as_bytes());
        data
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct ITimerVal {
    pub it_interval: TimeVal,
    pub it_value: TimeVal,
}

const ITIMER_REAL: usize = 0;
const ITIMER_VIRTUAL: usize = 1;
const ITIMER_PROF: usize = 2;

impl<'a> Syscall<'a> {
    pub fn sys_uname(&mut self) -> SyscallResult {
        info!("Syscall: uname");
        let args = self.cx.syscall_args();
        let uts = UserWritePtr::<UtsName>::from(args[0]);
        uts.write(&self.lproc, UtsName::default())?;
        Ok(0)
    }

    pub fn sys_gettimeofday(&mut self) -> SyscallResult {
        info!("Syscall: gettimeofday");
        let args = self.cx.syscall_args();
        let tv = UserWritePtr::<TimeVal>::from(args[0]);
        tv.write(&self.lproc, TimeVal::now())?;
        Ok(0)
    }

    pub fn sys_clockgettime(&mut self) -> SyscallResult {
        info!("Syscall: clockgettime");
        let args = self.cx.syscall_args();
        let (_clock_id, time_spec) = (args[0], UserWritePtr::<TimeSpec>::from(args[1]));
        time_spec.write(&self.lproc, TimeSpec::now())?;
        Ok(0)
    }

    pub fn sys_times(&mut self) -> SyscallResult {
        info!("Syscall: times");
        let args = self.cx.syscall_args();
        let tms_ptr = UserWritePtr::<Tms>::from(args[0]);

        let (utime, stime) = self.lproc.timer().lock(here!()).output_us();
        let tms = Tms {
            tms_utime: utime,
            tms_stime: stime,
            tms_cutime: utime,
            tms_cstime: stime,
        };
        tms_ptr.write(&self.lproc, tms)?;
        Ok(0)
    }

    pub async fn sys_nanosleep(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (req, rem) = (
            UserReadPtr::<TimeSpec>::from(args[0]),
            UserWritePtr::<TimeSpec>::from(args[1]),
        );

        info!("Syscall: nanosleep");
        // Calculate end time
        let time_spec = req.read(&self.lproc)?;
        let sleep_time_ms = time_spec.time_in_ms();

        // Sleep
        let before_sleep = get_time_ms();
        wake_after(sleep_time_ms).await;
        let after_sleep = get_time_ms();
        debug_assert!(after_sleep >= before_sleep + sleep_time_ms);
        log::debug!(
            "Sleep for {} ms, actually sleep {} ms",
            sleep_time_ms,
            after_sleep - before_sleep
        );

        // Sleep is done
        // Update rem if provided
        if rem.not_null() {
            rem.write(&self.lproc, TimeSpec::new(0, 0))?;
        }
        Ok(0)
    }

    pub async fn sys_sched_yield(&mut self) -> SyscallResult {
        yield_now().await;
        Ok(0)
    }

    pub fn sys_getuid(&self) -> SyscallResult {
        info!("Syscall: getuid");
        // We don't implement user management, just return 0
        Ok(0)
    }

    pub fn sys_getrusage(&self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let who = args[0] as u32;
        let usage = UserWritePtr::<Rusage>::from(args[1]);

        info!("Syscall: getrusage, who: {who}, usage: {usage}");

        let (utime, stime) = self.lproc.timer().lock(here!()).output_us();
        let data = match who {
            0 | 1 | u32::MAX => Rusage {
                ru_utime: utime.into(),
                ru_stime: stime.into(),
            },
            _ => return Err(SysError::EINVAL),
        };
        usage.write(&self.lproc, data)?;

        debug!("data: {data:?}");
        Ok(0)
    }

    pub fn sys_setitimer(&mut self) -> SyscallResult {
        log::info!("Syscall: setitimer");
        let args = self.cx.syscall_args();
        let which = args[0];
        let new_value = UserReadPtr::<ITimerVal>::from(args[1]);

        match which {
            ITIMER_REAL => {
                let new_value = new_value.read(&self.lproc)?;
                let next_wakeup_time = new_value.it_value.time_in_ms();
                let period = new_value.it_interval.time_in_ms();
                let pid = self.lproc.id();

                log::info!("Syscall: setitimer, which: {which}, new_value: {new_value:?}, next_wakeup_time: {next_wakeup_time}, period: {period}");

                if period == 0 {
                    if next_wakeup_time != 0 {
                        // One shot timer
                        let timer_id = TIMER_ID_POOL.lock(here!()).get();
                        self.lproc.with_mut_timer_map(|m| m.insert(timer_id, true)); // armed
                        timer::call_after(next_wakeup_time, async move {
                            let proc = GlobalLProcManager::get(pid);
                            if let Some(proc) = proc {
                                let armed = proc.with_mut_timer_map(|m| {
                                    m.remove(&timer_id).expect("timer_id should exist")
                                });
                                if armed {
                                    proc.send_signal(SignalSet::SIGALRM.get_signum());
                                }
                            }
                        });
                    } else {
                        // Disarm timer
                        self.lproc.with_mut_timer_map(|m| {
                            for (_, armed) in m.iter_mut() {
                                *armed = false;
                            }
                        });
                    }
                } else {
                    // Periodic timer
                    let timer_id = TIMER_ID_POOL.lock(here!()).get();
                    timer::call_after(next_wakeup_time, async move {
                        let proc = GlobalLProcManager::get(pid);
                        if let Some(proc) = proc {
                            let armed = proc.with_mut_timer_map(|m| {
                                m.get(&timer_id).expect("timer_id should exist").clone()
                            });
                            if armed {
                                proc.send_signal(SignalSet::SIGALRM.get_signum());
                                timer::call_after(period, async move {
                                    let proc = GlobalLProcManager::get(pid);
                                    if let Some(proc) = proc {
                                        let armed = proc.with_mut_timer_map(|m| {
                                            m.get(&timer_id).expect("timer_id should exist").clone()
                                        });
                                        if armed {
                                            proc.send_signal(SignalSet::SIGALRM.get_signum());
                                        }
                                    }
                                });
                            }
                        }
                    });
                }
            }
            _ => {
                log::warn!("setitimer, which: {which}, not implemented");
            }
        }
        Ok(0)
    }
}
