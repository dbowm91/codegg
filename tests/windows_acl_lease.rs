#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

use codegg::managed_process::{
    ManagedProcessError, ManagedProcessOutputChunk, ManagedProcessRequest, ManagedProcessService,
    ProcessProvenance, SandboxExecutionOutcome, SandboxRequest, TerminationReason,
};
use codegg::security::sandbox::SandboxLaunchSpec;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set")
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_acl_lease_rejects_symlink_root_before_child_creation() {
    let fixture = tempfile::tempdir().unwrap();
    let target = fixture.path().join("actual-root");
    let link = fixture.path().join("linked-root");
    std::fs::create_dir(&target).unwrap();
    std::os::windows::fs::symlink_dir(&target, &link)
        .expect("elevated disposable Windows runner can create a directory symlink");
    assert_reparse_root_rejected(link, fixture.path().join("child-started.txt")).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_acl_lease_rejects_reparse_ancestor_before_child_creation() {
    let fixture = tempfile::tempdir().unwrap();
    let target_parent = fixture.path().join("actual-parent");
    let linked_parent = fixture.path().join("linked-parent");
    std::fs::create_dir(&target_parent).unwrap();
    let nested = target_parent.join("nested-root");
    std::fs::create_dir(&nested).unwrap();
    std::os::windows::fs::symlink_dir(&target_parent, &linked_parent)
        .expect("elevated disposable Windows runner can create a directory symlink");
    assert_reparse_root_rejected(
        linked_parent.join("nested-root"),
        fixture.path().join("child-started.txt"),
    )
    .await;
}

async fn assert_reparse_root_rejected(root: PathBuf, marker: PathBuf) {
    let executable = powershell();
    let script = "[IO.File]::WriteAllText($env:CODEGG_CHILD_MARKER, 'started')";
    let args = vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script),
    ];
    let mut request = ManagedProcessRequest::new(
        std::iter::once(executable.clone().into_os_string())
            .chain(args.iter().cloned())
            .collect(),
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32"),
        ProcessProvenance::new("windows-acl-lease", "reparse-root"),
    );
    request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
        linux_protected_mount_view: false,
        protected_readonly_paths: Vec::new(),
        path_identities: Vec::new(),
        target: executable,
        args: args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        read_paths: Vec::new(),
        write_paths: vec![root],
        deny_paths: Vec::new(),
    });
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .with_var("CODEGG_CHILD_MARKER", marker.as_os_str());

    let result = ManagedProcessService::run(request).await;
    assert!(
        matches!(&result, Err(ManagedProcessError::SandboxFailed(_))),
        "reparse root must fail closed before child creation: {result:?}"
    );
    assert!(!marker.exists(), "rejected child must not have started");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_acl_lease_allows_empty_write_root_and_denies_empty_sibling_then_restores_acl() {
    let fixture = tempfile::tempdir().unwrap();
    let allowed = fixture.path().join("allowed");
    let outside = fixture.path().join("outside");
    std::fs::create_dir(&allowed).unwrap();
    std::fs::create_dir(&outside).unwrap();
    let allowed_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&allowed).unwrap();
    let outside_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&outside).unwrap();
    let journal_root = codegg_client::LocalDaemonPaths::resolve()
        .root
        .join("sandbox")
        .join("acl-leases");
    let executable = powershell();
    let script = r#"
