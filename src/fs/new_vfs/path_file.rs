use super::{
    path::Path,
    top::{VfsFile, VfsFileRef},
};
use crate::{impl_vfs_forward_dir, impl_vfs_forward_file, tools::errors::SysResult};
use alloc::{
    string::ToString,
    sync::{Arc, Weak},
};

pub struct VfsPathFile(Arc<VfsPathFileInner>);

struct VfsPathFileInner {
    path: Path,
    file: VfsFileRef,
    parent: Option<Weak<VfsPathFileInner>>,
}

impl VfsPathFile {
    fn new(path: Path, file: VfsFileRef, parent: Option<Weak<VfsPathFileInner>>) -> Self {
        Self(Arc::new(VfsPathFileInner { path, file, parent }))
    }

    pub fn new_root(file: VfsFileRef) -> Self {
        Self::new(Path::from(""), file, None)
    }

    fn new_sub(&self, name: &str, file: VfsFileRef) -> Self {
        let mut path = self.0.path.clone();
        path.push_back(name.to_string());
        Self::new(path, file, Some(Arc::downgrade(&self.0)))
    }

    pub fn path(&self) -> &Path {
        &self.0.path
    }
    fn file(&self) -> &VfsFileRef {
        &self.0.file
    }
    fn parent_inner(&self) -> Option<&Weak<VfsPathFileInner>> {
        self.0.parent.as_ref()
    }

    pub fn parent(&self) -> Option<Self> {
        self.parent_inner().map(|p| Self(Arc::clone(&p.upgrade().unwrap())))
    }
    pub async fn lookup_sub(&self, name: &str) -> SysResult<Self> {
        let file = self.file().lookup(name).await?;
        Ok(self.new_sub(name, file))
    }
}

impl VfsFile for VfsPathFile {
    fn attr(&self) -> crate::tools::errors::ASysResult<super::VfsFileAttr> {
        self.file().attr()
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    impl_vfs_forward_dir!(file());
    impl_vfs_forward_file!(file());
}
