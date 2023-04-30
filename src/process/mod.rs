use crate::{
    arch, boot::boot_pagetable_paddr, fs::vfs::filesystem::VfsNode, process::process::ProcessInfo,
};
use alloc::{sync::Arc, vec::Vec};

pub mod aux_vector;
pub mod pid_tid;
pub mod process;
mod share_page_mgr;
pub mod user_space;
pub mod userloop;

pub fn spawn_initproc(file: Arc<dyn VfsNode>) {
    let process = ProcessInfo::new();
    debug_assert!(process.pid() == 0, "initproc pid is not 0");

    let thread = process.create_first_thread();
    debug_assert!(thread.tid() == 0, "initproc tid is not 0");

    thread.exec_first(file, Vec::new(), Vec::new());

    arch::switch_page_table(boot_pagetable_paddr());
}

pub fn spawn_proc(file: Arc<dyn VfsNode>) {
    let process = ProcessInfo::new();

    let thread = process.create_first_thread();

    thread.exec_first(file, Vec::new(), Vec::new());

    arch::switch_page_table(boot_pagetable_paddr());
}
