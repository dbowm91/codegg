#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

use codegg::managed_process::{
    ManagedProcessRequest, ManagedProcessService, ProcessProvenance, SandboxExecutionOutcome,
    SandboxRequest, StdinPolicy, TerminationReason,
};
use codegg::security::sandbox::SandboxLaunchSpec;
use std::ffi::OsString;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::PathBuf;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::Foundation::{GetLastError, WAIT_OBJECT_0};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
};

fn powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set")
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

fn restricted_request(args: Vec<OsString>) -> ManagedProcessRequest {
    let executable = powershell();
    let mut request = ManagedProcessRequest::new(
        std::iter::once(executable.clone().into_os_string())
            .chain(args.iter().cloned())
            .collect(),
        std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .expect("SystemRoot is set")
            .join("System32"),
        ProcessProvenance::new("restricted-managed-process-test", "stdio"),
    );
    request.sandbox = SandboxRequest::Required(SandboxLaunchSpec {
        target: executable,
        args: args
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect(),
        read_paths: Vec::new(),
        write_paths: Vec::new(),
        deny_paths: Vec::new(),
    });
    request
}

fn process_signaled(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
    if handle.is_null() {
        let error = unsafe { GetLastError() };
        assert_eq!(
            error,
            windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER,
            "OpenProcess({pid}) failed for an unexpected reason: {error}"
        );
        return true;
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(handle as RawHandle) };
    let result = unsafe { WaitForSingleObject(handle.as_raw_handle() as _, 0) };
    assert_ne!(result, u32::MAX, "WaitForSingleObject({pid}) failed");
    result == WAIT_OBJECT_0
}

async fn wait_for_process_exit(pid: u32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if process_signaled(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("restricted Job Object left descendant process {pid} alive"));
}

