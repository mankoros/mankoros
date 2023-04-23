use crate::{process::process::ProcessInfo, vfs::filesystem::VfsNode};
use alloc::{vec::Vec, sync::Arc};

pub mod aux_vector;
pub mod pid_tid;
pub mod process;
pub mod user_space;
pub mod userloop;
mod share_page_mgr;

pub fn spawn_initproc(file: Arc<dyn VfsNode>) {        
    let process = ProcessInfo::new();
    debug_assert!(process.pid() == 1, "initproc pid is not 1");

    let thread = process.create_first_thread();
    debug_assert!(thread.tid() == 1, "initproc tid is not 1");

    thread.exec_first(file, Vec::new(), Vec::new());
}