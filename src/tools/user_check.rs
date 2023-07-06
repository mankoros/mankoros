use super::errors::{SysError, SysResult};
use crate::{
    memory::address::VirtAddr,
    process::{
        lproc::LightProcess,
        user_space::user_area::{PageFaultAccessType, UserAreaPerm},
    },
};
use alloc::{string::String, vec::Vec};
use log::trace;

pub struct UserCheck<'a> {
    lproc: &'a LightProcess,
}

unsafe impl Send for UserCheck<'_> {}
unsafe impl Sync for UserCheck<'_> {}

impl<'a> UserCheck<'a> {
    /// 创建一个用户态指针检查器，同时设置 SUM 模式，直到检查器被 drop 时关闭
    pub fn new_with_sum(lproc: &'a LightProcess) -> Self {
        unsafe { riscv::register::sstatus::set_sum() };
        Self { lproc }
    }

    fn has_perm(&self, vaddr: VirtAddr, perm: UserAreaPerm) -> bool {
        self.lproc.with_memory(|m| m.has_perm(vaddr, perm))
    }

    pub fn checked_read<T>(&self, ptr: *const T) -> SysResult<T> {
        let vaddr = VirtAddr::from(ptr as usize);
        if self.has_perm(vaddr, UserAreaPerm::READ) {
            unsafe { Ok(ptr.read()) }
        } else {
            Err(SysError::EFAULT)
        }
    }

    pub fn checked_write<T>(&self, ptr: *mut T, val: T) -> SysResult<()> {
        let vaddr = VirtAddr::from(ptr as usize);
        let pte = self.lproc.with_mut_memory(|m| {
            m.page_table.get_pte_copied_from_vpn(vaddr.round_down().page_num())
        });
        if self.has_perm(vaddr, UserAreaPerm::WRITE) {
            if pte.is_none() || !pte.unwrap().writable() {
                // Try Copy-on-write
                self.lproc
                    .with_mut_memory(|m| m.handle_pagefault(vaddr, PageFaultAccessType::RW))
                    .expect("Copy-on-write failed");
            }
            unsafe {
                ptr.write(val);
            }
            Ok(())
        } else {
            Err(SysError::EFAULT)
        }
    }

    pub fn checked_read_cstr(&self, ptr: *const u8) -> SysResult<String> {
        let mut s = String::new();
        let mut p = ptr;
        loop {
            // TODO-PERF: 复用找到的段的范围，减少大量的找段的时间
            let c = self.checked_read(p)?;
            if c == 0 {
                break;
            }

            trace!("checked_read_cstr: ptr: {:?}, c: {}", p, c as char);

            s.push(c as char);
            p = unsafe { p.add(1) };
        }
        Ok(s)
    }

    pub fn checked_write_cstr(&self, ptr: *mut u8, value: &str) -> SysResult<()> {
        let mut p = ptr;
        for c in value.chars() {
            self.checked_write(p, c as u8)?;
            p = unsafe { p.add(1) };
        }
        self.checked_write(p, 0)?;
        Ok(())
    }

    pub fn checked_read_2d_cstr(&self, ptr: *const *const u8) -> SysResult<Vec<String>> {
        let mut v = Vec::new();
        let mut p = ptr;
        loop {
            // TODO-PERF: 复用找到的段的范围，减少大量的找段的时间
            let ptr_s = self.checked_read(p)?;
            if ptr_s.is_null() {
                break;
            }

            trace!(
                "checked_read_2d_cstr: p: {:?}, ptr_s: {:#x}",
                p,
                ptr_s as usize
            );

            let s = self.checked_read_cstr(ptr_s)?;
            v.push(s);
            p = unsafe { p.add(1) };
        }
        Ok(v)
    }
}

impl<'a> Drop for UserCheck<'a> {
    fn drop(&mut self) {
        unsafe { riscv::register::sstatus::clear_sum() };
    }
}
