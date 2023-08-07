//! 一个异步环境的 elf 解析器, adapted from FTL-OS

use core::mem::{self, MaybeUninit};

use crate::{
    fs::new_vfs::top::VfsFileRef,
    tools::errors::{SysError, SysResult},
};
use alloc::{string::String, vec::Vec};
use xmas_elf::{header::Class, program::Flags};

#[derive(Copy, Clone, Debug)]
pub struct PhType_(pub u32);
pub type PhType = xmas_elf::program::Type;

#[derive(Copy, Clone, Debug)]
pub struct ShType_(pub u32);
pub type ShType = xmas_elf::sections::ShType;

#[derive(Copy, Clone, Debug)]
#[repr(C)]
pub struct ProgramHeader {
    pub type_: PhType_,
    pub flags: Flags,
    pub offset: u64,
    pub virtual_addr: u64,
    pub physical_addr: u64,
    pub file_size: u64,
    pub mem_size: u64,
    pub align: u64,
}

#[derive(Debug)]
#[repr(C)]
pub struct SectionHeader {
    name: u32,
    type_: ShType_,
    flags: u64,
    address: u64,
    offset: u64,
    size: u64,
    link: u32,
    info: u32,
    align: u64,
    entry_size: u64,
}

type HeaderPt1 = xmas_elf::header::HeaderPt1;
type HeaderPt2 = xmas_elf::header::HeaderPt2_<u64>;

pub struct ElfAnalyzer {
    file: VfsFileRef,
    pub pt1: HeaderPt1,
    pub pt2: HeaderPt2,
}

impl VfsFileRef {
    pub async fn read_obj<T: Sized>(&self, offset: usize) -> SysResult<T> {
        let size = mem::size_of::<T>();

        let mut t = MaybeUninit::<T>::uninit();
        let buf =
            unsafe { core::slice::from_raw_parts_mut(t.as_mut_ptr() as *mut T as *mut u8, size) };

        let n = self.read_at(offset, buf).await?;
        if n < size {
            Err(SysError::EFAULT)
        } else {
            Ok(unsafe { t.assume_init() })
        }
    }
}

pub async fn parse(file: &VfsFileRef) -> SysResult<ElfAnalyzer> {
    const PH1_SIZE: usize = mem::size_of::<HeaderPt1>();
    const PH2_SIZE: usize = mem::size_of::<HeaderPt2>();

    let pt1: HeaderPt1 = file.read_obj(0).await?;

    if pt1.magic != [0x7f, b'E', b'L', b'F'] {
        log::warn!("elf parse error: magic error");
        return Err(SysError::EFAULT);
    }
    assert!(
        pt1.class() == Class::SixtyFour,
        "elf parse error: not 64bit"
    );

    let pt2: HeaderPt2 = file.read_obj(PH1_SIZE).await?;

    Ok(ElfAnalyzer {
        file: file.clone(),
        pt1,
        pt2,
    })
}

impl ElfAnalyzer {
    pub fn ph_count(&self) -> usize {
        self.pt2.ph_count as usize
    }
    pub fn sh_count(&self) -> usize {
        self.pt2.sh_count as usize
    }

    pub async fn program_header(&self, index: usize) -> SysResult<ProgramHeader> {
        const PH_SIZE: usize = mem::size_of::<ProgramHeader>();
        debug_assert!(index < self.ph_count());

        let pt2 = &self.pt2;
        if pt2.ph_offset == 0 || pt2.ph_entry_size == 0 {
            log::warn!("There are no program headers in this file");
            return Err(SysError::EFAULT);
        }

        let size = pt2.ph_entry_size as usize;
        let start = pt2.ph_offset as usize + index * size;

        self.file.read_obj(start).await
    }
    pub async fn section_header(&self, index: usize) -> SysResult<SectionHeader> {
        const SH_SIZE: usize = mem::size_of::<SectionHeader>();
        assert!(index < self.sh_count());

        let size = self.pt2.sh_entry_size as usize;
        let start = self.pt2.sh_offset as usize + index * size;

        self.file.read_obj(start).await
    }

    pub async fn find_section_by_name(&self, name: &str) -> SysResult<Option<SectionHeader>> {
        for i in 0..self.sh_count() {
            let section = self.section_header(i).await?;
            if let Ok(sec_name) = section.get_name(self).await {
                if sec_name == name {
                    return Ok(Some(section));
                }
            }
        }
        Ok(None)
    }

