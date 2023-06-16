//! VFS 中对接底层文件系统的层次
//! 所有底层文件系统都需要一些实现了这些接口的结构体以供 VFS 使用

use crate::{tools::errors::{ASysResult}};
use alloc::{boxed::Box, vec::Vec, string::String};
use super::info::NodeStat;

pub trait FsFileSystem {
    fn root(&self) -> Box<dyn FsNode>;
}

/// 抽象的, 代表具体文件系统中一个文件应该能做什么的 trait
/// 每个具体文件系统 (fat32/devfs/...) 中的文件都应该实现这个 trait
/// "文件夹" 指的是底层 FS 应该有的一类特殊文件, 它至少能支持该 trait 中的所有文件夹操作
pub trait FsNode {
    // 通用文件操作
    /// 获取文件的各类属性,
    /// 例如文件类型, 文件大小, 文件创建时间等等
    fn stat(&self) -> ASysResult<NodeStat>;
    /// 读取文件内容
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> ASysResult<usize>;
    /// 写入文件内容
    fn write_at(&self, offset: usize, buf: &[u8]) -> ASysResult<usize>;

    // 文件夹操作
    /// 列出文件夹中的所有文件的名字
    fn list(&self) -> ASysResult<Vec<String>>;
    /// 根据名字查找文件夹中的文件, 不会递归查找
    /// 下层 FS 可以直接新建新的一个 dyn FsNode 返回, 不需要保证唯一性
    /// 上层 VFS 会确保对同一个文件只存在一个 dyn FsNode
    fn lookup(&self, name: &str) -> ASysResult<Box<dyn FsNode>>;
    /// 新建一个文件, 并在当前文件夹中创建一个 名字->新建文件 的映射
    /// 下层 FS 可以直接新建新的一个 dyn FsNode 返回, 不需要保证唯一性
    /// 上层 VFS 会确保对同一个文件只存在一个 dyn FsNode
    fn create(&self, name: &str, is_dir: bool) -> ASysResult<Box<dyn FsNode>>;
    /// 在当前文件夹创建一个 名字->文件 的映射
    fn link(&self, name: &str, node: &dyn FsNode) -> ASysResult;
    /// 在当前文件夹删除一个 名字->文件 的映射
    fn unlink(&self, name: &str) -> ASysResult;
}
