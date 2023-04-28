use log::warn;
pub struct VfsNodeAttr {
    /// File permission
    mode: VfsNodePermission,
    /// File type
    type_: VfsNodeType,
    /// Total size
    size: u64,
    /// Number of blocks
    blocks: u64,
}

bitflags::bitflags! {
    /// permission mode
    pub struct VfsNodePermission: u16 {
        /// Owner can read
        const OWNER_READ = 0o400;
        /// Owner can write
        const OWNER_WRITE = 0o200;
        /// Owner can execute
        const OWNER_EXEC = 0o100;
        /// Group can read
        const GROUP_READ = 0o040;
        /// Group can write
        const GROUP_WRITE = 0o020;
        /// Group can execute
        const GROUP_EXEC = 0o010;
        /// Others can read
        const OTHER_READ = 0o004;
        /// Others can write
        const OTHER_WRITE = 0o002;
        /// Others can execute
        const OTHER_EXEC = 0o001;
    }
}

/// Node type
/// Includes file, directory, etc.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsNodeType {
    /// Unknown
    Unknown = 0,
    /// FIFO
    FIFO = 1,
    /// Character device
    CharDevice = 2,
    /// Directory
    Dir = 4,
    /// Block device
    BlockDevice = 6,
    /// Regular file
    File = 8,
    /// Symbolic link
    SymLink = 10,
    /// Socket
    Socket = 12,
}

/// Directory entry
#[derive(Debug, Clone)]
pub struct VfsDirEntry {
    d_type: VfsNodeType,
    d_name: [u8; 63],
}

impl VfsNodePermission {
    /// Convert to rwx format
    pub const fn rwx_buf(&self) -> [u8; 9] {
        let mut perm = [b'-'; 9];
        if self.contains(VfsNodePermission::OWNER_READ) {
            perm[0] = b'r';
        }
        if self.contains(VfsNodePermission::OWNER_WRITE) {
            perm[1] = b'w';
        }
        if self.contains(VfsNodePermission::OWNER_EXEC) {
            perm[2] = b'x';
        }
        if self.contains(VfsNodePermission::GROUP_READ) {
            perm[3] = b'r';
        }
        if self.contains(VfsNodePermission::GROUP_WRITE) {
            perm[4] = b'w';
        }
        if self.contains(VfsNodePermission::GROUP_EXEC) {
            perm[5] = b'x';
        }
        if self.contains(VfsNodePermission::OTHER_READ) {
            perm[6] = b'r';
        }
        if self.contains(VfsNodePermission::OTHER_WRITE) {
            perm[7] = b'w';
        }
        if self.contains(VfsNodePermission::OTHER_EXEC) {
            perm[8] = b'x';
        }
        perm
    }

    /// return default permission for a file.
    /// 644
    pub const fn default_file() -> Self {
        Self::from_bits_truncate(0o644)
    }

    /// return default permission for a directory.
    /// 755
    pub const fn default_dir() -> Self {
        Self::from_bits_truncate(0o755)
    }
}

impl VfsNodeType {
    /// return a char represent the type
    pub const fn as_char(self) -> char {
        match self {
            VfsNodeType::Unknown => '?',
            VfsNodeType::FIFO => 'p',
            VfsNodeType::CharDevice => 'c',
            VfsNodeType::Dir => 'd',
            VfsNodeType::BlockDevice => 'b',
            VfsNodeType::File => '-',
            VfsNodeType::SymLink => 'l',
            VfsNodeType::Socket => 's',
        }
    }
}

impl VfsNodeAttr {
    /// Create a new VfsNodeAttr
    pub const fn new(mode: VfsNodePermission, type_: VfsNodeType, size: u64, blocks: u64) -> Self {
        Self {
            mode,
            type_,
            size,
            blocks,
        }
    }

    /// Get file permission
    pub const fn mode(&self) -> VfsNodePermission {
        self.mode
    }

    /// Get file type
    pub const fn type_(&self) -> VfsNodeType {
        self.type_
    }

    /// Get total size
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// Get number of blocks
    pub const fn blocks(&self) -> u64 {
        self.blocks
    }
}

impl VfsDirEntry {
    /// Create a new VfsDirEntry
    pub fn new(name: &str, type_: VfsNodeType) -> Self {
        let mut d_name = [0; 63];
        if name.len() > 63 {
            warn!("directory entry name too long: {} > {}", name.len(), 63);
            todo!();
            // probably return a Result<Self, Error>
        }
        d_name[..name.len()].copy_from_slice(name.as_bytes());
        Self {
            d_type: type_,
            d_name,
        }
    }
    /// Creates an empty VfsDirEntry
    pub const fn new_empty() -> Self {
        Self {
            d_type: VfsNodeType::Unknown,
            d_name: [0; 63],
        }
    }
    /// Return the type of the entry
    pub const fn d_type(&self) -> VfsNodeType {
        self.d_type
    }
    /// Return the name of the entry
    pub fn d_name(&self) -> &str {
        core::str::from_utf8(&self.d_name).unwrap()
    }

    pub fn name_as_bytes(&self) -> &[u8] {
        let len = self.d_name.iter().position(|&x| x == 0).unwrap_or(self.d_name.len());
        &self.d_name[..len]
    }
}
