//! Mechanism-faithful Linux sandbox tests.
//!
//! These tests execute the private helper as a separate process. They never
//! apply Landlock to the test process, and skip only when the running kernel
//! does not provide Landlock.

use codegg::security::sandbox::probe_landlock;
#[cfg(target_os = "linux")]
use codegg::security::sandbox::{
    sandbox_helper_path, SandboxLaunchSpec, SANDBOX_HELPER_ERROR_PREFIX,
};
#[cfg(target_os = "linux")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::process::{Command, Output};

#[cfg(target_os = "linux")]
fn runtime_paths(target: &Path) -> Vec<PathBuf> {
    let mut paths = vec![target.to_path_buf()];
    for raw in ["/usr/lib", "/usr/lib64", "/lib", "/lib64", "/dev/null"] {
        let path = Path::new(raw);
        if path.exists() {
            paths.push(path.to_path_buf());
        }
    }
    paths
}

#[cfg(target_os = "linux")]
fn run_helper(spec: &SandboxLaunchSpec, root: &Path) -> Output {
    let spec_file = tempfile::NamedTempFile::new_in(root).expect("sandbox spec file");
    serde_json::to_writer(spec_file.as_file(), spec).expect("sandbox spec encoding");
    sandbox_helper_path()
        .map(|helper| {
            Command::new(helper)
                .arg("--spec")
                .arg(spec_file.path())
                .output()
                .expect("sandbox helper process")
        })
        .expect("sandbox helper must be built for this test")
}

#[cfg(target_os = "linux")]
fn base_spec(root: &Path, script: &str, writable: bool) -> SandboxLaunchSpec {
    let target = PathBuf::from("/bin/sh").canonicalize().expect("/bin/sh");
    let mut read_paths = vec![root.to_path_buf()];
    read_paths.extend(runtime_paths(&target));
    SandboxLaunchSpec {
        target,
        args: vec!["-c".to_string(), script.to_string()],
        read_paths,
        write_paths: writable
            .then(|| vec![root.to_path_buf()])
            .unwrap_or_default(),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn supported_kernel_enforces_read_only_write_and_outside_root_contract() {
    if let Err(reason) = probe_landlock() {
        eprintln!("skipped: {reason}");
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace");
    std::fs::write(workspace.path().join("allowed.txt"), "allowed").expect("allowed file");
    let outside = tempfile::tempdir().expect("outside root");

    let read = run_helper(
        &base_spec(
            workspace.path(),
            &format!(
                "test -r '{}' && ! test -r /etc/passwd",
                workspace.path().join("allowed.txt").display()
            ),
            false,
        ),
        workspace.path(),
    );
    assert!(
        read.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&read.stderr)
    );

    let read_only = run_helper(
        &base_spec(
            workspace.path(),
            &format!(
                "printf blocked > '{}'",
                workspace.path().join("blocked.txt").display()
            ),
            false,
        ),
        workspace.path(),
    );
    assert!(!read_only.status.success());
    assert!(!workspace.path().join("blocked.txt").exists());

    let write = run_helper(
        &base_spec(
            workspace.path(),
            &format!(
                "printf written > '{}'",
                workspace.path().join("written.txt").display()
            ),
            true,
        ),
        workspace.path(),
    );
    assert!(
        write.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&write.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("written.txt")).unwrap(),
        "written"
    );

    let outside_write = outside.path().join("escape.txt");
    let outside_result = run_helper(
        &base_spec(
            workspace.path(),
            &format!("printf escape > '{}'", outside_write.display()),
            true,
        ),
        workspace.path(),
    );
    assert!(!outside_result.status.success());
    assert!(!outside_write.exists());
}

#[cfg(target_os = "linux")]
#[test]
fn required_rule_failure_stops_before_target_execution() {
    if let Err(reason) = probe_landlock() {
        eprintln!("skipped: {reason}");
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace");
    let mut spec = base_spec(workspace.path(), "echo TARGET_MARKER", false);
    spec.read_paths
        .push(workspace.path().join("missing-required"));
    let output = run_helper(&spec, workspace.path());
    assert_eq!(output.status.code(), Some(125));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("TARGET_MARKER"));
    assert!(String::from_utf8_lossy(&output.stderr).contains(SANDBOX_HELPER_ERROR_PREFIX));
}

#[cfg(not(target_os = "linux"))]
#[test]
fn non_linux_reports_landlock_unavailable() {
    assert!(probe_landlock().is_err());
}