$write = [IO.Path]::Combine($env:CODEGG_LEASE_ALLOWED, 'created.txt')
[IO.File]::WriteAllText($write, 'allowed-write')
$read = [IO.File]::ReadAllText($write)
try {
    $null = [IO.Directory]::GetFiles($env:CODEGG_LEASE_OUTSIDE)
    $outsideRead = 'unexpected-read'
} catch { $outsideRead = 'read-denied' }
try {
    [IO.File]::WriteAllText([IO.Path]::Combine($env:CODEGG_LEASE_OUTSIDE, 'created.txt'), 'forbidden')
    $outsideWrite = 'unexpected-write'
} catch { $outsideWrite = 'write-denied' }
[Console]::Out.Write(($read.Trim() + '|' + $outsideRead + '|' + $outsideWrite))
"#;
    let args = vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script),
    ];
    let mut request = ManagedProcessRequest::new(
        std::iter::once(executable.clone().into_os_string())
            .chain(args.iter().cloned())
            .collect(),
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32"),
        ProcessProvenance::new("windows-acl-lease", "allowed-denied"),
    );
    request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
        linux_protected_mount_view: false,
        protected_readonly_paths: Vec::new(),
        path_identities: Vec::new(),
        target: executable,
        args: args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        read_paths: Vec::new(),
        write_paths: vec![allowed.clone()],
        deny_paths: vec![outside.clone()],
    });
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .with_var("CODEGG_LEASE_ALLOWED", allowed.as_os_str())
        .with_var("CODEGG_LEASE_OUTSIDE", outside.as_os_str());

    let result = ManagedProcessService::run(request)
        .await
        .expect("leased AppContainer command runs through ManagedProcessService");
    assert!(
        result.exit_status.success(),
        "{}",
        result.stderr.to_string_lossy()
    );
    assert_eq!(
        result.stdout.to_string_lossy(),
        "allowed-write|read-denied|write-denied"
    );
    assert!(matches!(
        result.sandbox,
        SandboxExecutionOutcome::TestOnlyAppContainer {
            token_verified: true,
            job_verified: true,
            ..
        }
    ));
    assert_eq!(
        std::fs::read_to_string(allowed.join("created.txt"))
            .unwrap()
            .trim(),
        "allowed-write"
    );
    assert!(!outside.join("created.txt").exists());
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&allowed).unwrap(),
        allowed_before
    );
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&outside).unwrap(),
        outside_before
    );
    assert!(std::fs::read_dir(journal_root).unwrap().all(|entry| {
        entry
            .unwrap()
            .path()
            .extension()
            .and_then(|extension| extension.to_str())
            != Some("json")
    }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn windows_acl_lease_cancellation_restores_after_job_termination() {
    let fixture = tempfile::tempdir().unwrap();
    let allowed = fixture.path().join("allowed");
    std::fs::create_dir(&allowed).unwrap();
    let before = codegg::managed_process::windows_acl_lease_dacl_snapshot(&allowed).unwrap();
    let executable = powershell();
    let script =
        "[Console]::Out.Write('lease-started'); [Console]::Out.Flush(); Start-Sleep -Seconds 60";
    let args = vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script),
    ];
    let mut request = ManagedProcessRequest::new(
        std::iter::once(executable.clone().into_os_string())
            .chain(args.iter().cloned())
            .collect(),
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32"),
        ProcessProvenance::new("windows-acl-lease", "cancellation"),
    );
    request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
        linux_protected_mount_view: false,
        protected_readonly_paths: Vec::new(),
        path_identities: Vec::new(),
        target: executable,
        args: args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        read_paths: Vec::new(),
        write_paths: vec![allowed.clone()],
        deny_paths: Vec::new(),
    });
    let cancellation = CancellationToken::new();
    request.cancellation = cancellation.clone();
    request.timeout = Some(Duration::from_secs(30));
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<ManagedProcessOutputChunk>(8);
    let run = tokio::spawn(ManagedProcessService::run_streaming(request, output_tx));
    let mut output = Vec::new();
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            assert!(
                !run.is_finished(),
                "restricted child exited before startup marker"
            );
            let chunk = output_rx.recv().await.expect("output stream remains open");
            output.extend_from_slice(&chunk.bytes);
            if String::from_utf8_lossy(&output).contains("lease-started") {
                return;
            }
        }
    })
    .await
    .expect("restricted child emits startup marker");
    cancellation.cancel();
    let result = run
        .await
        .expect("managed process task completes")
        .expect("cancelled restricted child is reaped");
    assert_eq!(result.termination, TerminationReason::Cancelled);
    assert!(result.cleanup.job_object_established);
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&allowed).unwrap(),
        before
    );
}
