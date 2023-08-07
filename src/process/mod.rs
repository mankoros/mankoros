use self::{lproc::LightProcess, userloop::OutermostFuture};
use crate::{
    executor::{self, block_on},
    fs::{
        self,
        new_vfs::{path::Path, top::VfsFileRef},
    },
};
use alloc::{
    string::{String, ToString},
    sync::Arc,
    vec::Vec,
};

pub mod elf;
pub mod lproc;
pub mod lproc_mgr;
pub mod pid;
pub mod user_space;
pub mod userloop;

pub fn spawn_proc_from_file(path: Path, file: VfsFileRef) {
    let lproc = LightProcess::new();

    block_on(lproc.do_exec(file, Vec::new(), Vec::new()));
    lproc.with_mut_procfs_info(|info| info.exe_path = Some(path));
    spawn_proc(lproc);
}

pub fn spawn_init() {
    // Currently, we use busybox sh as the init process.

    let root_dir = fs::root::get_root_dir();
    let busybox = block_on(root_dir.lookup("busybox")).expect("Read busybox failed");

    let args = ["busybox", "sh"]
        .to_vec()
        .into_iter()
        .map(|s: &str| s.to_string())
        .collect::<Vec<_>>();

    // Some necessary environment variables.
    let mut envp = Vec::new();
    envp.push(String::from("LD_LIBRARY_PATH=."));
    envp.push(String::from("SHELL=/busybox"));
    envp.push(String::from("PWD=/"));
    envp.push(String::from("USER=root"));
    envp.push(String::from("MOTD_SHOWN=pam"));
    envp.push(String::from("LANG=C.UTF-8"));
    envp.push(String::from(
        "INVOCATION_ID=e9500a871cf044d9886a157f53826684",
    ));
    envp.push(String::from("TERM=vt220"));
    envp.push(String::from("SHLVL=2"));
    envp.push(String::from("JOURNAL_STREAM=8:9265"));
    envp.push(String::from("OLDPWD=/root"));
    envp.push(String::from("_=busybox"));
    envp.push(String::from("LOGNAME=root"));
    envp.push(String::from("HOME=/"));
    envp.push(String::from("PATH=/"));

    let lproc = LightProcess::new();
    block_on(lproc.do_exec(busybox, args, envp));
    lproc.with_mut_procfs_info(|info| info.exe_path = Some(Path::from("/busybox")));
    spawn_proc(lproc);
}

pub fn spawn_proc(lproc: Arc<LightProcess>) {
    let future = OutermostFuture::new(lproc.clone(), userloop::userloop(lproc));
    let (r, t) = executor::spawn(future);
    r.schedule();
    t.detach();
}
