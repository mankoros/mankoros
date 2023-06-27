use alloc::collections::BTreeMap;
use alloc::sync::{Arc, Weak};
use spin::RwLock;
use crate::process::lproc::FsInfo;
use crate::fs::new_vfs::underlying::FsNode;
use crate::tools::errors::{ASysResult, dyn_future};
use crate::fs::new_vfs::info::NodeStat;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::boxed::Box;
use crate::fs::new_vfs::inode::VfsNode;
use crate::fs::new_vfs::top::VfsFile;

pub struct InMemoryDir {}

// 这个 Node 不需要存任何信息, 元信息和目录信息都会在缓存层中被维护
// 它只需要在第一次查询元信息/目录时返回一个空的即可, 因为是 in-memory 的, 
// 写回之后也不需要保存
impl FsNode for InMemoryDir {
    fn stat(&self) -> ASysResult<NodeStat> {
        dyn_future(async { NodeStat::default_dir(0) })
    }

    fn read_at(&self, _offset: usize, _buf: &mut [u8]) -> ASysResult<usize> {
        unimplemented!("dir")
    }

    fn write_at(&self, _offset: usize, _buf: &[u8]) -> ASysResult<usize> {
        unimplemented!("dir")
    }

    fn list(&self) -> ASysResult<Vec<(String, Box<dyn FsNode>)>> {
        Vec::new()
    }

    fn lookup(&self, name: &str) -> ASysResult<Box<dyn FsNode>> {
        // do nothing
    }

    fn create(&self, name: &str, is_dir: bool) -> ASysResult<Box<dyn FsNode>> {
        Self::new()
    }

    fn link(&self, name: &str, node: &dyn FsNode) -> ASysResult {
        // do nothing
    }

    fn unlink(&self, name: &str) -> ASysResult {
        // do nothing
    }
}

fn split_path(path: &str) -> (&str, Option<&str>) {
    let trimmed_path = path.trim_start_matches('/');
    trimmed_path.find('/').map_or((trimmed_path, None), |n| {
        (&trimmed_path[..n], Some(&trimmed_path[n + 1..]))
    })
}
