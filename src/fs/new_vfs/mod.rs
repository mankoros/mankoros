pub mod top;
pub mod underlying;
pub mod path;
pub mod path_file;
pub mod page_cache;
pub mod dentry_cache;
pub mod sync_attr_cache;

type DeviceID = usize;

#[derive(Clone, Debug)]
pub struct VfsFileAttr {
    pub kind: VfsFileKind,
    /// 文件所属于的文件系统的设备 ID,
    /// 只有同一个文件系统内的文件, 这个 ID 才相同
    pub device_id: DeviceID,
    /// 文件大小 (单位为字节)
    pub byte_size: usize,  
    /// 文件占用的块数量
    pub block_count: usize,
    /// 文件最近一次访问时间
    pub access_time: usize, 
    /// 文件最近一次修改时间
    pub modify_time: usize, 
    /// 文件被创造的时间
    pub create_time: usize, 
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsFileKind {
    Unknown,
    Pipe,  
    CharDevice,   
    Directory,   
    BlockDevice,   
    RegularFile,  
    SymbolLink,  
    SocketFile, 
}