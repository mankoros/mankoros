//! Misc syscall
//!

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
}
