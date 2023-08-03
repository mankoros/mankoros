//! Misc syscall
//!

use log::info;

use crate::{
    executor::{
        hart_local::{get_curr_lproc, within_sum},
        util_futures::yield_now,
    },
    here,
    memory::{UserReadPtr, UserWritePtr},
    timer::{get_time_f64, Rusage, TimeSpec, TimeVal, Tms},
    tools::{errors::SysError, user_check::UserCheck},
};

use super::{Syscall, SyscallResult};

// copy from sys/utsname.h
#[repr(C)]
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

impl<'a> Syscall<'a> {
    pub fn sys_uname(&mut self) -> SyscallResult {
        info!("Syscall: uname");
        let args = self.cx.syscall_args();
        let uts = args[0] as *mut UtsName;

        let user_check = UserCheck::new_with_sum(&self.lproc);
        user_check.checked_write(uts, UtsName::default())?;

        Ok(0)
    }

    pub fn sys_gettimeofday(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let time_val = args[0] as *mut TimeVal;
        info!("Syscall: gettimeofday");
        within_sum(|| unsafe {
            (*time_val) = TimeVal::now();
        });
        Ok(0)
    }

    pub fn sys_clockgettime(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let _clock_id = args[0];
        let time_spec = args[1] as *mut TimeSpec;
        info!("Syscall: clockgettime");
        within_sum(|| unsafe {
            (*time_spec) = TimeSpec::now();
        });
        Ok(0)
    }

    pub fn sys_times(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let tms_ptr = args[0] as *mut Tms;

        info!("Syscall: times");
        match get_curr_lproc() {
            Some(lproc) => {
                let (utime, stime) = lproc.timer().lock(here!()).output_us();

                within_sum(|| {
                    unsafe {
                        (*tms_ptr).tms_utime = utime;
                        (*tms_ptr).tms_stime = stime;
                        // TODO: childtime calc
                        (*tms_ptr).tms_cutime = utime;
                        (*tms_ptr).tms_cstime = stime;
                    }
                });

                Ok(0)
            }
            None => {
                panic!("Current hart have no lporc");
            }
        }
    }

    pub async fn sys_nanosleep(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (req, rem): (UserReadPtr<TimeSpec>, UserWritePtr<TimeSpec>) = (
            UserReadPtr::from_usize(args[0]),
            UserWritePtr::from_usize(args[1]),
        );

        info!("Syscall: nanosleep");
        // Calculate end time
        let end_time = within_sum(|| get_time_f64() + (unsafe { *req.raw_ptr() }).time_in_sec());

        while get_time_f64() < end_time {
            yield_now().await
        }
        // Sleep is done
        // Update rem if provided
        if rem.raw_ptr_mut() as usize != 0 {
            within_sum(|| unsafe {
                (*rem.raw_ptr_mut()) = TimeSpec::new(0.0);
            })
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
        let usage = args[1] as *mut Rusage;

        info!("Syscall: getrusage");

        match get_curr_lproc() {
            Some(lproc) => {
                let (utime, stime) = lproc.timer().lock(here!()).output_us();

                match who {
                    0 | 1 | u32::MAX => {
                        within_sum(|| unsafe {
                            (*usage).ru_utime = utime.into();
                            (*usage).ru_stime = stime.into();
                        });
                    }
                    _ => return Err(SysError::EINVAL),
                };
                Ok(0)
            }
            None => {
                panic!("Current hart have no lporc");
            }
        }
    }
}
