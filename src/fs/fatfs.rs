use super::disk::BLOCK_SIZE;
use super::new_vfs::dentry_cache::DEntryCacheDir;
use super::new_vfs::sync_attr_cache::SyncAttrCacheFile;
use super::new_vfs::top::{VfsFS, VfsFileRef};
use super::new_vfs::underlying::{ConcreteFile, DEntryRef};
use super::new_vfs::{VfsFileAttr, VfsFileKind};
use super::partition::Partition;
use crate::tools::errors::{dyn_future, ASysResult, SysError};
use alloc::string::String;
use alloc::vec::Vec;
use core::cell::SyncUnsafeCell;
use core::mem::MaybeUninit;
use fatfs::{self, IoBase, IoError, Read, Seek, Write};
use log::warn;

/// Implementation of the fatfs glue code
/// FAT32 FS is supposed to work upon a partition
///
// Implementation for Partition
impl fatfs::IoBase for Partition {
    type Error = ();
}
impl fatfs::Read for Partition {
    fn read(&mut self, mut buf: &mut [u8]) -> Result<usize, Self::Error> {
        let mut read_len = 0;
        while !buf.is_empty() {
            match self.read_one(buf) {
                Ok(0) => break,
                Ok(n) => {
                    let tmp = buf;
                    buf = &mut tmp[n..];
                    read_len += n;
                }
                Err(_) => return Err(()),
            }
        }
        Ok(read_len)
    }
}

impl fatfs::Write for Partition {
    fn write(&mut self, mut buf: &[u8]) -> Result<usize, Self::Error> {
        let mut write_len = 0;
        while !buf.is_empty() {
            match self.write_one(buf) {
                Ok(0) => break,
                Ok(n) => {
                    buf = &buf[n..];
                    write_len += n;
                }
                Err(_) => return Err(()),
            }
        }
        Ok(write_len)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl fatfs::Seek for Partition {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let size = self.size();
        let new_pos = match pos {
            fatfs::SeekFrom::Start(pos) => Some(pos),
            fatfs::SeekFrom::Current(off) => self.position().checked_add_signed(off),
            fatfs::SeekFrom::End(off) => size.checked_add_signed(off),
        }
        .ok_or(())?;
        if new_pos > size {
            warn!("Seek beyond the end of the block device");
        }
        self.set_position(new_pos);
        Ok(new_pos)
    }
}

// Impl for VFS

fn as_vfs_err(_f: impl IoError) -> SysError {
    SysError::EINVAL
}

pub struct FatFileSystem {
    inner: fatfs::FileSystem<Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>,
    root: SyncUnsafeCell<MaybeUninit<VfsFileRef>>,
}

unsafe impl Sync for FatFileSystem {}
unsafe impl Send for FatFileSystem {}

impl FatFileSystem {
    pub fn new(parition: Partition) -> Self {
        let inner = fatfs::FileSystem::new(parition, fatfs::FsOptions::new())
            .expect("failed to initialize FAT filesystem");
        Self {
            inner,
            root: SyncUnsafeCell::new(MaybeUninit::uninit()),
        }
    }

    pub fn init(&'static self) {
        let root = self.inner.root_dir();
        let root = FatConcreteGenericFile::new_dir(root);
        // TODO: size & dev id
        let root = SyncAttrCacheFile::new_direct(
            root,
            VfsFileAttr {
                kind: VfsFileKind::Directory,
                device_id: 0,
                self_device_id: 0,
                byte_size: 6656,
                block_count: 13,
                access_time: 0,
                modify_time: 0,
                create_time: 0,
            },
        );
        let root = DEntryCacheDir::new_root(root);
        let root = VfsFileRef::new(root);
        unsafe { &mut *self.root.get() }.write(root);
    }
}

impl VfsFS for FatFileSystem {
    fn root(&self) -> VfsFileRef {
        unsafe { (*self.root.get()).assume_init_ref() }.clone()
    }
}

type FatFile = fatfs::File<'static, Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;
type FatDir = fatfs::Dir<'static, Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;
type FatDEntry =
    fatfs::DirEntry<'static, Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>;

pub enum FatConcreteGenericFile {
    File(SyncUnsafeCell<FatFile>),
    Dir(SyncUnsafeCell<FatDir>),
}

impl FatConcreteGenericFile {
    fn file(&self) -> &mut FatFile {
        match self {
            FatConcreteGenericFile::File(f) => unsafe { &mut *f.get() },
            _ => panic!("not a file"),
        }
    }

    fn dir(&self) -> &mut FatDir {
        match self {
            FatConcreteGenericFile::Dir(f) => unsafe { &mut *f.get() },
            _ => panic!("not a dir"),
        }
    }

    fn new_file(f: FatFile) -> Self {
        FatConcreteGenericFile::File(SyncUnsafeCell::new(f))
    }

