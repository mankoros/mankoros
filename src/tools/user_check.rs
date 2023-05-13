use crate::{process::{lproc::LightProcess, user_space::user_area::UserAreaPerm}, memory::address::VirtAddr};
use alloc::{string::String, vec::Vec};
use log::{trace};

pub struct UserCheck<'a> {
    lproc: &'a LightProcess
}

impl<'a> UserCheck<'a> {
    /// 创建一个用户态指针检查器, 同时设置 SUM 模式, 直到检查器被 drop 时关闭
    pub fn new_with_sum(lproc: &'a LightProcess) -> Self {
        unsafe { riscv::register::sstatus::set_sum() };
        Self {
            lproc
        }
    }

    fn has_perm(&self, vaddr: VirtAddr, perm: UserAreaPerm) -> bool {
        self.lproc.with_memory(|m| m.has_perm(vaddr, perm))
    }

    pub fn checked_read<T>(&self, ptr: *const T) -> Result<T, ()> {
        let vaddr = (ptr as usize).into();
        if self.has_perm(vaddr, UserAreaPerm::READ) {
            unsafe {
                Ok(ptr.read())
            }
        } else {
            Err(())
        }
    }

    pub fn checked_write<T>(&self, ptr: *mut T, val: T) -> Result<(), ()> {
        let vaddr = (ptr as usize).into();
        if self.has_perm(vaddr, UserAreaPerm::WRITE) {
            unsafe {
                ptr.write(val);
            }
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn checked_read_cstr(&self, ptr: *const u8) -> Result<String, ()> {
        let mut s = String::new();
        let mut p = ptr;
        loop {
            // TODO-PERF: 复用找到的段的范围, 减少大量的找段的时间
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

    pub fn checked_read_2d_cstr(&self, ptr: *const *const u8) -> Result<Vec<String>, ()> {
        let mut v = Vec::new();
        let mut p = ptr;
        loop {
            // TODO-PERF: 复用找到的段的范围, 减少大量的找段的时间
            let ptr_s = self.checked_read(p)?;
            if ptr_s.is_null() {
                break;
            }

            trace!("checked_read_2d_cstr: p: {:?}, ptr_s: {:#x}", p, ptr_s as usize);

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