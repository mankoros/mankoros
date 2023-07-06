//! Misc syscall
//!

use log::info;

use crate::{
    arch::within_sum,
    executor::{hart_local::get_curr_lproc, util_futures::yield_now},
    here,
    memory::{UserReadPtr, UserWritePtr},
    timer::{get_time_f64, TimeSpec, TimeVal, Tms},
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
            release: Self::from_str("rolling"),
            version: Self::from_str("unknown"),
            machine: Self::from_str("unknown"),
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
        let args = self.cx.syscall_args();
        let uts = args[0] as *mut UtsName;

        unsafe {
            riscv::register::sstatus::set_sum();
            (*uts) = UtsName::default();
            riscv::register::sstatus::clear_sum();
        }
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

                return Ok(0);
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
}
