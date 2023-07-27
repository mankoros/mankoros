mod dir;
mod file;
mod fs;
mod tools;

use fs::Fat32FS;

pub type DEntryIter<'a> = dir::GroupDEntryIter<'a>;
pub use dir::FatDEntryData;
pub use file::FATFile;
pub use fs::BlkDevRef;
pub use fs::FatFSWrapper;

// https://wiki.osdev.org/FAT

type BlockID = u64;
type SectorID = u64;
// fat32 ==> cluster id is within u32
type ClusterID = u32;
// byte offset within a cluster
type ClsOffsetT = u16;
type SctOffsetT = u16;
macro_rules! parse {
    (u8, $buf:expr, $beg_idx:expr) => {
        $buf[$beg_idx]
    };
    (u16, $buf:expr, $beg_idx:expr) => {
        u16::from_le_bytes($buf[$beg_idx..($beg_idx + 2)].try_into().unwrap())
    };
    (u32, $buf:expr, $beg_idx:expr) => {
        u32::from_le_bytes($buf[$beg_idx..($beg_idx + 4)].try_into().unwrap())
    };
}

pub(self) use parse;
