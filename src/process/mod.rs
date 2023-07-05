use self::{lproc::LightProcess, userloop::OutermostFuture};
use crate::{
    executor,
    fs::{self, vfs::filesystem::VfsNode},
};
use alloc::{string::ToString, sync::Arc, vec::Vec};

pub mod aux_vector;
pub mod lproc;
pub mod pid;
mod shared_frame_mgr;
pub mod user_space;
pub mod userloop;
pub use shared_frame_mgr::with_shared_frame_mgr;

pub fn spawn_proc_from_file(file: Arc<dyn VfsNode>) {
    let lproc = LightProcess::new();

    let future = OutermostFuture::new(lproc.clone(), async {
        lproc.clone().do_exec(file, Vec::new(), Vec::new());
        userloop::userloop(lproc).await;
    });

    let (r, t) = executor::spawn(future);
    r.schedule();
    t.detach();
}

pub fn spawn_init() {
    // Currently, we use busybox sh as the init process.

    let root_dir = fs::root::get_root_dir();
    let busybox = root_dir.clone().lookup("/busybox").expect("Read busybox failed");

    let args = ["sh"].to_vec().into_iter().map(|s| s.to_string()).collect::<Vec<_>>();

    let lproc = LightProcess::new();

    let future = OutermostFuture::new(lproc.clone(), async {
        lproc.clone().do_exec(busybox, args, Vec::new());
        userloop::userloop(lproc).await;
    });

    let (r, t) = executor::spawn(future);
    r.schedule();
    t.detach();
}

pub fn spawn_proc(lproc: Arc<LightProcess>) {
    let future = OutermostFuture::new(lproc.clone(), userloop::userloop(lproc));
    let (r, t) = executor::spawn(future);
    r.schedule();
    t.detach();
}
