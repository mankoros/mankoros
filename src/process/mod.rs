use self::{lproc::LightProcess, userloop::OutermostFuture};
use crate::{executor, fs::new_vfs::top::VfsFileRef};
use alloc::{sync::Arc, vec::Vec};

pub mod aux_vector;
pub mod lproc;
pub mod pid;
mod shared_frame_mgr;
pub mod user_space;
pub mod userloop;
pub use shared_frame_mgr::with_shared_frame_mgr;

pub fn spawn_proc_from_file(file: VfsFileRef) {
    let lproc = LightProcess::new();

    let future = OutermostFuture::new(lproc.clone(), async {
        lproc.clone().do_exec(file, Vec::new(), Vec::new());
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
