//! Memory related syscall
//!

use bitflags::bitflags;
use log::info;

use crate::{
    consts::PAGE_MASK,
    memory::{address::VirtAddr, pagetable::pte::PTEFlags, UserWritePtr},
    process::user_space::{
        shm_mgr::{global_shm_mgr, ShmId},
        user_area::UserAreaPerm,
    },
    syscall::memory::ipc::{ShmIdDs, IPC_RMID, IPC_SET, IPC_STAT},
    tools::errors::{LinuxError, SysError},
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

        log::info!(
            "Syscall mmap: mmap start=0x{:x} len=0x{:x} prot=[{:?}] flags=[{:?}] fd={} offset={:x}",
            start,
            len,
            prot,
            flags,
            fd,
            offset
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
        log::info!("Syscall munmap: munmap start={:x} len={:x}", start, len);

        if start & PAGE_MASK != 0 {
            return Err(SysError::EINVAL);
        }

        let range = VirtAddr::from(start)..VirtAddr::from(start + len);
        self.lproc.with_mut_memory(|m| m.unmap_range(range));

        Ok(0)
    }

    pub fn sys_mprotect(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (start, len, prot) = (
            args[0],
            args[1],
            MMAPPROT::from_bits(args[2] as u32).unwrap(),
        );
        log::info!(
            "Syscall mprotect: mprotect start=0x{:x} len=0x{:x} prot=[{:?}]",
            start,
            len,
            prot
        );

        if start & PAGE_MASK != 0 {
            return Err(LinuxError::EINVAL);
        }

        self.lproc.with_mut_memory(|m| {
            let area = m.areas_mut().get_area_mut(start.into()).ok_or(LinuxError::ENOMEM)?;
            area.set_perm(prot.into());
            Ok(0)
        })?;

        Ok(0)
    }

    pub fn sys_shmget(&mut self) -> SyscallResult {
        use ipc::*;

        let args = self.cx.syscall_args();
        let (key, size, shmflg) = (args[0], args[1], args[2]);
        info!(
            "Syscall shmget: shmget key={} size={} shmflg={}",
            key, size, shmflg
        );

        let mgr = global_shm_mgr();

        let shm = if let Some(shm) = mgr.get(key) {
            if (shmflg & IPC_CREAT != 0) && (shmflg & IPC_EXCL != 0) {
                return Err(SysError::EEXIST);
            }
            shm
        } else {
            if shmflg & IPC_CREAT == 0 {
                return Err(SysError::ENOENT);
            }

            let key = if key == IPC_PRIVATE { None } else { Some(key) };

            mgr.create(key, size, self.lproc.id())?
        };

        let id = self.lproc.with_mut_shm_table(|f| f.alloc(shm));
        Ok(id)
    }

    pub fn sys_shmat(&mut self) -> SyscallResult {
        use ipc::*;
        let args = self.cx.syscall_args();
        let (shmid, shmaddr, shmflg) = (args[0], args[1], args[2]);
        info!(
            "Syscall shmat: shmat shmid={} shmaddr={} shmflg={}",
            shmid, shmaddr, shmflg
        );

        let shm = self.lproc.with_shm_table(|f| f.get(shmid)).ok_or(SysError::EINVAL)?;
        let vaddr = if shmaddr == 0 {
            None
        } else {
            Some(VirtAddr::from(shmaddr))
        };
        let pid = self.lproc.id();
        let perm = if shmflg & SHM_RDONLY != 0 {
            UserAreaPerm::READ
        } else {
            UserAreaPerm::READ | UserAreaPerm::WRITE
        };

        let vaddr = self
            .lproc
            .with_mut_memory(|m| m.attach_shm(vaddr, pid, shmid as ShmId, shm, perm))?;

        Ok(vaddr.bits())
    }

    pub fn sys_shmdt(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let shmaddr = VirtAddr::from(args[0]);
        info!("Syscall shmdt: shmdt shmaddr={:?}", shmaddr);
        self.lproc.with_mut_memory(|m| m.detach_shm(shmaddr)).map(|()| 0)
    }

    pub fn sys_shmctl(&mut self) -> SyscallResult {
        let args = self.cx.syscall_args();
        let (shmid, cmd, buf) = (args[0], args[1], UserWritePtr::<ShmIdDs>::from(args[2]));
        info!(
            "Syscall shmctl: shmctl shmid={} cmd={} buf={}",
            shmid, cmd, buf
        );

        let shm = self.lproc.with_shm_table(|f| f.get(shmid)).ok_or(SysError::EINVAL)?;

        match cmd {
            IPC_STAT => {
                let data = ShmIdDs {
                    shm_perm: ipc::IPCPerm {
                        __key: shmid as _,
                        uid: 0,
                        gid: 0,
                        cuid: 0,
                        cgid: 0,
                        mode: 0o777,
                        __seq: 0,
                        __pad2: 0,
                        __glibc_reserved1: 0,
                        __glibc_reserved2: 0,
                    },
                    shm_segsz: shm.size() as _,
                    shm_atime: 0,
                    shm_dtime: 0,
                    shm_ctime: 0,
                    shm_cpid: Into::<usize>::into(shm.creater()) as _,
                    shm_lpid: Into::<usize>::into(shm.last_operater()) as _,
                    shm_nattch: shm.attach_cnt() as _,
                    __glibc_reserved4: 0,
                    __glibc_reserved5: 0,
                };
                buf.write(&self.lproc, data)?;
                Ok(0)
            }
            IPC_SET => {
                log::info!("IPC_SET can change nothing now");
                Ok(0)
            }
            IPC_RMID => {
                let result = self.lproc.with_mut_shm_table(|f| f.remove(shmid));
                match result {
                    Some(_) => Ok(0),
                    None => Err(SysError::EINVAL),
                }
            }
            _ => Err(SysError::EINVAL),
        }
    }
}

mod ipc {
    /* Mode bits for `msgget', `semget', and `shmget'.  */
    /// Create key if key does not exist.
    pub const IPC_CREAT: usize = 0o1000;
    /// Fail if key exists.
    pub const IPC_EXCL: usize = 0o2000;
    /// Return error on wait.
    pub const IPC_NOWAIT: usize = 0o4000;

    /* Control commands for `msgctl', `semctl', and `shmctl'.  */
    /// Remove identifier.
    pub const IPC_RMID: usize = 0;
    /// Set `ipc_perm' options.
    pub const IPC_SET: usize = 1;
    /// Get `ipc_perm' options.
    pub const IPC_STAT: usize = 2;
    /// Get kernel structure.
    pub const IPC_INFO: usize = 3;

    /* Special key values.  */
    /// Private key.
    pub const IPC_PRIVATE: usize = 0;

    pub const SHM_RDONLY: usize = 0o10000;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct IPCPerm {
        pub __key: u32,
        pub uid: u32,
        pub gid: u32,
        pub cuid: u32,
        pub cgid: u32,
        pub mode: u32,
        pub __seq: u16,
        pub __pad2: u16,
        pub __glibc_reserved1: u64,
        pub __glibc_reserved2: u64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct ShmIdDs {
        pub shm_perm: IPCPerm,
        pub shm_segsz: u64,
        pub shm_atime: u64,
        pub shm_dtime: u64,
        pub shm_ctime: u64,
        pub shm_cpid: u32,
        pub shm_lpid: u32,
        pub shm_nattch: u64,
        pub __glibc_reserved4: u64,
        pub __glibc_reserved5: u64,
    }
}
