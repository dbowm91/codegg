//! Mechanism-faithful Linux sandbox tests.
//!
//! These tests execute the private helper as a separate process. They never
//! apply Landlock to the test process, and skip only when the running kernel
//! does not provide Landlock.

use codegg::security::sandbox::probe_landlock;
#[cfg(target_os = "linux")]
use codegg::security::sandbox::{
    decode_sandbox_status, SandboxLaunchOutcome, SandboxLaunchSpec, SANDBOX_STATUS_FD,
};
#[cfg(target_os = "linux")]
use nix::unistd::pipe;
#[cfg(target_os = "linux")]
use std::io::Read;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
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
struct HelperOutput {
    output: Output,
    status: Vec<u8>,
}

#[cfg(target_os = "linux")]
fn test_helper_path() -> PathBuf {
    std::env::current_exe()
        .expect("test executable")
        .parent()
        .and_then(|path| path.parent())
        .expect("target debug directory")
        .join("codegg-sandbox-helper")
}

#[cfg(target_os = "linux")]
fn run_helper(spec: &SandboxLaunchSpec, root: &Path) -> HelperOutput {
    let spec_file = tempfile::NamedTempFile::new_in(root).expect("sandbox spec file");
    serde_json::to_writer(spec_file.as_file(), spec).expect("sandbox spec encoding");
    let (reader, writer) = pipe().expect("status pipe");
    let writer_fd = writer.as_raw_fd();
    let mut command = Command::new(test_helper_path());
    command
        .arg("--spec")
        .arg(spec_file.path())
        .arg("--status-fd")
        .arg(SANDBOX_STATUS_FD.to_string());
    unsafe {
        command.pre_exec(move || {
            if writer_fd != SANDBOX_STATUS_FD {
                if libc::dup2(writer_fd, SANDBOX_STATUS_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::close(writer_fd);
            }
            if libc::fcntl(SANDBOX_STATUS_FD, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = command.output().expect("sandbox helper process");
    drop(writer);
    let mut status = Vec::new();
    std::fs::File::from(reader)
        .read_to_end(&mut status)
        .expect("sandbox status frame");
    HelperOutput { output, status }
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
        write_paths: if writable {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        },
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
        read.output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&read.output.stderr)
    );
    assert!(matches!(
        decode_sandbox_status(&read.status).unwrap(),
        SandboxLaunchOutcome::Enforced { .. }
    ));

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
    assert!(!read_only.output.status.success());
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
        write.output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&write.output.stderr)
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
    assert!(!outside_result.output.status.success());
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
    assert_eq!(output.output.status.code(), Some(125));
    assert!(!String::from_utf8_lossy(&output.output.stdout).contains("TARGET_MARKER"));
    match decode_sandbox_status(&output.status).unwrap() {
        SandboxLaunchOutcome::SetupError { reason } => {
            assert!(
                reason.contains("missing-required"),
                "unexpected reason: {reason}"
            );
        }
        other => panic!("expected setup failure, got {other:?}"),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn target_cannot_write_private_status_endpoint_after_exec() {
    if let Err(reason) = probe_landlock() {
        eprintln!("skipped: {reason}");
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace");
    let output = run_helper(
        &base_spec(
            workspace.path(),
            "if printf forged >&3 2>/dev/null; then exit 42; else exit 0; fi",
            false,
        ),
        workspace.path(),
    );
    assert!(
        output.output.status.success(),
        "target retained status writer: {}",
        String::from_utf8_lossy(&output.output.stderr)
    );
    assert!(matches!(
        decode_sandbox_status(&output.status).unwrap(),
        SandboxLaunchOutcome::Enforced { .. }
    ));
}

#[cfg(not(target_os = "linux"))]
#[test]
fn non_linux_reports_landlock_unavailable() {
    assert!(probe_landlock().is_err());
}
