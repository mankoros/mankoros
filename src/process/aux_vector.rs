use alloc::vec::Vec;
use xmas_elf::ElfFile;

use crate::{consts::PAGE_SIZE, memory::address::VirtAddr};

/// end of vector
pub const AT_NULL: usize = 0;
/// entry should be ignored
pub const AT_IGNORE: usize = 1;
/// file descriptor of program
pub const AT_EXECFD: usize = 2;
/// program headers for program
pub const AT_PHDR: usize = 3;
/// size of program header entry
pub const AT_PHENT: usize = 4;
/// number of program headers
pub const AT_PHNUM: usize = 5;
/// system page size
pub const AT_PAGESZ: usize = 6;
/// base address of interpreter
pub const AT_BASE: usize = 7;
/// flags
pub const AT_FLAGS: usize = 8;
/// entry point of program
pub const AT_ENTRY: usize = 9;
/// program is not ELF
pub const AT_NOTELF: usize = 10;
/// real uid
pub const AT_UID: usize = 11;
/// effective uid
pub const AT_EUID: usize = 12;
/// real gid
pub const AT_GID: usize = 13;
/// effective gid
pub const AT_EGID: usize = 14;
/// string identifying CPU for optimizations
pub const AT_PLATFORM: usize = 15;
/// arch dependent hints at CPU capabilities
pub const AT_HWCAP: usize = 16;
/// frequency at which times() increments
pub const AT_CLKTCK: usize = 17;
/* AT_* values 18 through 22 are reserved */
/// secure mode boolean
pub const AT_SECURE: usize = 23;
/// string identifying real platform, may differ from AT_PLATFORM.
pub const AT_BASE_PLATFORM: usize = 24;
/// address of 16 random bytes
pub const AT_RANDOM: usize = 25;
/// extension of AT_HWCAP
pub const AT_HWCAP2: usize = 26;
/// filename of program
pub const AT_EXECFN: usize = 31;

struct AuxElement {
    pub aux_type: usize,
    pub aux_value: usize,
}
struct AuxVector {
    vec: Vec<AuxElement>,
}

impl AuxVector {
    pub fn from_elf(elf: &ElfFile, begin_addr: VirtAddr) -> Self {
        let pgm_header_addr = (begin_addr + elf.header.pt2.ph_offset() as usize).0;
        let pgm_header_cnt = elf.header.pt2.ph_count() as usize;
        let pgm_header_entry_size = elf.header.pt2.ph_entry_size() as usize;
        let entry_point = elf.header.pt2.entry_point() as usize;

        let mut auxv = Vec::new();
        macro_rules! push_elm {
            ($aux_type: expr, $aux_value: expr) => {
                auxv.push(AuxElement {
                    aux_type: $aux_type,
                    aux_value: $aux_value,
                });
            };
        }

        push_elm!(AT_PHDR, pgm_header_addr);
        push_elm!(AT_PHENT, pgm_header_entry_size);
        push_elm!(AT_PHNUM, pgm_header_cnt);

        push_elm!(AT_PAGESZ, PAGE_SIZE);
        push_elm!(AT_BASE, 0);
        push_elm!(AT_FLAGS, 0);
        push_elm!(AT_ENTRY, entry_point);

        // magic number from UltraOS, don't know why
        // ref: https://github.com/xiyurain/UltraOS/blob/3b95ec3cffe94fd5165da525c4193906366f7d5a/codes/os/src/mm/memory_set.rs#LL297C73-L297C73
        push_elm!(AT_NOTELF, 0x112d);

        // values below are copied from FTL-OS
        push_elm!(AT_UID, 0);
        push_elm!(AT_EUID, 0);
        push_elm!(AT_GID, 0);
        push_elm!(AT_EGID, 0);

        push_elm!(AT_PLATFORM, 0);
        push_elm!(AT_HWCAP, 0);
        push_elm!(AT_CLKTCK, 100);
        push_elm!(AT_SECURE, 0);

        // rest entries are not nesscary for now

        AuxVector { vec: auxv }
    }
}
