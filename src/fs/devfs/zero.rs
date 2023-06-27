use crate::{fs::new_vfs::{underlying::FsNode, info::NodeStat}, tools::errors::ASysResult, impl_default_non_dir};

pub struct ZeroDev {}

impl ZeroDev {
    pub fn new() -> Self {
        Self {}
    }
}

impl FsNode for ZeroDev {
    fn stat(&self) -> ASysResult<NodeStat> {
        NodeStat::default_file(0)
    }

    fn read_at(&self, offset: usize, buf: &mut [u8]) -> ASysResult<usize> {
        for i in buf.iter_mut() {
            *i = 0;
        }
        Ok(buf.len())
    }

    fn write_at(&self, offset: usize, buf: &[u8]) -> ASysResult<usize> {
        Ok(buf.len())
    }

    impl_default_non_dir!(ZeroDev);
}