use self::{lproc::LightProcess, userloop::OutermostFuture};
use crate::{executor, fs::vfs::filesystem::VfsNode};
use alloc::{sync::Arc, vec::Vec};

pub mod aux_vector;
pub mod lproc;
pub mod pid;
mod shared_frame_mgr;
pub mod user_space;
pub mod userloop;

pub fn spawn_proc(file: Arc<dyn VfsNode>) {
    let lproc = LightProcess::new();

    let future = OutermostFuture::new(lproc.clone(), async {
        lproc.clone().exec_first(file, Vec::new(), Vec::new());
        userloop::userloop(lproc).await;
    });

    let (r, t) = executor::spawn(future);
    r.run();
    t.detach();
}
