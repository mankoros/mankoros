//! 各类文件信息

use bitflags::bitflags;

#[derive(Clone)]
pub struct NodeStat {
    /// 文件所在设备的 ID
    pub device_id: u64,   
    /// 文件 INode 节点号
    pub inode_number: u64,   
    /// 文件类型和访问权限
    pub type_and_mode: u32,  
    /// 硬链接数
    pub link_count: u32, 
    /// 所有者 ID
    pub user_id: u32,   
    /// 所有组 ID
    pub group_id: u32,   
    /// 设备文件所代表的设备的设备号
    pub self_device_id: u64,  
    /// 文件大小 (单位为字节)
    pub byte_size: usize,  
    /// 文件最近一次访问时间
    pub access_time: usize, 
    /// 文件最近一次修改时间
    pub modify_time: usize, 
    /// 文件被创造的时间
    pub create_time: usize, 
}

impl NodeStat {
    pub fn kind(&self) -> NodeType {
        NodeType::from_bits((self.type_and_mode >> 12) as u8)
    }

    pub fn perm(&self) -> NodePermission {
        NodePermission::from_bits((self.type_and_mode & 0o7777) as u16).unwrap()
    }
    pub fn device_id(&self) -> u64 {
        self.device_id
    }
    pub fn inode_number(&self) -> u64 {
        self.inode_number
    }
    pub fn link_count(&self) -> u32 {
        self.link_count
    }
    pub fn user_id(&self) -> u32 {
        self.user_id
    }
    pub fn group_id(&self) -> u32 {
        self.group_id
    }
    pub fn self_device_id(&self) -> u64 {
        debug_assert!(self.kind().is_device());
        self.self_device_id
    }
    pub fn byte_size(&self) -> usize {
        self.byte_size
    }
    pub fn access_time(&self) -> usize {
        self.access_time
    }
    pub fn modify_time(&self) -> usize {
        self.modify_time
    }
    pub fn create_time(&self) -> usize {
        self.create_time
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeType {
    Unknown,
    FIFO,  
    CharDevice,   
    Directory,   
    BlockDevice,   
    RegularFile,  
    SymbolLink,  
    SocketFile, 
}

impl NodeType {
    pub fn to_bits(&self) -> u8 {
        match self {
            NodeType::Unknown       => 0o000,
            NodeType::FIFO          => 0o001,
            NodeType::CharDevice    => 0o002,
            NodeType::Directory     => 0o004,
            NodeType::BlockDevice   => 0o006,
            NodeType::RegularFile   => 0o010,
            NodeType::SymbolLink    => 0o012,
            NodeType::SocketFile    => 0o014,
        }
    }

    pub fn from_bits(bits: u8) -> Self {
        match bits {
            0o001 => NodeType::FIFO,
            0o002 => NodeType::CharDevice,
            0o004 => NodeType::Directory,
            0o006 => NodeType::BlockDevice,
            0o010 => NodeType::RegularFile,
            0o012 => NodeType::SymbolLink,
            0o014 => NodeType::SocketFile,
            _ => NodeType::Unknown,
        }
    }

    pub fn is_fifo(self) -> bool {
        self == NodeType::FIFO
    }
    pub fn is_pipe(self) -> bool {
        self == NodeType::FIFO
    }
    pub fn is_device(self) -> bool {
        self == NodeType::CharDevice || self == NodeType::BlockDevice
    }
    pub fn is_dir(self) -> bool {
        self == NodeType::Directory
    }
    pub fn is_file(self) -> bool {
        self == NodeType::RegularFile
    }
    pub fn is_symlink(self) -> bool {
        self == NodeType::SymbolLink
    }
    pub fn is_socket(self) -> bool {
        self == NodeType::SocketFile
    }
}

bitflags! {
    pub struct NodePermission: u16 {
        const SET_UID       = 0o4000;
        const SET_GID       = 0o2000;
        const SET_STICKY    = 0o1000;
        const OWNER_READ    = 0o0400;
        const OWNER_WRITE   = 0o0200;
        const OWNER_EXEC    = 0o0100;
        const GROUP_READ    = 0o0040;
        const GROUP_WRITE   = 0o0020;
        const GROUP_EXEC    = 0o0010;
        const OTHER_READ    = 0o0004;
        const OTHER_WRITE   = 0o0002;
        const OTHER_EXEC    = 0o0001;
        const OTHER_ALL     = 0o0007;
    }
}

impl NodePermission {
    #[inline(always)]
    pub fn owner_ro(&self) -> bool {
        self.contains(NodePermission::OWNER_READ)
    }
    #[inline(always)]
    pub fn owner_rw(&self) -> bool {
        self.contains(NodePermission::OWNER_READ) && self.contains(NodePermission::OWNER_WRITE)
    }
    #[inline(always)]
    pub fn owner_rx(&self) -> bool {
        self.contains(NodePermission::OWNER_READ) && self.contains(NodePermission::OWNER_EXEC)
    }
    #[inline(always)]
    pub fn owner_rwx(&self) -> bool {
        self.contains(NodePermission::OWNER_READ)
            && self.contains(NodePermission::OWNER_WRITE)
            && self.contains(NodePermission::OWNER_EXEC)
    }

    #[inline(always)]
    pub fn group_ro(&self) -> bool {
        self.contains(NodePermission::GROUP_READ)
    }
    #[inline(always)]
    pub fn group_rw(&self) -> bool {
        self.contains(NodePermission::GROUP_READ) && self.contains(NodePermission::GROUP_WRITE)
    }
    #[inline(always)]
    pub fn group_rx(&self) -> bool {
        self.contains(NodePermission::GROUP_READ) && self.contains(NodePermission::GROUP_EXEC)
    }
    #[inline(always)]
    pub fn group_rwx(&self) -> bool {
        self.contains(NodePermission::GROUP_READ)
            && self.contains(NodePermission::GROUP_WRITE)
            && self.contains(NodePermission::GROUP_EXEC)
    }

    #[inline(always)]
    pub fn other_ro(&self) -> bool {
        self.contains(NodePermission::OTHER_READ)
    }
    #[inline(always)]
    pub fn other_rw(&self) -> bool {
        self.contains(NodePermission::OTHER_READ) && self.contains(NodePermission::OTHER_WRITE)
    }
    #[inline(always)]
    pub fn other_rx(&self) -> bool {
        self.contains(NodePermission::OTHER_READ) && self.contains(NodePermission::OTHER_EXEC)
    }
    #[inline(always)]
    pub fn other_rwx(&self) -> bool {
        self.contains(NodePermission::OTHER_READ)
            && self.contains(NodePermission::OTHER_WRITE)
            && self.contains(NodePermission::OTHER_EXEC)
    }

    #[inline(always)]
    pub fn all_rwx(&self) -> bool {
        self.owner_rwx() && self.group_rwx() && self.other_rwx()
    }
}
