//! Misc syscall
//!

use log::{info, warn};

use crate::{
    arch::within_sum,
    axerrno::AxError,
    executor::hart_local::get_curr_lproc,
    here,
    process::lproc,
    timer::{TimeVal, Tms},
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
    pub fn sys_uname(&mut self, uts: *mut UtsName) -> SyscallResult {
        unsafe {
            riscv::register::sstatus::set_sum();
            (*uts) = UtsName::default();
            riscv::register::sstatus::clear_sum();
        }
        Ok(0)
    }

    pub fn sys_gettimeofday(&mut self, time_val: *mut TimeVal) -> SyscallResult {
        info!("Syscall: gettimeofday");
        within_sum(|| unsafe {
            (*time_val) = TimeVal::now();
        });
        Ok(0)
    }

    pub fn sys_times(&mut self, tms_ptr: *mut Tms) -> SyscallResult {
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
                warn!("Current hart have no lporc");
                return Err(AxError::NotFound);
            }
        }
    }
}
