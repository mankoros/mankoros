use super::{
    dir::{
        AtomDEPos, AtomDEntryView, Fat32DEntryAttr, GroupDEPos, GroupDEntryIter,
        Standard8p3EntryRepr,
    },
    tools::{ClusterChain, WithDirty},
    ClusterID, Fat32FS, SectorID,
};
use crate::{
    fs::new_vfs::{VfsFileAttr, VfsFileKind},
    tools::errors::{dyn_future, SysError, SysResult},
};
use alloc::{boxed::Box, string::ToString, vec::Vec};
use core::{cmp::Reverse, pin::Pin};
use futures::Stream;
use ringbuffer::RingBufferExt;

pub(super) struct StdEntryEditor {
    pub(super) sector: SectorID,
    pub(super) offset: u16,
    pub(super) std: WithDirty<Standard8p3EntryRepr>,
}

impl StdEntryEditor {
    pub fn std(&self) -> &Standard8p3EntryRepr {
        self.std.as_ref()
    }
    pub fn std_mut(&self) -> &mut Standard8p3EntryRepr {
        self.std.as_mut()
    }

    pub async fn sync(&self, fs: &'static Fat32FS) -> SysResult<()> {
        let bc = fs.block_dev().get(self.sector).await?;
        let ptr = &bc.as_mut_slice()[self.offset as usize..][..32] as *const _ as *mut [u8]
            as *mut Standard8p3EntryRepr;
        unsafe { *ptr = self.std().clone() };
        self.std.clear();
        Ok(())
    }
}

pub struct FATFile {
    fs: &'static Fat32FS,
    editor: StdEntryEditor,
    chain: ClusterChain,
    gde_pos: GroupDEPos,
}

fn round_up(x: usize, y: usize) -> usize {
    (x + y - 1) / y
}
