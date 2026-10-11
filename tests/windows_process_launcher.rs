#![cfg(windows)]

use codegg::managed_process::{
    EnvironmentPolicy, ManagedProcessError, ManagedProcessRequest, ManagedProcessService,
    OutputPolicy, ProcessProvenance, TerminationReason,
};
use std::ffi::OsString;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use windows_sys::Win32::Foundation::{GetLastError, WAIT_OBJECT_0};
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

fn request_script(path: &Path, script: &str, args: &[OsString]) -> ManagedProcessRequest {
    std::fs::write(path, script).expect("write PowerShell fixture");
    let mut argv = vec![
        powershell().into_os_string(),
        OsString::from("-NoLogo"),
        OsString::from("-NoProfile"),
        OsString::from("-NonInteractive"),
        OsString::from("-File"),
        path.as_os_str().to_owned(),
    ];
    argv.extend(args.iter().cloned());
    ManagedProcessRequest::new(
        argv,
        std::env::current_dir().expect("current directory"),
        ProcessProvenance::new("windows-process-launcher-test", "attempt"),
    )
}

fn process_fixture_request(
    mode: &str,
    pid_file: &Path,
    child_pid_file: Option<&Path>,
) -> ManagedProcessRequest {
    let argv = vec![
        std::env::current_exe()
            .expect("Windows process test executable")
            .into_os_string(),
        OsString::from("--exact"),
        OsString::from("windows_process_fixture"),
        OsString::from("--nocapture"),
    ];
    let mut request = ManagedProcessRequest::new(
        argv,
        std::env::current_dir().expect("current directory"),
        ProcessProvenance::new("windows-process-launcher-fixture", mode),
    );
    let mut environment = EnvironmentPolicy::sanitized()
        .with_var("CODEGG_WINDOWS_FIXTURE_MODE", OsString::from(mode))
        .with_var(
            "CODEGG_WINDOWS_FIXTURE_PID_FILE",
            pid_file.as_os_str().to_owned(),
        );
    if let Some(child_pid_file) = child_pid_file {
        environment = environment.with_var(
            "CODEGG_WINDOWS_FIXTURE_CHILD_PID_FILE",
            child_pid_file.as_os_str().to_owned(),
        );
    }
    request.environment_policy = environment;
    request
}

fn powershell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "''"))
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

async fn wait_for_pid_file(
    path: &Path,
    process: &mut tokio::task::JoinHandle<
        Result<codegg::managed_process::ManagedProcessResult, ManagedProcessError>,
    >,
) -> u32 {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(value) = tokio::fs::read_to_string(path).await {
                if let Ok(pid) = value.trim().parse() {
                    return pid;
                }
            }
            if process.is_finished() {
                let result = (&mut *process).await;
                panic!("managed process exited before publishing its PID: {result:?}");
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("fixture process did not publish its PID")
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

#[test]
fn windows_process_fixture() {
    let Ok(mode) = std::env::var("CODEGG_WINDOWS_FIXTURE_MODE") else {
        return;
    };
    let pid_file = std::env::var_os("CODEGG_WINDOWS_FIXTURE_PID_FILE")
        .map(PathBuf::from)
        .expect("fixture PID file");

    if mode == "spawn-child-and-wait" || mode == "spawn-child-and-exit" {
        std::fs::write(&pid_file, std::process::id().to_string()).expect("write fixture PID");
        let child_pid_file = std::env::var_os("CODEGG_WINDOWS_FIXTURE_CHILD_PID_FILE")
            .map(PathBuf::from)
            .expect("fixture child PID file");
        let child =
            StdCommand::new(std::env::current_exe().expect("Windows process test executable"))
                .args(["--exact", "windows_process_fixture", "--nocapture"])
                .env("CODEGG_WINDOWS_FIXTURE_MODE", "wait")
                .env("CODEGG_WINDOWS_FIXTURE_PID_FILE", &child_pid_file)
                .spawn()
                .expect("spawn Job-owned fixture descendant");
        std::fs::write(&child_pid_file, child.id().to_string()).expect("write fixture child PID");
        if mode == "spawn-child-and-exit" {
            return;
        }
    } else {
        std::fs::write(&pid_file, std::process::id().to_string()).expect("write fixture PID");
        if mode == "sibling" {
            std::thread::sleep(Duration::from_millis(500));
            return;
        }
    }

    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn launches_unicode_argv_and_reports_job_ownership() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let sentinel = "CodeGG snowman ☃ — こんにちは";
    let script =
        "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; [Console]::Write($args[0])";
    let process = request_script(
        &directory.path().join("echo-argv.ps1"),
        script,
        &[OsString::from(sentinel)],
    );

    let result = ManagedProcessService::run(process)
        .await
        .expect("PowerShell should launch through ManagedProcessService");
    assert!(
        result.exit_status.success(),
        "PowerShell exited {:?}; stdout={:?} stderr={:?}",
        result.exit_status,
        result.stdout.to_string_lossy(),
        result.stderr.to_string_lossy()
    );
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
    assert!(
        result.exit_status.success(),
        "PowerShell exited {:?}; stdout={:?} stderr={:?}",
        result.exit_status,
        result.stdout.to_string_lossy(),
        result.stderr.to_string_lossy()
    );
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
    let mut first = process_fixture_request("wait", &first_marker, None);
    first.cancellation = cancellation.clone();
    first.timeout = Some(Duration::from_secs(30));
    let mut first = tokio::spawn(ManagedProcessService::run(first));
    let _first_pid = wait_for_pid_file(&first_marker, &mut first).await;

    let mut sibling = process_fixture_request("sibling", &sibling_marker, None);
    sibling.timeout = Some(Duration::from_secs(10));
    let mut sibling = tokio::spawn(ManagedProcessService::run(sibling));
    let _sibling_pid = wait_for_pid_file(&sibling_marker, &mut sibling).await;
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
    assert_eq!(survived.termination, TerminationReason::Exited);
    assert!(
        survived.exit_status.success(),
        "sibling exited {:?}; termination={:?}; cleanup={:?}; duration={:?}",
        survived.exit_status,
        survived.termination,
        survived.cleanup,
        survived.duration
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_terminates_root_and_job_owned_descendants() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let root_pid_file = directory.path().join("root-pid.txt");
    let pid_file = directory.path().join("child-pid.txt");
    let cancellation = CancellationToken::new();
    let mut process =
        process_fixture_request("spawn-child-and-wait", &root_pid_file, Some(&pid_file));
    process.cancellation = cancellation.clone();
    process.timeout = Some(Duration::from_secs(30));

    let mut run = tokio::spawn(ManagedProcessService::run(process));
    let child_pid = wait_for_pid_file(&pid_file, &mut run).await;
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
    let root_pid_file = directory.path().join("root-pid.txt");
    let pid_file = directory.path().join("child-pid.txt");
    let result = ManagedProcessService::run(process_fixture_request(
        "spawn-child-and-exit",
        &root_pid_file,
        Some(&pid_file),
    ))
    .await
    .expect("root exit should close its Job before joining inherited pipes");
    let child_pid = tokio::fs::read_to_string(&pid_file)
        .await
        .expect("PowerShell child PID file")
        .trim()
        .parse::<u32>()
        .expect("PowerShell child PID");

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
