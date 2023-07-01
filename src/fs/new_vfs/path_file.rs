use super::{top::{VfsFile, VfsFileRef}, path::Path};

pub struct VfsPathFile {
    path: Path,
    file: VfsFileRef
}

impl VfsPathFile {
    pub fn parent(&self) -> VfsPathFile {
        unimplemented!()
    }
}

// impl VfsFile for VfsPathFile {
// }