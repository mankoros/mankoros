use super::{ClsOffsetT, ClusterID, FATFile, Fat32FS};
use crate::{
    fs::{
        disk::BLOCK_SIZE,
        fat32::parse,
        new_vfs::{underlying::ConcreteDEntryRef, VfsFileAttr, VfsFileKind},
    },
    tools::errors::SysResult,
};
use alloc::{boxed::Box, string::String, vec::Vec};
use core::{future::Future, pin::pin, slice, task::Poll};
use futures::Stream;

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy)]
    struct Fat32DEntryAttr : u8 {
        const READ_ONLY = 0x01;
        const HIDDEN = 0x02;
        const SYSTEM = 0x04;
        const VOLUME_ID = 0x08;
        const DIRECTORY = 0x10;
        const ARCHIVE = 0x20;
        const LFN =
            Self::READ_ONLY.bits() | Self::HIDDEN.bits() |
            Self::SYSTEM.bits() | Self::VOLUME_ID.bits();
    }
}

#[derive(Clone)]
pub struct FATDentry {
    // info about the dentry itself
    fs: &'static Fat32FS,
    cluster_id: ClusterID,
    cluster_offset: ClsOffsetT,

    // info about the file represented by the dentry
    attr: Fat32DEntryAttr,
    begin_cluster: ClusterID,
    name: String,
    size: u32,
}

impl ConcreteDEntryRef for FATDentry {
    type FileT = FATFile;

    fn name(&self) -> String {
        self.name.clone()
    }

    fn attr(&self) -> VfsFileAttr {
        let kind = if self.attr.contains(Fat32DEntryAttr::DIRECTORY) {
            VfsFileKind::Directory
        } else {
            VfsFileKind::RegularFile
        };

        let byte_size = self.size as usize;
        let block_count = (byte_size / BLOCK_SIZE) + !(byte_size % BLOCK_SIZE == 0) as usize;

        VfsFileAttr {
            kind,
            device_id: self.fs.device_id(),
            self_device_id: 0,
            byte_size,
            block_count,
            access_time: 0,
            modify_time: 0,
            create_time: 0,
        }
    }

    fn file(&self) -> Self::FileT {
        FATFile {
            fs: self.fs,
            begin_cluster: self.begin_cluster,
            last_cluster: None,
        }
    }
}

pub struct DEntryIter {
    fs: &'static Fat32FS,
    buf: Box<[u8]>,
    cur_cluster: ClusterID,
    cur_offset: ClsOffsetT,
    is_end: bool,
    is_uninit: bool,
}

impl DEntryIter {
    pub fn new(fs: &'static Fat32FS, begin_cluster: ClusterID) -> Self {
        let buf = unsafe { Box::new_uninit_slice(fs.cluster_size_byte).assume_init() };
        Self {
            fs,
            buf,
            cur_cluster: begin_cluster,
            cur_offset: 0,
            is_end: false,
            is_uninit: true,
        }
    }

    fn at_cluster_end(&self) -> bool {
        self.cur_offset == self.buf.len() as u16
    }
}

impl Stream for DEntryIter {
    type Item = SysResult<FATDentry>;

