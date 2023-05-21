use core::cell::UnsafeCell;

use crate::impl_vfs_dir_default;
use crate::sync::SpinNoIrqLock;
use crate::{here, impl_vfs_non_dir_default, sync};

use super::disk::BLOCK_SIZE;
use super::vfs::filesystem::*;
use super::vfs::node::*;
use super::vfs::*;
use super::{
    partition::Partition,
    vfs::{self, filesystem::VfsNode},
};
use alloc::boxed::Box;
use alloc::sync::Arc;
use fatfs::{self, Read, Seek, Write};
use log::{trace, warn};

/// fatfs trait for vfs wrapper
pub struct FatVfsWrapper {
    offset: u64,
    file: Arc<dyn VfsNode>,
}

impl fatfs::IoBase for FatVfsWrapper {
    type Error = ();
}

impl fatfs::Read for FatVfsWrapper {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        let read_len = self.file.sync_read_at(self.offset, buf).expect("VfsWrapper read error");
        self.offset += read_len as u64;
        Ok(read_len)
    }
}

impl fatfs::Write for FatVfsWrapper {
    fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        let write_len = self.file.sync_write_at(self.offset, buf).expect("VfsWrapper write error");
        self.offset += write_len as u64;
        Ok(write_len)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.file.fsync();
        Ok(())
    }
}

impl fatfs::Seek for FatVfsWrapper {
    fn seek(&mut self, pos: fatfs::SeekFrom) -> Result<u64, Self::Error> {
        let size = self.file.stat().expect("VfsWrapper stat error").size();
        let new_pos = match pos {
            fatfs::SeekFrom::Start(pos) => Some(pos),
            fatfs::SeekFrom::Current(off) => self.offset.checked_add_signed(off),
            fatfs::SeekFrom::End(off) => size.checked_add_signed(off),
        }
        .ok_or(())?;
        if new_pos > size {
            warn!("Seek beyond the end of the block device");
        }
        self.offset = new_pos;
        Ok(new_pos)
    }
}

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

const fn as_vfs_err(err: fatfs::Error<()>) -> VfsError {
    use fatfs::Error::*;
    match err {
        AlreadyExists => VfsError::AlreadyExists,
        CorruptedFileSystem => VfsError::InvalidData,
        DirectoryIsNotEmpty => VfsError::DirectoryNotEmpty,
        InvalidInput | InvalidFileNameLength | UnsupportedFileNameCharacter => {
            VfsError::InvalidInput
        }
        NotEnoughSpace => VfsError::StorageFull,
        NotFound => VfsError::NotFound,
        UnexpectedEof => VfsError::UnexpectedEof,
        WriteZero => VfsError::WriteZero,
        Io(_) => VfsError::Io,
        _ => VfsError::Io,
    }
}

pub struct FatFileSystem {
    inner: fatfs::FileSystem<Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>,
    root_dir: UnsafeCell<Option<vfs::filesystem::VfsNodeRef>>,
}

pub struct FileWrapper<'a>(
    sync::SpinNoIrqLock<
        fatfs::File<'a, Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>,
    >,
);
pub struct DirWrapper<'a>(
    fatfs::Dir<'a, Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>,
);

unsafe impl Sync for FatFileSystem {}
unsafe impl Send for FatFileSystem {}
unsafe impl<'a> Send for FileWrapper<'a> {}
unsafe impl<'a> Sync for FileWrapper<'a> {}
unsafe impl<'a> Send for DirWrapper<'a> {}
unsafe impl<'a> Sync for DirWrapper<'a> {}

impl FatFileSystem {
    pub fn new(parition: Partition) -> Self {
        let inner = fatfs::FileSystem::new(parition, fatfs::FsOptions::new())
            .expect("failed to initialize FAT filesystem");
        Self {
            inner,
            root_dir: UnsafeCell::new(None),
        }
    }

    pub fn init(&'static self) {
        // must be called before later operations
        unsafe { *self.root_dir.get() = Some(Self::new_dir(self.inner.root_dir())) }
    }

    fn new_file(
        file: fatfs::File<'_, Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>,
    ) -> Arc<FileWrapper> {
        Arc::new(FileWrapper(SpinNoIrqLock::new(file)))
    }

    fn new_dir(
        dir: fatfs::Dir<'_, Partition, fatfs::NullTimeProvider, fatfs::LossyOemCpConverter>,
    ) -> Arc<DirWrapper> {
        Arc::new(DirWrapper(dir))
    }
}

impl Vfs for FatFileSystem {
    fn root_dir(&self) -> VfsNodeRef {
        let root_dir = unsafe { (*self.root_dir.get()).as_ref().unwrap() };
        root_dir.clone()
    }
}

