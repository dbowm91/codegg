#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

use codegg::managed_process::{
    ManagedProcessOutputChunk, ManagedProcessRequest, ManagedProcessService, ProcessProvenance,
    SandboxExecutionOutcome, SandboxRequest, TerminationReason,
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
async fn windows_acl_lease_allows_owned_root_and_denies_sibling_then_restores_acl() {
    let fixture = tempfile::tempdir().unwrap();
    let allowed = fixture.path().join("allowed");
    let readonly = fixture.path().join("readonly");
    let outside = fixture.path().join("outside");
    std::fs::create_dir(&allowed).unwrap();
    std::fs::create_dir(&readonly).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::write(readonly.join("seed.txt"), "readonly-content").unwrap();
    std::fs::write(outside.join("secret.txt"), "synthetic-secret").unwrap();
    let allowed_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&allowed).unwrap();
    let readonly_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&readonly).unwrap();
    let outside_before =
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&outside).unwrap();
    let executable = powershell();
    let script = "$write = [IO.Path]::Combine($env:CODEGG_LEASE_ALLOWED, 'created.txt'); [IO.File]::WriteAllText($write, 'allowed-write'); $read = [IO.File]::ReadAllText($write); $readonly = if ([IO.File]::ReadAllText([IO.Path]::Combine($env:CODEGG_LEASE_READONLY, 'seed.txt')).Trim() -eq 'readonly-content') { 'readonly-listed' } else { 'readonly-read-failed' }; try { [IO.File]::WriteAllText([IO.Path]::Combine($env:CODEGG_LEASE_READONLY, 'forbidden.txt'), 'forbidden'); $readonlyWrite = 'unexpected-write' } catch { $readonlyWrite = 'readonly-write-denied' }; try { [IO.File]::ReadAllText([IO.Path]::Combine($env:CODEGG_LEASE_OUTSIDE, 'secret.txt')) | Out-Null; $outsideRead = 'unexpected-read' } catch { $outsideRead = 'read-denied' }; try { [IO.File]::WriteAllText([IO.Path]::Combine($env:CODEGG_LEASE_OUTSIDE, 'created.txt'), 'forbidden'); $outsideWrite = 'unexpected-write' } catch { $outsideWrite = 'write-denied' }; [Console]::Out.Write(($read.Trim() + '|' + $readonly + '|' + $readonlyWrite + '|' + $outsideRead + '|' + $outsideWrite))";
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
        target: executable,
        args: args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        read_paths: vec![readonly.clone()],
        write_paths: vec![allowed.clone()],
        deny_paths: vec![outside.clone()],
    });
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .with_var("CODEGG_LEASE_ALLOWED", allowed.as_os_str())
        .with_var("CODEGG_LEASE_READONLY", readonly.as_os_str())
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
        "allowed-write|readonly-listed|readonly-write-denied|read-denied|write-denied"
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
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&readonly).unwrap(),
        readonly_before
    );
    assert_eq!(
        codegg::managed_process::windows_acl_lease_dacl_snapshot(&outside).unwrap(),
        outside_before
    );
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
