#![cfg(target_os = "linux")]

//! Read-only, opt-in cgroup v2 delegation inventory. This fixture does not
//! create cgroups, change controller state, attach processes, or claim quota
//! enforcement. A positive precondition report still requires a separate
//! reviewed attach-before-exec fixture.

use std::fs::{self, OpenOptions};
use std::path::{Component, Path, PathBuf};

const OPT_IN: &str = "CODEGG_RUN_LINUX_CGROUP_V2_PROBE";
const CGROUP_ROOT: &str = "/sys/fs/cgroup";

fn current_cgroup(root: &Path) -> Result<PathBuf, String> {
    let contents = fs::read_to_string("/proc/self/cgroup").map_err(|error| error.to_string())?;
    let relative = contents
        .lines()
        .find_map(|line| line.strip_prefix("0::"))
        .ok_or_else(|| "no unified cgroup v2 entry in /proc/self/cgroup".to_owned())?;
    let path = Path::new(relative.trim_start_matches('/'));
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err("current cgroup path contains a non-normal component".into());
    }
    let resolved = root
        .join(path)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !resolved.starts_with(root) {
        return Err("resolved current cgroup escapes the cgroup v2 mount".into());
    }
    Ok(resolved)
}

fn read_optional(path: &Path) -> String {
    match fs::read_to_string(path) {
        Ok(value) => value.trim().to_owned(),
        Err(error) => format!("unavailable:{}", error.kind()),
    }
}

#[test]
fn opt_in_read_only_cgroup_v2_delegation_inventory() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {OPT_IN}=1 to inspect cgroup v2 delegation");
        return;
    }

    let root = Path::new(CGROUP_ROOT)
        .canonicalize()
        .expect("cgroup v2 mount root");
    let controllers = read_optional(&root.join("cgroup.controllers"));
    assert!(
        !controllers.is_empty() && !controllers.starts_with("unavailable:"),
        "cgroup v2 controllers file unavailable: {controllers}"
    );
    let current = current_cgroup(&root).expect("current unified cgroup path");
    eprintln!(
        "current_cgroup_type={}",
        read_optional(&current.join("cgroup.type"))
    );
    let controller_files = [
        "cgroup.controllers",
        "cgroup.subtree_control",
        "memory.max",
        "pids.max",
        "cpu.max",
        "cgroup.kill",
    ];
    for name in controller_files {
        eprintln!("{name}={}", read_optional(&current.join(name)));
    }
    eprintln!("root_controllers={controllers}");
    eprintln!("current_cgroup={}", current.display());

    // Opening cgroup.procs for write is a non-mutating permission check. This
    // test never writes to it, enables controllers, or creates a child leaf.
    let attach_permission = OpenOptions::new()
        .write(true)
        .open(current.join("cgroup.procs"));
    match attach_permission {
        Ok(file) => {
            drop(file);
            eprintln!(
                "attach_permission=openable; attachment and controller delegation remain unproven"
            );
        }
        Err(error) => {
            eprintln!("attach_permission=unavailable:{}", error.kind());
            eprintln!("disposition=unavailable-no-delegated-writable-cgroup");
        }
    }
}
