// pub mod dentry_cache;
pub mod mount;
pub mod page_cache;
pub mod path;
pub mod path_cache;
pub mod path_file;
pub mod sync_attr_file;
pub mod top;
pub mod underlying;

type DeviceID = usize;

#[derive(Clone, Debug)]
pub struct VfsFileAttr {
    pub kind: VfsFileKind,
    /// 文件所属于的文件系统的设备 ID,
    /// 只有同一个文件系统内的文件, 这个 ID 才相同
    pub device_id: DeviceID,
    /// 如果文件为块设备, 则这个字段表示该文件本身的设备 ID
    pub self_device_id: DeviceID,
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

impl From<VfsFileKind> for u32 {
    fn from(val: VfsFileKind) -> Self {
        // low 9 bits are for permissions, 12 bits are for file type
        match val {
            VfsFileKind::Unknown => 0,
            VfsFileKind::Pipe => 0o10000,
            VfsFileKind::CharDevice => 0o20000,
            VfsFileKind::Directory => 0o40000,
            VfsFileKind::BlockDevice => 0o60000,
            VfsFileKind::RegularFile => 0o100000,
            VfsFileKind::SymbolLink => 0o120000,
            VfsFileKind::SocketFile => 0o140000,
        }
    }
}

pub struct DeviceIDCollection;
impl DeviceIDCollection {
    pub const TMP_FS_ID: DeviceID = 0;
    pub const DEV_FS_ID: DeviceID = 1;
    pub const PIPE_FS_ID: DeviceID = 2;
    pub const STDIN_FS_ID: DeviceID = 3;
    pub const STDOUT_FS_ID: DeviceID = 4;
    pub const STDERR_FS_ID: DeviceID = 5;

    pub const CONCERTE_FS_ID_BEG: DeviceID = 256;
}
