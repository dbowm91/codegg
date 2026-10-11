#![cfg(windows)]

use codegg::managed_process::{
    ManagedProcessError, ManagedProcessRequest, ManagedProcessService, OutputPolicy,
    ProcessProvenance, TerminationReason,
};
use std::ffi::OsString;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::Foundation::{GetLastError, WAIT_OBJECT_0};
use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, SYNCHRONIZE};

fn powershell() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .expect("SystemRoot is set")
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

fn request(script: impl Into<OsString>) -> ManagedProcessRequest {
    ManagedProcessRequest::new(
        vec![
            powershell().into_os_string(),
            OsString::from("-NoLogo"),
            OsString::from("-NoProfile"),
            OsString::from("-NonInteractive"),
            OsString::from("-Command"),
            script.into(),
        ],
        std::env::current_dir().expect("current directory"),
        ProcessProvenance::new("windows-process-launcher-test", "attempt"),
    )
}

fn powershell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
}

fn process_signaled(pid: u32) -> bool {
    let handle = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
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

async fn wait_for_pid_file(path: &Path) -> u32 {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(value) = tokio::fs::read_to_string(path).await {
                if let Ok(pid) = value.trim().parse() {
                    return pid;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("PowerShell child did not publish its PID")
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
    .unwrap_or_else(|_| panic!("Job Object left descendant process {pid} alive"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launches_unicode_argv_and_reports_job_ownership() {
    let sentinel = "CodeGG snowman ☃ — こんにちは";
    let script =
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::Write($args[0])";
    let mut process = request(script);
    process.argv.push(OsString::from(sentinel));

    let result = ManagedProcessService::run(process)
        .await
        .expect("PowerShell should launch through ManagedProcessService");
    assert!(result.exit_status.success());
    assert_eq!(result.stdout.as_bytes().as_slice(), sentinel.as_bytes());
    assert!(result.cleanup.job_object_established);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timeout_terminates_the_job_and_classifies_the_exit() {
    let mut process = request("Start-Sleep -Seconds 60");
    process.timeout = Some(Duration::from_millis(100));

    let result = ManagedProcessService::run(process)
        .await
        .expect("timed process should be reaped");
    assert_eq!(result.termination, TerminationReason::TimedOut);
    assert!(result.cleanup.job_object_established);
    assert!(result.cleanup.forced_signal_sent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn output_capture_remains_bounded_on_windows() {
    let mut process = request("[Console]::Write('x' * 4096)");
    process.output_policy = OutputPolicy::new(64);

    let result = ManagedProcessService::run(process)
        .await
        .expect("PowerShell should exit after bounded output");
    assert!(result.exit_status.success());
    assert_eq!(result.stdout.total_bytes, 4096);
    assert!(result.stdout.retained_bytes() <= 64);
    assert!(result.stdout.is_truncated());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelling_one_run_does_not_terminate_a_sibling_job() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let first_marker = directory.path().join("first-pid.txt");
    let sibling_marker = directory.path().join("sibling-pid.txt");
    let cancellation = CancellationToken::new();
    let mut first = request(format!(
        "[IO.File]::WriteAllText({}, $PID.ToString()); Start-Sleep -Seconds 60",
        powershell_quote(&first_marker)
    ));
    first.cancellation = cancellation.clone();
    first.timeout = Some(Duration::from_secs(30));
    let first = tokio::spawn(ManagedProcessService::run(first));
    let _first_pid = wait_for_pid_file(&first_marker).await;

    let mut sibling = request(format!(
        "[IO.File]::WriteAllText({}, $PID.ToString()); Start-Sleep -Milliseconds 500; [Console]::Write('sibling survived')",
        powershell_quote(&sibling_marker)
    ));
    sibling.timeout = Some(Duration::from_secs(10));
    let sibling = tokio::spawn(ManagedProcessService::run(sibling));
    let _sibling_pid = wait_for_pid_file(&sibling_marker).await;
    cancellation.cancel();

    let cancelled = first
        .await
        .expect("first managed process task")
        .expect("cancelled process should be reaped");
    let survived = sibling
        .await
        .expect("sibling managed process task")
        .expect("sibling process should be unaffected");
    assert_eq!(cancelled.termination, TerminationReason::Cancelled);
    assert!(survived.exit_status.success());
    assert_eq!(survived.stdout.to_string_lossy(), "sibling survived");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_terminates_root_and_job_owned_descendants() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let pid_file = directory.path().join("child-pid.txt");
    let system_root = std::env::var("SystemRoot").expect("SystemRoot is set");
    let cmd = PathBuf::from(&system_root).join("System32").join("cmd.exe");
    let script = format!(
        "$child = Start-Process -FilePath {} -ArgumentList @('/d', '/c', 'timeout /t 60 /nobreak > nul') -PassThru -WindowStyle Hidden; [IO.File]::WriteAllText({}, $child.Id.ToString()); Start-Sleep -Seconds 60",
        powershell_quote(&cmd),
        powershell_quote(&pid_file),
    );
    let cancellation = CancellationToken::new();
    let mut process = request(script);
    process.cancellation = cancellation.clone();
    process.timeout = Some(Duration::from_secs(30));

    let run = tokio::spawn(ManagedProcessService::run(process));
    let child_pid = wait_for_pid_file(&pid_file).await;
    cancellation.cancel();
    let result = run
        .await
        .expect("managed process task should finish")
        .expect("cancelled process should be reaped");

    assert_eq!(result.termination, TerminationReason::Cancelled);
    assert!(result.cleanup.job_object_established);
    assert!(result.cleanup.forced_signal_sent);
    wait_for_process_exit(child_pid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn root_exit_closes_the_job_before_waiting_for_inherited_pipe_eof() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let pid_file = directory.path().join("child-pid.txt");
    let system_root = std::env::var("SystemRoot").expect("SystemRoot is set");
    let cmd = PathBuf::from(&system_root).join("System32").join("cmd.exe");
    let script = format!(
        "$null = Start-Process -FilePath {} -ArgumentList @('/d', '/c', 'timeout /t 60 /nobreak > nul') -PassThru -WindowStyle Hidden; [IO.File]::WriteAllText({}, $null.Id.ToString())",
        powershell_quote(&cmd),
        powershell_quote(&pid_file),
    );
    let result = ManagedProcessService::run(request(script))
        .await
        .expect("root exit should close its Job before joining inherited pipes");
    let child_pid = wait_for_pid_file(&pid_file).await;

    assert!(result.exit_status.success());
    assert!(result.cleanup.job_object_established);
    wait_for_process_exit(child_pid).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn required_filesystem_sandbox_fails_before_launch_on_windows() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let marker = directory.path().join("should-not-exist.txt");
    let mut process = request("[IO.File]::WriteAllText('should-not-exist', 'ran')");
    *process
        .argv
        .last_mut()
        .expect("PowerShell command argument") = OsString::from(format!(
        "[IO.File]::WriteAllText({}, 'ran')",
        powershell_quote(&marker)
    ));
    process.sandbox = codegg::managed_process::SandboxRequest::Required(
        codegg::security::sandbox::SandboxLaunchSpec {
            target: powershell(),
            args: vec![],
            read_paths: vec![],
            write_paths: vec![],
            deny_paths: vec![],
        },
    );

    let error = ManagedProcessService::run(process)
        .await
        .expect_err("Windows M002 must not report filesystem enforcement");
    assert!(matches!(error, ManagedProcessError::SandboxFailed(_)));
    assert!(
        !marker.exists(),
        "required sandbox failure must precede spawn"
    );
}