    async fn get_shstr(&self, index: u32) -> SysResult<String> {
        let offset = self.section_header(self.pt2.sh_str_index as usize).await?.offset();
        let mut buf = [0; 128];
        let mut cur = offset + index as usize;
        let mut s = Vec::new();
        'outer: loop {
            let n = self.file.read_at(cur, &mut buf).await?;
            for &c in &buf[..n] {
                if c == 0 {
                    break 'outer;
                }
                s.push(c);
            }
            if n < buf.len() {
                break;
            }
            cur += n;
        }
        String::from_utf8(s).map_err(|_e| SysError::EFAULT)
    }
}

impl ProgramHeader {
    pub fn type_(&self) -> SysResult<PhType> {
        self.type_.as_type()
    }
}

impl SectionHeader {
    pub async fn raw_data(&self, elf: &ElfAnalyzer) -> SysResult<Vec<u8>> {
        assert_ne!(self.get_type().unwrap(), ShType::Null);
        let mut v = Vec::with_capacity(self.size());
        v.resize(self.size(), 0);
        let n = elf.file.read_at(self.offset(), &mut v).await?;
        if n != v.len() {
            Err(SysError::EFAULT)
        } else {
            Ok(v)
        }
    }
    pub fn get_type(&self) -> SysResult<ShType> {
        self.type_().as_type()
    }
    fn type_(&self) -> ShType_ {
        self.type_
    }
    fn name(&self) -> u32 {
        self.name
    }
    fn size(&self) -> usize {
        self.size as usize
    }
    fn offset(&self) -> usize {
        self.offset as usize
    }
    pub async fn get_name(&self, elf: &ElfAnalyzer) -> SysResult<String> {
        match self.get_type()? {
            ShType::Null => Err(SysError::EFAULT),
            _ => elf.get_shstr(self.name()).await,
        }
    }
}

impl PhType_ {
    pub fn as_type(&self) -> SysResult<PhType> {
        use xmas_elf::program::{TYPE_GNU_RELRO, TYPE_HIOS, TYPE_HIPROC, TYPE_LOOS, TYPE_LOPROC};
        match self.0 {
            0 => Ok(PhType::Null),
            1 => Ok(PhType::Load),
            2 => Ok(PhType::Dynamic),
            3 => Ok(PhType::Interp),
            4 => Ok(PhType::Note),
            5 => Ok(PhType::ShLib),
            6 => Ok(PhType::Phdr),
            7 => Ok(PhType::Tls),
            TYPE_GNU_RELRO => Ok(PhType::GnuRelro),
            t if (TYPE_LOOS..=TYPE_HIOS).contains(&t) => Ok(PhType::OsSpecific(t)),
            t if (TYPE_LOPROC..=TYPE_HIPROC).contains(&t) => Ok(PhType::ProcessorSpecific(t)),
            _ => Err(SysError::EFAULT),
        }
    }
}

impl ShType_ {
    pub fn as_type(self) -> SysResult<ShType> {
        use xmas_elf::sections::{
            SHT_HIOS, SHT_HIPROC, SHT_HIUSER, SHT_LOOS, SHT_LOPROC, SHT_LOUSER,
        };

        match self.0 {
            0 => Ok(ShType::Null),
            1 => Ok(ShType::ProgBits),
            2 => Ok(ShType::SymTab),
            3 => Ok(ShType::StrTab),
            4 => Ok(ShType::Rela),
            5 => Ok(ShType::Hash),
            6 => Ok(ShType::Dynamic),
            7 => Ok(ShType::Note),
            8 => Ok(ShType::NoBits),
            9 => Ok(ShType::Rel),
            10 => Ok(ShType::ShLib),
            11 => Ok(ShType::DynSym),
            // sic.
            14 => Ok(ShType::InitArray),
            15 => Ok(ShType::FiniArray),
            16 => Ok(ShType::PreInitArray),
            17 => Ok(ShType::Group),
            18 => Ok(ShType::SymTabShIndex),
            st if (SHT_LOOS..=SHT_HIOS).contains(&st) => Ok(ShType::OsSpecific(st)),
            st if (SHT_LOPROC..=SHT_HIPROC).contains(&st) => Ok(ShType::ProcessorSpecific(st)),
            st if (SHT_LOUSER..=SHT_HIUSER).contains(&st) => Ok(ShType::User(st)),
            _ => Err(SysError::EFAULT),
        }
    }
}