    fn new_dir(f: FatDir) -> Self {
        FatConcreteGenericFile::Dir(SyncUnsafeCell::new(f))
    }
}

impl Clone for FatConcreteGenericFile {
    fn clone(&self) -> Self {
        use FatConcreteGenericFile::*;
        match self {
            File(_) => Self::new_file(self.file().clone()),
            Dir(_) => Self::new_dir(self.dir().clone()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct FatConcreteDirEntry(FatDEntry);

impl DEntryRef for FatConcreteDirEntry {
    type FileT = FatConcreteGenericFile;
    fn name(&self) -> String {
        self.0.file_name()
    }
    fn attr(&self) -> VfsFileAttr {
        let kind = if self.0.is_dir() {
            VfsFileKind::Directory
        } else {
            VfsFileKind::RegularFile
        };

        let byte_size = self.0.len() as usize;
        let block_count = byte_size / BLOCK_SIZE;

        VfsFileAttr {
            kind,
            device_id: 1,
            self_device_id: 0,
            byte_size,
            block_count,
            modify_time: 0,
            access_time: 0,
            create_time: 0,
        }
    }
    fn file(&self) -> Self::FileT {
        if self.0.is_dir() {
            FatConcreteGenericFile::new_dir(self.0.to_dir())
        } else {
            FatConcreteGenericFile::new_file(self.0.to_file())
        }
    }
}

unsafe impl Sync for FatConcreteDirEntry {}
unsafe impl Send for FatConcreteDirEntry {}
unsafe impl Sync for FatConcreteGenericFile {}
unsafe impl Send for FatConcreteGenericFile {}

impl ConcreteFile for FatConcreteGenericFile {
    type DEntryRefT = FatConcreteDirEntry;

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> ASysResult<usize> {
        let file = self.file();
        let sync_read_at = move || -> Result<usize, <FatFile as IoBase>::Error> {
            let mut read_len = 0;
            let mut file_end = false;
            file.seek(fatfs::SeekFrom::Start(offset as u64))?;
            // This for loop is needed since fatfs do not guarantee read the whole buffer, may only read a sector
            let mut buf = buf;
            while !buf.is_empty() && !file_end {
                let fat_read_len = file.read(buf)?;
                if fat_read_len == 0 {
                    file_end = true;
                }
                read_len += fat_read_len;
                buf = &mut buf[fat_read_len..];
            }
            Ok(read_len)
        };

        let result = sync_read_at().map_err(as_vfs_err);
        dyn_future(async move { result })
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> ASysResult<usize> {
        let file = self.file();

        // TODO: impl a read_at like write, not sure how long fatfs can write
        let mut sync_write_at = || -> Result<usize, <FatFile as IoBase>::Error> {
            file.seek(fatfs::SeekFrom::Start(offset as u64))?;
            file.write(buf)
        };

        let result = sync_write_at().map_err(as_vfs_err);
        dyn_future(async move { result })
    }

    fn lookup_batch(
        &self,
        skip_n: usize,
        _name: Option<&str>,
    ) -> ASysResult<(bool, Vec<Self::DEntryRefT>)> {
        let dir = self.dir();

        if skip_n != 0 {
            todo!("skip_n != 0 is not supported")
        }

        let sync_list = || -> Result<(bool, Vec<Self::DEntryRefT>), <FatFile as IoBase>::Error> {
            let mut v = Vec::new();
            for de in dir.iter() {
                v.push(FatConcreteDirEntry(de?))
            }
            Ok((true, v))
        };

        let result = sync_list().map_err(as_vfs_err);
        dyn_future(async move { result })
    }

    fn set_attr(&self, _dentry_ref: Self::DEntryRefT, _attr: VfsFileAttr) -> ASysResult {
        todo!("set_attr")
    }

    fn create(&self, name: &str, kind: VfsFileKind) -> ASysResult<Self::DEntryRefT> {
        let dir = self.dir();

        let sync_create = || -> Result<Self::DEntryRefT, <FatFile as IoBase>::Error> {
            match kind {
                VfsFileKind::RegularFile => {
                    dir.create_file(name)?;
                    let dentry =
                        dir.iter().map(Result::unwrap).find(|x| x.file_name() == name).unwrap();
                    Ok(FatConcreteDirEntry(dentry))
                }
                VfsFileKind::Directory => {
                    dir.create_dir(name)?;
                    let dentry =
                        dir.iter().map(Result::unwrap).find(|x| x.file_name() == name).unwrap();
                    Ok(FatConcreteDirEntry(dentry))
                }
                _ => unimplemented!(),
            }
        };

        let result = sync_create().map_err(as_vfs_err);
        dyn_future(async move { result })
    }

    fn remove(&self, dentry_ref: Self::DEntryRefT) -> ASysResult {
        let dir = self.dir();

        let sync_remove =
            || -> Result<(), <FatFile as IoBase>::Error> { dir.remove(&dentry_ref.name()) };

        let result = sync_remove().map_err(as_vfs_err);
        dyn_future(async move { result })
    }

    fn detach(&self, _dentry_ref: Self::DEntryRefT) -> ASysResult<Self> {
        todo!("detach")
    }
}
