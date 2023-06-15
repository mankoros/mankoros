//! Memory related syscall
//!

use bitflags::bitflags;
use log::info;

use crate::{
    axerrno::AxError, consts::PAGE_MASK, memory::{pagetable::pte::PTEFlags, address::VirtAddr},
    process::user_space::user_area::UserAreaPerm,
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

impl Into<PTEFlags> for MMAPPROT {
    fn into(self) -> PTEFlags {
        // 记得加 user 项，否则用户拿到后无法访问
        let mut flag = PTEFlags::U;
        if self.contains(MMAPPROT::PROT_READ) {
            flag |= PTEFlags::R;
        }
        if self.contains(MMAPPROT::PROT_WRITE) {
            flag |= PTEFlags::W;
        }
        if self.contains(MMAPPROT::PROT_EXEC) {
            flag |= PTEFlags::X;
        }
        flag
    }
}

impl Into<UserAreaPerm> for MMAPPROT {
    fn into(self) -> UserAreaPerm {
        let mut flag = UserAreaPerm::empty();
        if self.contains(MMAPPROT::PROT_READ) {
            flag |= UserAreaPerm::READ;
        }
        if self.contains(MMAPPROT::PROT_WRITE) {
            flag |= UserAreaPerm::WRITE;
        }
        if self.contains(MMAPPROT::PROT_EXEC) {
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
    pub fn sys_brk(&mut self, brk: usize) -> SyscallResult {
        info!("Syscall brk: brk {}", brk);

        if brk == 0 {
            let cur_brk = self.lproc.with_memory(|m| m.areas().get_heap_break());
            Ok(cur_brk.bits())
        } else {
            self.lproc.with_mut_memory(|m| {
                m.areas_mut()
                    .reset_heap_break(VirtAddr::from(brk))
                    .map(|_| 0)
                    .map_err(|_| AxError::NoMemory)
            })
        }
    }

    pub fn sys_mmap(
        &mut self,
        start: usize,
        len: usize,
        prot: MMAPPROT,
        flags: MMAPFlags,
        fd: i32,
        offset: usize,
    ) -> SyscallResult {
        info!(
            "Syscall mmap: mmap start={:x} len={:} prot=[{:#?}] flags=[{:#?}] fd={} offset={:x}",
            start, len, prot, flags, fd, offset
        );

        // start == 0 表明需要 OS 为其找一段内存，而 MAP_FIXED 表明必须 mmap 在固定位置。两者是冲突的
        if start == 0 && flags.contains(MMAPFlags::MAP_FIXED) {
            return Err(AxError::InvalidInput);
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
                        .map_err(|_| AxError::NoMemory)
                });
            }
        } else {
            // File
            if fd >= 0 {
                let fd = fd as usize;
                return self.lproc.with_mut_fdtable(|f| {
                    if let Some(fd) = f.get(fd) {
                        // Currently, we don't support shared mappings.
                        return self.lproc.with_mut_memory(|m| {
                            m.areas_mut()
                                .insert_mmap_private(len, prot.into(), fd.file.clone(), offset)
                                .map(|(r, _)| r.start.bits())
                                .map_err(|_| AxError::NoMemory)
                        });
                    }
                    Err(AxError::NotFound)
                });
            }
        }

        Err(AxError::InvalidInput)
    }

    pub fn sys_munmap(&mut self, start: usize, len: usize) -> SyscallResult {
        info!("Syscall munmap: munmap start={:x} len={:x}", start, len);

        if start & PAGE_MASK != 0 {
            return Err(AxError::InvalidInput);
        }

        let range = VirtAddr::from(start)..VirtAddr::from(start + len);
        self.lproc.with_mut_memory(|m| m.unmap_range(range));

        Ok(0)
    }
}