impl VfsNode for FileWrapper<'static> {
    impl_vfs_non_dir_default! {}

    fn stat(&self) -> VfsResult<VfsNodeAttr> {
        let size = self.0.lock(here!()).seek(fatfs::SeekFrom::End(0)).map_err(as_vfs_err)?;
        let blocks = (size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64;
        // FAT fs doesn't support permissions, we just set everything to 755
        let perm = VfsNodePermission::from_bits_truncate(0o755);
        Ok(VfsNodeAttr::new(perm, VfsNodeType::File, size, blocks))
    }

    fn sync_read_at(&self, offset: u64, mut buf: &mut [u8]) -> VfsResult<usize> {
        let mut file = self.0.lock(here!());
        let mut read_len = 0;
        let mut file_end = false;
        file.seek(fatfs::SeekFrom::Start(offset)).map_err(as_vfs_err)?;
        // This for loop is needed since fatfs do not guarantee read the whole buffer, may only read a sector
        while buf.len() != 0 && !file_end {
            let fat_read_len = file.read(buf).map_err(as_vfs_err)?;
            if fat_read_len == 0 {
                file_end = true;
            }
            read_len += fat_read_len;
            buf = &mut buf[fat_read_len..];
        }
        Ok(read_len)
    }

    fn sync_write_at(&self, offset: u64, buf: &[u8]) -> VfsResult<usize> {
        let mut file = self.0.lock(here!());
        // TODO: impl a read_at like write, not sure how long fatfs can write
        file.seek(fatfs::SeekFrom::Start(offset)).map_err(as_vfs_err)?;
        file.write(buf).map_err(as_vfs_err)
    }

    fn read_at<'a>(&'a self, offset: u64, buf: &'a mut [u8]) -> AVfsResult<usize> {
        Box::pin(async move { self.sync_read_at(offset, buf) })
    }

    fn write_at<'a>(&'a self, offset: u64, buf: &'a [u8]) -> AVfsResult<usize> {
        Box::pin(async move { self.sync_write_at(offset, buf) })
    }

    fn truncate(&self, size: u64) -> VfsResult {
        let mut file = self.0.lock(here!());
        file.seek(fatfs::SeekFrom::Start(size)).map_err(as_vfs_err)?; // TODO: more efficient
        file.truncate().map_err(as_vfs_err)
    }
}

impl VfsNode for DirWrapper<'static> {
    impl_vfs_dir_default! {}

    fn stat(&self) -> VfsResult<VfsNodeAttr> {
        // FAT fs doesn't support permissions, we just set everything to 755
        Ok(VfsNodeAttr::new(
            VfsNodePermission::from_bits_truncate(0o755),
            VfsNodeType::Dir,
            BLOCK_SIZE as u64,
            1,
        ))
    }

    fn parent(&self) -> Option<VfsNodeRef> {
        self.0.open_dir("..").map_or(None, |dir| Some(FatFileSystem::new_dir(dir)))
    }

    fn lookup(self: Arc<Self>, path: &str) -> VfsResult<VfsNodeRef> {
        trace!("lookup at fatfs: {}", path);
        let path = path.trim_matches('/');
        if path.is_empty() || path == "." {
            return Ok(self.clone());
        }
        if let Some(rest) = path.strip_prefix("./") {
            return self.lookup(rest);
        }

        // TODO: use `fatfs::Dir::find_entry`, but it's not public.
        if let Ok(file) = self.0.open_file(path) {
            Ok(FatFileSystem::new_file(file))
        } else if let Ok(dir) = self.0.open_dir(path) {
            Ok(FatFileSystem::new_dir(dir))
        } else {
            Err(VfsError::NotFound)
        }
    }

    fn create(&self, path: &str, ty: VfsNodeType) -> VfsResult {
        trace!("create {:?} at fatfs: {}", ty, path);
        let path = path.trim_matches('/');
        if path.is_empty() || path == "." {
            return Ok(());
        }
        if let Some(rest) = path.strip_prefix("./") {
            return self.create(rest, ty);
        }

        match ty {
            VfsNodeType::File => {
                self.0.create_file(path).map_err(as_vfs_err)?;
                Ok(())
            }
            VfsNodeType::Dir => {
                self.0.create_dir(path).map_err(as_vfs_err)?;
                Ok(())
            }
            _ => Err(VfsError::Unsupported),
        }
    }

    fn remove(&self, path: &str) -> VfsResult {
        trace!("remove at fatfs: {}", path);
        let path = path.trim_matches('/');
        assert!(!path.is_empty()); // already check at `root.rs`
        if let Some(rest) = path.strip_prefix("./") {
            return self.remove(rest);
        }
        self.0.remove(path).map_err(as_vfs_err)
    }

    fn read_dir(&self, start_idx: usize, dirents: &mut [VfsDirEntry]) -> VfsResult<usize> {
        let mut iter = self.0.iter().skip(start_idx);
        for (i, out_entry) in dirents.iter_mut().enumerate() {
            let x = iter.next();
            match x {
                Some(Ok(entry)) => {
                    let ty = if entry.is_dir() {
                        VfsNodeType::Dir
                    } else if entry.is_file() {
                        VfsNodeType::File
                    } else {
                        unreachable!()
                    };
                    *out_entry = VfsDirEntry::new(&entry.file_name(), ty);
                }
                _ => return Ok(i),
            }
        }
        Ok(dirents.len())
    }
}
