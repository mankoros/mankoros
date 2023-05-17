use crate::axerrno::AxError;
use crate::axerrno::AxResult;

pub mod filesystem;
pub mod node;
pub mod path;

use alloc::boxed::Box;
use core::{future::Future, pin::Pin};

pub type VfsError = AxError;
pub type VfsResult<T = ()> = AxResult<T>;
pub type Async<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;
pub type AVfsResult<'a, T = ()> = Async<'a, VfsResult<T>>;
