use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    executor::block_on,
    fs::{self, new_vfs::path::Path},
    process::{lproc::LightProcess, spawn_proc},
};

/// Execute final competition tests
///
pub fn run_busybox_test() {
    run_script("busybox_testcode.sh");
}

pub fn run_libc_static() {
    run_script("run-static.sh");
}

pub fn run_libc_dynamic() {
    run_script("run-dynamic.sh");
}

pub fn run_lua() {
    run_script("lua_testcode.sh");
}

pub fn run_time_test() {
    run_binary("time-test", Vec::new());
}

pub fn run_libc_bench() {
    run_binary("libc-bench", Vec::new());
}

pub fn run_iozone() {
    run_script("iozone_testcode.sh");
}

fn run_script(name: &str) {
    let root_dir = fs::root::get_root_dir();
    let busybox = block_on(root_dir.lookup("busybox")).expect("Read busybox failed");

    let args = ["busybox", "sh", name]
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
    lproc.do_exec(busybox, args, envp);
    lproc.with_mut_procfs_info(|info| info.exe_path = Some(Path::from("/busybox")));
    spawn_proc(lproc);
}

fn run_binary(path: &str, args: Vec<String>) {
    let path = Path::from(path);
    let root_dir = fs::root::get_root_dir();
    let bin = block_on(root_dir.resolve(&path)).expect("Read binary failed");

    // Some necessary environment variables.
    let lproc = LightProcess::new();
    lproc.do_exec(bin, args, Vec::new());
    lproc.with_mut_procfs_info(|info| info.exe_path = Some(path));
    spawn_proc(lproc);
}
