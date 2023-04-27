use crate::axerrno::AxError;
use crate::axerrno::AxResult;

pub mod filesystem;
pub mod node;

pub type VfsError = AxError;
pub type VfsResult<T = ()> = AxResult<T>;
