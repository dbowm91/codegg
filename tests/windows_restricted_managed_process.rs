#![cfg(all(windows, feature = "windows-sandbox-test-support"))]

use codegg::managed_process::{
    ManagedProcessRequest, ManagedProcessService, ProcessProvenance, SandboxExecutionOutcome,
    SandboxRequest, StdinPolicy, TerminationReason,
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
            if status.contains("filesystem rights are unqualified")
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
    let script = "[Console]::Out.Write('child-started'); Start-Sleep -Seconds 60";
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

#[tokio::test]
async fn unsupported_filesystem_roots_fail_before_the_test_child_is_created() {
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
    spec.write_paths.push(PathBuf::from("C:\\fixture-root"));

    let error = ManagedProcessService::run(request)
        .await
        .expect_err("test-only path must reject unqualified filesystem rights");
    assert!(matches!(
        error,
        codegg::managed_process::ManagedProcessError::SandboxFailed(reason)
            if reason.contains("does not apply filesystem roots")
    ));
}
