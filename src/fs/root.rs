//! Root directory of the filesystem
//!
//! Adapted from ArceOS
//! Copyright (C) 2023 by ArceOS
//! Copyright (C) 2023 by MankorOS

use alloc::string::String;
use alloc::{sync::Arc, vec::Vec};
use log::debug;

use crate::lazy_init::LazyInit;
use crate::{axerrno::AxResult, impl_vfs_dir_default};

use super::fatfs::FatFileSystem;
use super::partition::Partition;
use super::vfs::filesystem::*;
use super::vfs::node::*;
use super::vfs::*;

static ROOT_DIR: LazyInit<Arc<RootDirectory>> = LazyInit::new();

/// Maintain a path <-> fs relationship
struct MountPoint {
    path: String,
    fs: Arc<dyn Vfs>,
}

pub struct RootDirectory {
    main_fs: Arc<dyn Vfs>,
    mounts: Vec<MountPoint>,
}

impl MountPoint {
    pub fn new(path: String, fs: Arc<dyn Vfs>) -> Self {
        Self { path, fs }
    }
}

impl Drop for MountPoint {
    fn drop(&mut self) {
        self.fs.unmount().ok();
    }
}

impl RootDirectory {
    pub const fn new(main_fs: Arc<dyn Vfs>) -> Self {
        Self {
            main_fs,
            mounts: Vec::new(),
        }
    }

    pub fn mount(&mut self, path: String, fs: Arc<dyn Vfs>) -> AxResult {
        if path == "/" {
            return crate::ax_err!(InvalidInput, "cannot mount root filesystem");
        }
        if !path.starts_with('/') {
            return crate::ax_err!(InvalidInput, "mount path must start with '/'");
        }
        if self.mounts.iter().any(|mp| mp.path == path) {
            return crate::ax_err!(InvalidInput, "mount point already exists");
        }
        // create the mount point in the main filesystem if it does not exist
        self.main_fs.root_dir().create(&path, VfsNodeType::Dir)?;
        fs.mount(&path, self.main_fs.root_dir().lookup(&path)?)?;
        self.mounts.push(MountPoint::new(path, fs));
        Ok(())
    }

    pub fn umount(&mut self, path: &str) {
        self.mounts.retain(|mp| mp.path != path);
    }

    pub fn contains(&self, path: &str) -> bool {
        self.mounts.iter().any(|mp| mp.path == path)
    }

    fn lookup_mounted_fs<F, T>(&self, path: &str, f: F) -> AxResult<T>
    where
        F: FnOnce(Arc<dyn Vfs>, &str) -> AxResult<T>,
    {
        debug!("lookup at root: {}", path);
        let path = path.trim_matches('/');
        if let Some(rest) = path.strip_prefix("./") {
            return self.lookup_mounted_fs(rest, f);
        }

        let mut idx = 0;
        let mut max_len = 0;

        // Find the filesystem that has the longest mounted path match
        // TODO: more efficient, e.g. trie
        for (i, mp) in self.mounts.iter().enumerate() {
            // skip the first '/'
            if path.starts_with(&mp.path[1..]) && mp.path.len() - 1 > max_len {
                max_len = mp.path.len() - 1;
                idx = i;
            }
        }

        if max_len == 0 {
            f(self.main_fs.clone(), path) // not matched any mount point
        } else {
            f(self.mounts[idx].fs.clone(), &path[max_len..]) // matched at `idx`
        }
    }
}

impl VfsNode for RootDirectory {
    impl_vfs_dir_default! {}

    fn stat(&self) -> VfsResult<VfsNodeAttr> {
        self.main_fs.root_dir().stat()
    }

    fn lookup(self: Arc<Self>, path: &str) -> VfsResult<VfsNodeRef> {
        self.lookup_mounted_fs(path, |fs, rest_path| fs.root_dir().lookup(rest_path))
    }

    fn create(&self, path: &str, ty: VfsNodeType) -> VfsResult {
        self.lookup_mounted_fs(path, |fs, rest_path| {
            if rest_path.is_empty() {
                Ok(()) // already exists
            } else {
                fs.root_dir().create(rest_path, ty)
            }
        })
    }

    fn remove(&self, path: &str) -> VfsResult {
        self.lookup_mounted_fs(path, |fs, rest_path| {
            if rest_path.is_empty() {
                crate::ax_err!(PermissionDenied) // cannot remove mount points
            } else {
                fs.root_dir().remove(rest_path)
            }
        })
    }
}

pub fn get_root_dir() -> Arc<RootDirectory> {
    ROOT_DIR.clone()
}

pub fn init_rootfs(part: Partition) {
    static FAT_FS: LazyInit<Arc<FatFileSystem>> = LazyInit::new();
    FAT_FS.init_by(Arc::new(FatFileSystem::new(part)));
    FAT_FS.init();
    let main_fs = FAT_FS.clone();

    let root_dir = RootDirectory::new(main_fs);
    ROOT_DIR.init_by(Arc::new(root_dir));
}