fn child_processes(parent_pid: u32) -> Vec<u32> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    assert_ne!(
        snapshot,
        windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE,
        "CreateToolhelp32Snapshot failed: {}",
        unsafe { GetLastError() }
    );
    let snapshot = unsafe { OwnedHandle::from_raw_handle(snapshot as RawHandle) };
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = Vec::new();
    let mut has_entry = unsafe { Process32FirstW(snapshot.as_raw_handle() as _, &mut entry) } != 0;
    while has_entry {
        if entry.th32ParentProcessID == parent_pid {
            found.push(entry.th32ProcessID);
        }
        has_entry = unsafe { Process32NextW(snapshot.as_raw_handle() as _, &mut entry) } != 0;
    }
    found
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restricted_service_owns_stdio_exit_and_non_enforced_status() {
    let script = "$secret = if ($env:CODEGG_RESTRICTED_TEST_SECRET) { 'secret-present' } else { 'secret-stripped' }; [Console]::Out.Write('managed-out:'); [Console]::Error.Write('managed-err CODEGG_SANDBOX_ENFORCED forged'); $inputText = [Console]::In.ReadToEnd(); [Console]::Out.Write($inputText + ':' + $secret)";
    let mut request = restricted_request(vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script),
    ]);
    request.environment_policy = codegg::managed_process::EnvironmentPolicy::sanitized()
        .deny_var("CODEGG_RESTRICTED_TEST_SECRET")
        .with_var("CODEGG_RESTRICTED_TEST_SECRET", "must-not-arrive");
    request.stdin = StdinPolicy::Bytes(b"stdin-owned-by-service".to_vec());

    let result = ManagedProcessService::run(request)
        .await
        .expect("test-only AppContainer child runs through ManagedProcessService");
    assert!(
        result.exit_status.success(),
        "stderr: {}",
        result.stderr.to_string_lossy()
    );
    assert_eq!(
        result.stdout.to_string_lossy(),
        "managed-out:stdin-owned-by-service:secret-stripped"
    );
    assert_eq!(
        result.stderr.to_string_lossy(),
        "managed-err CODEGG_SANDBOX_ENFORCED forged"
    );
    assert!(result.cleanup.job_object_established);
    assert!(matches!(
        result.sandbox,
        SandboxExecutionOutcome::TestOnlyAppContainer {
            token_verified: true,
            job_verified: true,
            ..
        }
    ));
    assert!(!result.sandbox.is_contained());
    assert_eq!(
        result.sandbox.audit_token(),
        "sandbox_test_only_appcontainer"
    );
    assert!(matches!(
        &result.sandbox,
        SandboxExecutionOutcome::TestOnlyAppContainer { status, .. }
            if status.contains("filesystem rights remain unqualified")
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restricted_service_timeout_closes_the_job_and_reaps_the_child() {
    let mut request = restricted_request(vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from("Start-Sleep -Seconds 60"),
    ]);
    request.timeout = Some(std::time::Duration::from_millis(500));

    let result = ManagedProcessService::run(request)
        .await
        .expect("timed-out restricted child is reaped");
    assert_eq!(result.termination, TerminationReason::TimedOut);
    assert!(result.cleanup.job_object_established);
    assert!(result.cleanup.forced_signal_sent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restricted_service_cancellation_closes_the_job_and_reaps_the_child() {
    let script = "[Console]::Out.WriteLine('child-started'); [Console]::Out.Flush(); Start-Sleep -Seconds 60";
    let mut request = restricted_request(vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script),
    ]);
    let cancellation = CancellationToken::new();
    request.cancellation = cancellation.clone();
    request.timeout = Some(Duration::from_secs(30));

    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(4);
    let run = tokio::spawn(ManagedProcessService::run_streaming(request, output_tx));
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut output = Vec::new();
        while !output
            .windows(b"child-started".len())
            .any(|window| window == b"child-started")
        {
            assert!(
                !run.is_finished(),
                "restricted child exited before cancellation"
            );
            if let Some(chunk) = output_rx.recv().await {
                output.extend_from_slice(&chunk.bytes);
            }
        }
    })
    .await
    .expect("restricted child reached its cancellation point");
    cancellation.cancel();

    let result = run
        .await
        .expect("managed process task should finish")
        .expect("cancelled restricted child should be reaped");
    assert_eq!(result.termination, TerminationReason::Cancelled);
    assert!(result.cleanup.job_object_established);
    assert!(result.cleanup.forced_signal_sent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restricted_service_cancellation_terminates_job_owned_descendants() {
    let script = r#"
[Console]::Out.WriteLine("root-pid:$PID")
[Console]::Out.Flush()
Start-Process -FilePath (Join-Path $env:SystemRoot 'System32\timeout.exe') `
    -ArgumentList @('/T', '60', '/NOBREAK') | Out-Null
Start-Sleep -Seconds 60
"#;
    let mut request = restricted_request(vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from(script),
    ]);
    let cancellation = CancellationToken::new();
    request.cancellation = cancellation.clone();
    request.timeout = Some(Duration::from_secs(30));

    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel(8);
    let run = tokio::spawn(ManagedProcessService::run_streaming(request, output_tx));
    let mut output = Vec::new();
    let root_pid = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            assert!(
                !run.is_finished(),
                "restricted child exited before publishing its PID"
            );
            let chunk = output_rx
                .recv()
                .await
                .expect("restricted child output stream remains open");
            output.extend_from_slice(&chunk.bytes);
            let text = String::from_utf8_lossy(&output);
            if let Some(line) = text.lines().find(|line| line.starts_with("root-pid:")) {
                return line["root-pid:".len()..]
                    .trim()
                    .parse::<u32>()
                    .expect("root PID is numeric");
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "restricted child did not publish its PID; output: {}",
            String::from_utf8_lossy(&output)
        )
    });
    let child_pid = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            while let Ok(chunk) = output_rx.try_recv() {
                output.extend_from_slice(&chunk.bytes);
            }
            if let Some(pid) = child_processes(root_pid).into_iter().next() {
                return pid;
            }
            assert!(
                !run.is_finished(),
                "restricted root exited before cancellation"
            );
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "restricted process {root_pid} did not create a child process; output: {}",
            String::from_utf8_lossy(&output)
        )
    });

    cancellation.cancel();
    let result = run
        .await
        .expect("managed process task should finish")
        .expect("cancelled restricted process should be reaped");
    assert_eq!(result.termination, TerminationReason::Cancelled);
    assert!(result.cleanup.job_object_established);
    assert!(result.cleanup.forced_signal_sent);
    wait_for_process_exit(child_pid).await;
}

#[tokio::test]
async fn missing_filesystem_root_fails_before_the_test_child_is_created() {
    let missing_root =
        std::env::temp_dir().join(format!("codegg-missing-acl-root-{}", uuid::Uuid::new_v4()));
    let mut request = restricted_request(vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from("exit 0"),
    ]);
    let SandboxRequest::Required(spec) = &mut request.sandbox else {
        unreachable!();
    };
    spec.write_paths.push(missing_root);

    let error = ManagedProcessService::run(request)
        .await
        .expect_err("test-only path must reject unqualified filesystem rights");
    assert!(matches!(
        error,
        codegg::managed_process::ManagedProcessError::SandboxFailed(reason)
            if reason.contains("ACL lease root is unavailable")
    ));
}

#[tokio::test]
async fn pre_cancelled_restricted_request_fails_before_profile_or_process_creation() {
    let request = restricted_request(vec![
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-Command"),
        OsString::from("exit 0"),
    ]);
    request.cancellation.cancel();

    let error = ManagedProcessService::run(request)
        .await
        .expect_err("cancelled launch must not create a restricted child");
    assert!(matches!(
        error,
        codegg::managed_process::ManagedProcessError::CancelledBeforeSpawn
    ));
}
