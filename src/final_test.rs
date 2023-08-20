use alloc::{
    string::{String, ToString},
    vec::Vec,
};

use crate::{
    executor::{self, block_on},
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

pub fn run_interrupts() {
    run_binary("interrupts-test-1", Vec::new());
    executor::run_until_idle();
    run_binary("interrupts-test-2", Vec::new());
    executor::run_until_idle();
}

pub fn run_lmbench() {
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_syscall -P 1 null".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_syscall -P 1 read".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_syscall -P 1 write".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/busybox",
        "busybox mkdir -p /var/tmp".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/busybox",
        "busybox touch /var/tmp/lmbench".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_syscall -P 1 stat /var/tmp/lmbench"
            .split(" ")
            .map(|s| s.to_string())
            .collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_syscall -P 1 fstat /var/tmp/lmbench"
            .split(" ")
            .map(|s| s.to_string())
            .collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_syscall -P 1 open /var/tmp/lmbench"
            .split(" ")
            .map(|s| s.to_string())
            .collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_select -n 100 -P 1 file"
            .split(" ")
            .map(|s| s.to_string())
            .collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_sig -P 1 install".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_sig -P 1 catch".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_proc -P 1 fork".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_proc -P 1 exec".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_proc -P 1 shell".split(" ").map(|s| s.to_string()).collect(),
    );
    executor::run_until_idle();
    run_binary(
        "/lmbench_all",
        "lmbench_all lat_ctx -P 1 -s 32 2 4 8 16 24 32 64 96"
            .split(" ")
            .map(|s| s.to_string())
            .collect(),
    );
    executor::run_until_idle();
}

pub fn run_unixbench() {
    run_script("unixbench_testcode.sh");
    executor::run_until_idle();
}

fn run_script(name: &str) {
    let root_dir = fs::get_root_dir();
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
    block_on(lproc.do_exec(busybox, args, envp));
    lproc.with_mut_procfs_info(|info| info.exe_path = Some(Path::from("/busybox")));
    spawn_proc(lproc);
}

fn run_binary(path: &str, args: Vec<String>) {
    let path = Path::from(path);
    let root_dir = fs::get_root_dir();
    let bin = block_on(root_dir.resolve(&path)).expect("Read binary failed");

    // Some necessary environment variables.
    let lproc = LightProcess::new();
    block_on(lproc.do_exec(bin, args, Vec::new()));
    lproc.with_mut_procfs_info(|info| info.exe_path = Some(path));
    spawn_proc(lproc);
}

fn run_binary_with_env(path: &str, args: Vec<String>, envp: Vec<String>) {
    let path = Path::from(path);
    let root_dir = fs::get_root_dir();
    let bin = block_on(root_dir.resolve(&path)).expect("Read binary failed");

    // Some necessary environment variables.
    let lproc = LightProcess::new();
    block_on(lproc.do_exec(bin, args, envp));
    lproc.with_mut_procfs_info(|info| info.exe_path = Some(path));
    spawn_proc(lproc);
}
