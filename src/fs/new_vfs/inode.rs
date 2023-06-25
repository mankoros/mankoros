//! VFS 第二层中的文件相关部分

use core::ops::Deref;
use crate::{sync::{SleepLock}, tools::errors::{ASysResult, SysResult}, memory::address::PhysAddr};
use alloc::{boxed::Box, vec::Vec, string::String};
use super::{underlying::FsNode, info::NodeStat};

/// 代表一个可读可写的文件, 同时具有页缓存和映射页管理的功能
pub struct VfsNode {
    /// 页缓存与页映射管理器, 同时也负责脏页管理
    mapped_page_mgr: MappedPageManager,
    /// 底层文件系统中的文件
    pub(super) fs_node: SleepLock<Box<dyn FsNode>>,
}

/// 管理文件映射页的结构体
struct MappedPageManager {
    // TODO
}

impl MappedPageManager {
    pub fn new() -> Self {
        Self {
            // TODO
        }
    }
}

impl VfsNode {
    pub(super)  fn new(fs_node: Box<dyn FsNode>) -> Self {
        Self {
            mapped_page_mgr: MappedPageManager::new(),
            fs_node: SleepLock::new(fs_node),
        }
    }

    pub async fn stat(&self) -> SysResult<NodeStat> {
        self.fs_node.lock().await.stat().await
    }
    pub async fn read_at(&self, offset: usize, buf: &mut [u8]) -> SysResult<usize> {
        self.fs_node.lock().await.read_at(offset, buf).await
    }
    pub async fn write_at(&self, offset: usize, buf: &[u8]) -> SysResult<usize> {
        self.fs_node.lock().await.write_at(offset, buf).await
    }

    // TODO: map & unmap

    pub async fn list(&self) -> SysResult<Vec<String>> {
        self.fs_node.lock().await.list().await
    }

    pub async fn lookup(&self, name: &str) -> SysResult<VfsNode> {
        let fs_node = self.fs_node.lock().await.lookup(name).await?;
        Ok(VfsNode::new(fs_node))
    }

    pub async fn create(&self, name: &str, is_dir: bool) -> SysResult<VfsNode> {
        let fs_node = self.fs_node.lock().await.create(name, is_dir).await?;
        Ok(VfsNode::new(fs_node))
    }
}