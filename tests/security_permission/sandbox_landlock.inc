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
    let helper = test_helper_path();
    let mut command = Command::new(&helper);
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
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("sandbox helper process at {}: {error}", helper.display()));
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
        env: Vec::new(),
        linux_protected_mount_view: false,
        linux_process_isolation: false,
        linux_offline_network: false,
        protected_readonly_paths: Vec::new(),
        path_identities: Vec::new(),
        expected_created_git_pointers: Vec::new(),
        target,
        args: vec!["-c".to_string(), script.to_string()],
        read_paths,
        write_paths: if writable {
            vec![root.to_path_buf()]
        } else {
            Vec::new()
        },
        deny_paths: Vec::new(),
    }
}

#[cfg(target_os = "linux")]
#[test]
fn supported_kernel_enforces_read_only_write_and_outside_root_contract() {
    if let Err(reason) = probe_landlock() {
        assert!(
            !qualification_required(),
            "the qualification lane requires a Landlock backend: {reason}"
        );
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
                "(exec 3< '{}') && !(exec 3< /etc/passwd)",
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
        assert!(
            !qualification_required(),
            "the qualification lane requires a Landlock backend: {reason}"
        );
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
        assert!(
            !qualification_required(),
            "the qualification lane requires a Landlock backend: {reason}"
        );
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

/// True when this run is a qualification lane, where a missing backend is a
/// **failure** rather than a skip.
///
/// `ci.yml` sets the marker on the Linux job, so a runner that loses its
/// Landlock backend turns red instead of reporting a green skip. Locally it is
/// unset, so a host without Landlock still skips.
#[cfg(target_os = "linux")]
fn qualification_required() -> bool {
    matches!(
        std::env::var("CODEGG_SANDBOX_QUALIFICATION").as_deref(),
        Ok("1") | Ok("true")
    )
}

/// The grant set a launch is measured under.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy, PartialEq)]
enum GrantArm {
    /// The production set, exactly as `policy::tool_read_allowances` builds it.
    Production,
    /// Production plus the `/proc` tree the narrowing removed: the positive
    /// control for "the grant is the barrier, not the kernel".
    WithProcTree,
    /// Production with every `/proc` entry removed: what the narrow entries are
    /// load-bearing for.
    WithoutProc,
}

/// A launch under the **production** read grants, not a hand-made set.
#[cfg(target_os = "linux")]
fn production_spec(root: &Path, script: &str, arm: GrantArm) -> SandboxLaunchSpec {
    use codegg::security::sandbox::policy;

    let mut spec = base_spec(root, script, false);
    let mut read_paths: Vec<PathBuf> = vec![root.to_path_buf()];
    for path in policy::tool_read_allowances() {
        if !path.exists() {
            continue;
        }
        // The per-process aliases travel unresolved, exactly as the product
        // sends them: canonicalizing one here would bind this test process's
        // pid instead of the sandboxed command's.
        if policy::is_self_proc_alias(&path) {
            if arm != GrantArm::WithoutProc {
                read_paths.push(path);
            }
            continue;
        }
        let Ok(resolved) = policy::resolve_policy_path(&path) else {
            continue;
        };
        if arm == GrantArm::WithoutProc && resolved.starts_with("/proc") {
            continue;
        }
        read_paths.push(resolved);
    }
    if arm == GrantArm::WithProcTree {
        read_paths.push(PathBuf::from("/proc"));
    }
    spec.read_paths = read_paths;
    spec
}

/// The probes the narrowing must not break.
#[cfg(target_os = "linux")]
fn toolchain_probes() -> Vec<(&'static str, &'static str)> {
    vec![
        ("shell", "true"),
        ("git", "git --version >/dev/null"),
        ("python", "python3 -c 'import json' >/dev/null"),
        ("resolver", "getent hosts localhost >/dev/null"),
        ("self-proc", "IFS= read -r line < /proc/self/stat"),
    ]
}

/// The C002 finding, proved on a real launch: another process's `/proc` entry
/// is out of reach under the default grant set, and the grant is the barrier.
#[cfg(target_os = "linux")]
#[test]
fn the_default_grant_set_denies_another_processes_proc_entry() {
    if let Err(reason) = probe_landlock() {
        assert!(
            !qualification_required(),
            "the qualification lane requires a Landlock backend: {reason}"
        );
        eprintln!("skipped: {reason}");
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace");
    // A same-uid process to look at. `/proc/<pid>/stat` is world-readable, so
    // the kernel permits the read and only the grant can deny it.
    let mut holder = Command::new("/bin/sh")
        .args(["-c", "sleep 30"])
        .spawn()
        .expect("environment holder");
    let pid = holder.id();
    let stat = format!("cat /proc/{pid}/stat >/dev/null");
    let environ = format!("cat /proc/{pid}/environ >/dev/null");

    let denied = run_helper(
        &production_spec(workspace.path(), &stat, GrantArm::Production),
        workspace.path(),
    );
    assert!(
        !denied.output.status.success(),
        "the default grant set still reaches another process's `/proc/<pid>/stat`"
    );

    // The property itself: another process's environment is unreadable.
    let denied_environ = run_helper(
        &production_spec(workspace.path(), &environ, GrantArm::Production),
        workspace.path(),
    );
    assert!(
        !denied_environ.output.status.success(),
        "the default grant set still reads `/proc/<pid>/environ`"
    );

    // The command's own `/proc/self` survives, and the reader must be the
    // shell itself: `read` with a redirection opens the file at the pid the
    // helper exec'd into, while a child's `/proc/self` names the child - the
    // residue the descriptor states.
    let own = run_helper(
        &production_spec(
            workspace.path(),
            "IFS= read -r line < /proc/self/stat",
            GrantArm::Production,
        ),
        workspace.path(),
    );
    assert!(
        own.output.status.success(),
        "the narrowing denied the command its own `/proc/self`: {}",
        String::from_utf8_lossy(&own.output.stderr)
    );

    // Positive control: with the `/proc` tree restored the same entry reads, so
    // the denials above are the grant and not the kernel.
    let allowed = run_helper(
        &production_spec(workspace.path(), &stat, GrantArm::WithProcTree),
        workspace.path(),
    );
    assert!(
        allowed.output.status.success(),
        "the positive control could not read `/proc/<pid>/stat` with `/proc` restored: {}",
        String::from_utf8_lossy(&allowed.output.stderr)
    );

    let _ = holder.kill();
    let _ = holder.wait();
}

/// The narrowing is sufficient for the toolchain, and its `/proc` entries are
/// load-bearing rather than decorative.
///
/// This is the committed matrix the plan asked for, reduced from three arms to
/// two: `/var/run` is no longer granted at all, so the "without `/var/run`" arm
/// has nothing left to remove — its only surviving entries are the resolver
/// files, and they are conditional on the distribution's layout.
#[cfg(target_os = "linux")]
#[test]
fn the_narrowed_grant_set_runs_the_toolchain_and_its_proc_entries_are_load_bearing() {
    if let Err(reason) = probe_landlock() {
        assert!(
            !qualification_required(),
            "the qualification lane requires a Landlock backend: {reason}"
        );
        eprintln!("skipped: {reason}");
        return;
    }
    let workspace = tempfile::tempdir().expect("workspace");
    let probes = toolchain_probes();

    let mut broken = Vec::new();
    for (name, script) in &probes {
        let out = run_helper(
            &production_spec(workspace.path(), script, GrantArm::Production),
            workspace.path(),
        );
        if !out.output.status.success() {
            broken.push(format!(
                "{name}: {}",
                String::from_utf8_lossy(&out.output.stderr).trim()
            ));
        }
    }
    assert!(
        broken.is_empty(),
        "the narrowed grant set broke the toolchain: {broken:?}"
    );

    let mut dependent = Vec::new();
    for (name, script) in &probes {
        let out = run_helper(
            &production_spec(workspace.path(), script, GrantArm::WithoutProc),
            workspace.path(),
        );
        if !out.output.status.success() {
            dependent.push(*name);
        }
    }
    assert!(
        !dependent.is_empty(),
        "no probe needed `/proc` at all, so the narrow `/proc` entries are dead weight"
    );
}

/// A green qualification lane implies a Landlock ABI 6 runner.
///
/// The two failure modes used to be indistinguishable from a green run: a
/// runner without a backend *skipped* the fixtures, and a pre-6.12 kernel
/// *failed* the ABI-6 assertions. With the marker set the first becomes a
/// failure too, and this test names the ABI the run actually obtained.
///
/// The marker itself is asserted on every host, so a typo in `ci.yml` cannot
/// silently disable the lane and leave this test green for the wrong reason.
#[cfg(target_os = "linux")]
#[test]
fn the_qualification_lane_obtains_landlock_abi_six() {
    let marker = std::env::var("CODEGG_SANDBOX_QUALIFICATION").ok();
    assert!(
        matches!(
            marker.as_deref(),
            None | Some("") | Some("0") | Some("1") | Some("true")
        ),
        "`CODEGG_SANDBOX_QUALIFICATION` carries an unrecognised value: {marker:?}"
    );
    if qualification_required() {
        codegg::security::sandbox::landlock::probe_process_scopes().unwrap_or_else(|reason| {
            panic!("the qualification lane requires Landlock ABI 6 process scopes: {reason}")
        });
    }
}

#[cfg(not(target_os = "linux"))]
#[test]
fn non_linux_reports_landlock_unavailable() {
    assert!(probe_landlock().is_err());
}
