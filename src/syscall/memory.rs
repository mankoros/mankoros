//! Memory related syscall
//!

use bitflags::bitflags;
use log::info;

use crate::{
    consts::PAGE_MASK,
    memory::{address::VirtAddr, pagetable::pte::PTEFlags},
    process::user_space::user_area::UserAreaPerm,
    tools::errors::SysError,
};

use super::{Syscall, SyscallResult};

bitflags! {
    /// 指定 mmap 的选项
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct MMAPPROT: u32 {
        /// 不挂起当前进程，直接返回
        const PROT_READ = 1 << 0;
        /// 报告已执行结束的用户进程的状态
        const PROT_WRITE = 1 << 1;
        /// 报告还未结束的用户进程的状态
        const PROT_EXEC = 1 << 2;
    }
}

impl From<MMAPPROT> for PTEFlags {
    fn from(val: MMAPPROT) -> Self {
        // 记得加 user 项，否则用户拿到后无法访问
        let mut flag = PTEFlags::U;
        if val.contains(MMAPPROT::PROT_READ) {
            flag |= PTEFlags::R;
        }
        if val.contains(MMAPPROT::PROT_WRITE) {
            flag |= PTEFlags::W;
        }
        if val.contains(MMAPPROT::PROT_EXEC) {
            flag |= PTEFlags::X;
        }
        flag
    }
}

impl From<MMAPPROT> for UserAreaPerm {
    fn from(val: MMAPPROT) -> Self {
        let mut flag = UserAreaPerm::empty();
        if val.contains(MMAPPROT::PROT_READ) {
            flag |= UserAreaPerm::READ;
        }
        if val.contains(MMAPPROT::PROT_WRITE) {
            flag |= UserAreaPerm::WRITE;
        }
        if val.contains(MMAPPROT::PROT_EXEC) {
            flag |= UserAreaPerm::EXECUTE;
        }

        flag
    }
}

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct MMAPFlags: u32 {
        /// 对这段内存的修改是共享的
        const MAP_SHARED = 1 << 0;
        /// 对这段内存的修改是私有的
        const MAP_PRIVATE = 1 << 1;
        // 以上两种只能选其一

        /// 固定位置
        const MAP_FIXED = 1 << 4;
        /// 不映射到实际文件
        const MAP_ANONYMOUS = 1 << 5;
        /// 映射时不保留空间，即可能在实际使用 mmap 出来的内存时内存溢出
        const MAP_NORESERVE = 1 << 14;
    }
}

impl<'a> Syscall<'a> {
    pub fn sys_brk(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let brk = args[0];
        info!("Syscall brk: brk {:x}", brk);

        if brk == 0 {
            let cur_brk = self.lproc.with_memory(|m| m.areas().get_heap_break());
            Ok(cur_brk.bits())
        } else {
            self.lproc.with_mut_memory(|m| {
                m.areas_mut()
                    .reset_heap_break(VirtAddr::from(brk))
                    .map(|_| // If Ok, then return the requested brk
                     brk)
            })
        }
    }

    pub fn sys_mmap(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (start, len, prot, flags, fd, offset) = (
            args[0],
            args[1],
            MMAPPROT::from_bits(args[2] as u32).unwrap(),
            MMAPFlags::from_bits(args[3] as u32).unwrap(),
            args[4] as i32,
            args[5],
        );

        info!(
            "Syscall mmap: mmap start={:x} len={:} prot=[{:?}] flags=[{:?}] fd={} offset={:x}",
            start, len, prot, flags, fd, offset
        );

        // start == 0 表明需要 OS 为其找一段内存，而 MAP_FIXED 表明必须 mmap 在固定位置。两者是冲突的
        if start == 0 && flags.contains(MMAPFlags::MAP_FIXED) {
            return Err(SysError::EINVAL);
        }
        // 是否可以放在任意位置
        let _anywhere = start == 0 || !flags.contains(MMAPFlags::MAP_FIXED);

        if flags.contains(MMAPFlags::MAP_ANONYMOUS) {
            // 根据 linux 规范需要 fd 设为 -1 且 offset 设为 0
            if fd == -1 && offset == 0 {
                return self.lproc.with_mut_memory(|m| {
                    m.areas_mut()
                        .insert_mmap_anonymous(len, prot.into())
                        .map(|(r, _)| r.start.bits())
                });
            }
        } else {
            // File
            if fd >= 0 {
                let fd = fd as usize;
                return self.lproc.with_mut_fdtable(|f| {
                    if let Some(fd) = f.get(fd) {
                        // Currently, we don't support shared mappings.
                        self.lproc.with_mut_memory(|m| {
                            m.areas_mut()
                                .insert_mmap_private(len, prot.into(), fd.file.clone(), offset)
                                .map(|(r, _)| r.start.bits())
                        })
                    } else {
                        Err(SysError::EBADF)
                    }
                });
            }
        }

        Err(SysError::EINVAL)
    }

    pub fn sys_munmap(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (start, len) = (args[0], args[1]);
        info!("Syscall munmap: munmap start={:x} len={:x}", start, len);

        if start & PAGE_MASK != 0 {
            return Err(SysError::EINVAL);
        }

        let range = VirtAddr::from(start)..VirtAddr::from(start + len);
        self.lproc.with_mut_memory(|m| m.unmap_range(range));

        Ok(0)
    }
}
