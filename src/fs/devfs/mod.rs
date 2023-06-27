//! Device filesystem used by [ArceOS](https://github.com/rcore-os/arceos).
//!
//! The implementation is based on [`axfs_vfs`].
//!

mod dir;
mod zero;

pub use dir::InMemoryDir;
pub use zero::ZeroDev;