    fn poll_next(
        self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let this = unsafe { self.get_unchecked_mut() };

        // check if it has been ended
        if this.is_end {
            return Poll::Ready(None);
        }

        // record begin cluster and offset for DEntryRef
        let (begin_cls, begin_offset) = if this.at_cluster_end() {
            (this.cur_cluster + 1, 0)
        } else {
            (this.cur_cluster, this.cur_offset)
        };

        let mut lfn_names = Vec::<(u8, [u16; 13])>::new();
        loop {
            log::trace!(
                "DEntryIter: (cluster, cur_offset): {}, {}",
                this.cur_cluster,
                this.cur_offset
            );

            // check if need to move to next cluster
            if this.at_cluster_end() {
                let next_cls =
                    this.fs.with_fat(|fat_table_mgr| fat_table_mgr.next(this.cur_cluster));
                // if not next, end
                let next_cls = match next_cls {
                    Some(next_cls) => next_cls,
                    None => {
                        this.is_end = true;
                        return Poll::Ready(None);
                    }
                };
                this.is_uninit = true;
                // update cur_cluster and cur_offset
                this.cur_cluster = next_cls;
                this.cur_offset = 0;
            }

            // check if need to read the current cluster to buf
            if this.is_uninit {
                // if read pending, return pending
                // if read error, return error
                match pin!(this.fs.read_cluster(this.cur_cluster, &mut this.buf)).poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(Err(e)) => return Poll::Ready(Some(Err(e))),
                    _ => {}
                }

                this.is_uninit = false;
            }

            // read a dentry
            let dentry_raw = {
                let offset = this.cur_offset as usize;
                &this.buf[offset..(offset + 32)]
            };
            this.cur_offset += 32;

            // read the first byte to determine whether it is a valid dentry
            let first_byte = parse!(u8, dentry_raw, 0);
            if first_byte == 0 {
                // an empty entry, indicating the end of dentries
                this.is_end = true;
                return Poll::Ready(None);
            } else if first_byte == 0xE5 {
                // deleted entry, indicating an unused slot
                continue;
            }

            // read the attribute byte to determine whether it is a LFN entry
            let attr = parse!(u8, dentry_raw, 11);
            if attr == 0x0F {
                // LFN
                let ord = parse!(u8, dentry_raw, 0);
                lfn_names.push((ord, [0; 13]));
                let lfn_name = &mut lfn_names.last_mut().unwrap().1;
                // copy the first 5 * 2char, then 6 * 2char, finanlly 2 * 2char
                lfn_name[0..5].copy_from_slice(unsafe {
                    slice::from_raw_parts(dentry_raw[1..11].as_ptr() as *const u16, 5)
                });
                lfn_name[5..11].copy_from_slice(unsafe {
                    slice::from_raw_parts(dentry_raw[14..26].as_ptr() as *const u16, 6)
                });
                lfn_name[11..13].copy_from_slice(unsafe {
                    slice::from_raw_parts(dentry_raw[28..32].as_ptr() as *const u16, 2)
                });
            } else {
                // standard 8.3, or 8.3 with LFN
                // collect all the information to build a result dentry

                // if not LFN, the name is in 8.3
                let name = if lfn_names.len() == 0 {
                    // 8.3 name use space (0x20) for "no char"
                    let mut name_chars = Vec::<u8>::with_capacity(8 + 1 + 3);
                    for i in 0..8 {
                        let c = parse!(u8, dentry_raw, i);
                        if c == 0x20 {
                            break;
                        }
                        name_chars.push(c);
                    }
                    // if no extension, no dot
                    if parse!(u8, dentry_raw, 8) != b' ' {
                        name_chars.push(b'.');
                        for i in 8..11 {
                            let c = parse!(u8, dentry_raw, i);
                            if c == 0x20 {
                                break;
                            }
                            name_chars.push(c);
                        }
                    }
                    // use from_utf8 to parse ASCII name
                    String::from_utf8(name_chars).unwrap()
                } else {
                    // sort the LFN entries we have found
                    // the LFS char is 2-byte, usually UTF-16
                    let mut name_chars = Vec::<u16>::with_capacity(13 * lfn_names.len());
                    lfn_names.sort_by_cached_key(|(ord, _)| *ord);
                    'outer: for (_, lfn_name) in lfn_names.iter() {
                        // then collect all the valid chars (not '\0\0')
                        for c in lfn_name.iter() {
                            if *c == 0 {
                                break 'outer;
                            }
                            name_chars.push(*c);
                        }
                    }
                    String::from_utf16(&name_chars).unwrap()
                };

                // then collect other information

                let attr = Fat32DEntryAttr::from_bits(attr).unwrap();

                let cluster_high = parse!(u16, dentry_raw, 20);
                let cluster_low = parse!(u16, dentry_raw, 26);
                let begin_cluster = ((cluster_high as u32) << 16) | (cluster_low as u32);

                let size = parse!(u32, dentry_raw, 28);

                let dentry = FATDentry {
                    fs: this.fs,
                    cluster_id: begin_cls,
                    cluster_offset: begin_offset,

                    name,
                    attr,
                    begin_cluster,
                    size,
                };

                return Poll::Ready(Some(Ok(dentry)));
            }
        }
    }
}
