use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::test_runner::parse::{ingest_stderr_line, ingest_stdout_line, TestParseState};
use crate::test_runner::resolve::{resolve_test_command, TestResolveError};
use crate::test_runner::types::{
    ResolvedTestCommand, TestReport, TestRunRequest, TestStatus, TestTimeout, TimeoutKind,
};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_STALL_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_REPORT_BYTES: usize = 20_000;
const STALL_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const GRACEFUL_KILL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Error, Debug)]
pub enum TestRunError {
    #[error(transparent)]
    Resolve(#[from] TestResolveError),

    #[error("failed to create log directory: {0}")]
    LogDir(io::Error),

    #[error("failed to spawn process: {0}")]
    Spawn(io::Error),

    #[error("stdout pipe unavailable: {0}")]
    StdoutPipe(io::Error),

    #[error("stderr pipe unavailable: {0}")]
    StderrPipe(io::Error),

    #[error("failed to write to log file: {0}")]
    LogWrite(io::Error),

    #[error("process wait failed: {0}")]
    ProcessWait(String),

    #[error("empty command vector")]
    EmptyCommand,

    #[error("invalid request: {0}")]
    InvalidRequest(String),
}

#[derive(Debug, Clone)]
struct SharedState {
    parse_state: Arc<Mutex<TestParseState>>,
    last_output_at: Arc<Mutex<Instant>>,
    last_output_excerpt: Arc<Mutex<Option<String>>>,
}

impl SharedState {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            parse_state: Arc::new(Mutex::new(TestParseState::default())),
            last_output_at: Arc::new(Mutex::new(now)),
            last_output_excerpt: Arc::new(Mutex::new(None)),
        }
    }

    async fn record_output(&self, line: &str) {
        let mut last_at = self.last_output_at.lock().await;
        *last_at = Instant::now();
        drop(last_at);

        let mut excerpt = self.last_output_excerpt.lock().await;
        *excerpt = Some(truncate_utf8(line, 200));
    }
}

fn truncate_utf8(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        let mut end = max_chars;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

fn create_log_dir(workdir: &Path) -> Result<PathBuf, TestRunError> {
    let now = Utc::now().format("%Y%m%dT%H%M%SZ");
    let short_uuid = &Uuid::new_v4().to_string()[..8];
    let dir_name = format!("{now}-{short_uuid}");
    let log_dir = workdir.join(".codegg").join("test-runs").join(&dir_name);

    std::fs::create_dir_all(&log_dir).map_err(TestRunError::LogDir)?;

    Ok(log_dir)
}

pub async fn run_resolved_test(
    request: &TestRunRequest,
    resolved: ResolvedTestCommand,
) -> Result<TestReport, TestRunError> {
    let timeout_secs = request.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if timeout_secs == 0 {
        return Err(TestRunError::InvalidRequest(
            "timeout_secs must be > 0".into(),
        ));
    }
    let stall_timeout_secs = request
        .stall_timeout_secs
        .unwrap_or(DEFAULT_STALL_TIMEOUT_SECS);
    let max_report_bytes = request.max_report_bytes.unwrap_or(DEFAULT_MAX_REPORT_BYTES);

    if resolved.argv.is_empty() {
        return Err(TestRunError::EmptyCommand);
    }

    let log_dir = create_log_dir(&request.workdir)?;
    let stdout_log_path = log_dir.join("stdout.log");
    let stderr_log_path = log_dir.join("stderr.log");

    let start = Instant::now();
    let shared = SharedState::new();

    let mut child = spawn_child(&resolved)?;

    let stdout_pipe = child
        .stdout
        .take()
        .ok_or_else(|| TestRunError::StdoutPipe(io::Error::other("stdout not piped")))?;
    let stderr_pipe = child
        .stderr
        .take()
        .ok_or_else(|| TestRunError::StderrPipe(io::Error::other("stderr not piped")))?;

    let stdout_log = std::fs::File::create(&stdout_log_path).map_err(TestRunError::LogWrite)?;
    let stderr_log = std::fs::File::create(&stderr_log_path).map_err(TestRunError::LogWrite)?;

    let stdout_task = spawn_reader_task(stdout_pipe, stdout_log, shared.clone(), true);
    let stderr_task = spawn_reader_task(stderr_pipe, stderr_log, shared.clone(), false);

    let wall_clock_deadline = start + Duration::from_secs(timeout_secs);
    let stall_interval = if stall_timeout_secs == 0 {
        Duration::from_secs(u64::MAX)
    } else {
        STALL_CHECK_INTERVAL
    };
    let stall_deadline_secs = stall_timeout_secs;

    let result = supervisor_loop(
        &mut child,
        stdout_task,
        stderr_task,
        &shared,
        wall_clock_deadline,
        stall_interval,
        stall_deadline_secs,
        start,
    )
    .await;

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let parse_state = shared.parse_state.lock().await.clone();

    let report = build_report(
        &result,
        &resolved,
        elapsed_ms,
        &parse_state,
        &log_dir,
        &stdout_log_path,
        &stderr_log_path,
        max_report_bytes,
    );

    let report_json = serde_json::to_string_pretty(&report).unwrap_or_default();
    let report_path = log_dir.join("report.json");
    let _ = std::fs::write(&report_path, report_json);

    Ok(report)
}

fn spawn_child(resolved: &ResolvedTestCommand) -> Result<Child, TestRunError> {
    let mut cmd = Command::new(&resolved.argv[0]);
    if resolved.argv.len() > 1 {
        cmd.args(&resolved.argv[1..]);
    }
    cmd.current_dir(&resolved.cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    cmd.spawn().map_err(TestRunError::Spawn)
}

fn spawn_reader_task(
    reader: impl tokio::io::AsyncRead + Unpin + Send + 'static,
    mut log_file: std::fs::File,
    shared: SharedState,
    is_stdout: bool,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let buf_reader = BufReader::new(reader);
        let mut lines = buf_reader.lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let _ = writeln_raw_line(&mut log_file, &line);

                    let lossy_line = String::from_utf8_lossy(line.as_bytes()).to_string();
                    let mut state = shared.parse_state.lock().await;
                    if is_stdout {
                        ingest_stdout_line(&mut state, &lossy_line);
                    } else {
                        ingest_stderr_line(&mut state, &lossy_line);
                    }
                    drop(state);

                    shared.record_output(&lossy_line).await;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }
    })
}

fn writeln_raw_line(log_file: &mut std::fs::File, line: &str) -> Result<(), io::Error> {
    use io::Write;
    log_file.write_all(line.as_bytes())?;
    log_file.write_all(b"\n")?;
    log_file.flush()
}

enum SupervisorResult {
    Completed {
        exit_code: Option<i32>,
    },
    WallClockTimeout {
        elapsed_ms: u64,
        last_output: Option<String>,
    },
    StallTimeout {
        elapsed_ms: u64,
        last_output: Option<String>,
    },
    ChildFailed(String),
}

async fn supervisor_loop(
    child: &mut Child,
    mut stdout_task: tokio::task::JoinHandle<()>,
    mut stderr_task: tokio::task::JoinHandle<()>,
    shared: &SharedState,
    wall_clock_deadline: Instant,
    stall_interval: Duration,
    stall_deadline_secs: u64,
    start: Instant,
) -> SupervisorResult {
    let mut stdout_done = false;
    let mut stderr_done = false;

    loop {
        tokio::select! {
            status = child.wait() => {
                if !stdout_done {
                    let _ = stdout_task.await;
                }
                if !stderr_done {
                    let _ = stderr_task.await;
                }
                return match status {
                    Ok(exit_status) => SupervisorResult::Completed {
                        exit_code: exit_status.code(),
                    },
                    Err(e) => SupervisorResult::ChildFailed(e.to_string()),
                };
            }
            _ = &mut stdout_task, if !stdout_done => {
                stdout_done = true;
            }
            _ = &mut stderr_task, if !stderr_done => {
                stderr_done = true;
            }
            _ = tokio::time::sleep_until(wall_clock_deadline.into()) => {
                let elapsed = start.elapsed().as_millis() as u64;
                let excerpt = shared.last_output_excerpt.lock().await.clone();
                kill_child(child).await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return SupervisorResult::WallClockTimeout {
                    elapsed_ms: elapsed,
                    last_output: excerpt,
                };
            }
            _ = tokio::time::sleep(stall_interval) => {
                if stall_deadline_secs == 0 {
                    continue;
                }
                let last_at = *shared.last_output_at.lock().await;
                let since_last = last_at.elapsed();
                if since_last > Duration::from_secs(stall_deadline_secs) {
                    let elapsed = start.elapsed().as_millis() as u64;
                    let excerpt = shared.last_output_excerpt.lock().await.clone();
                    kill_child(child).await;
                    let _ = stdout_task.await;
                    let _ = stderr_task.await;
                    return SupervisorResult::StallTimeout {
                        elapsed_ms: elapsed,
                        last_output: excerpt,
                    };
                }
            }
        }
    }
}

async fn kill_child(child: &mut Child) {
    let _ = child.kill().await;
    let _ = tokio::time::timeout(GRACEFUL_KILL_TIMEOUT, child.wait()).await;
}

fn build_report(
    result: &SupervisorResult,
    resolved: &ResolvedTestCommand,
    elapsed_ms: u64,
    parse_state: &TestParseState,
    log_dir: &Path,
    stdout_log: &Path,
    stderr_log: &Path,
    max_report_bytes: usize,
) -> TestReport {
    let (status, exit_code, timeout) = match result {
        SupervisorResult::Completed { exit_code } => {
            let status = if *exit_code == Some(0) {
                TestStatus::Passed
            } else {
                TestStatus::Failed
            };
            (status, *exit_code, None)
        }
        SupervisorResult::WallClockTimeout {
            elapsed_ms,
            last_output,
        } => {
            let timeout = TestTimeout {
                kind: TimeoutKind::WallClock,
                elapsed_ms: *elapsed_ms,
                last_output: last_output.clone(),
            };
            (TestStatus::TimedOut, None, Some(timeout))
        }
        SupervisorResult::StallTimeout {
            elapsed_ms,
            last_output,
        } => {
            let timeout = TestTimeout {
                kind: TimeoutKind::NoOutput,
                elapsed_ms: *elapsed_ms,
                last_output: last_output.clone(),
            };
            (TestStatus::TimedOut, None, Some(timeout))
        }
        SupervisorResult::ChildFailed(msg) => {
            let timeout = TestTimeout {
                kind: TimeoutKind::WallClock,
                elapsed_ms,
                last_output: Some(format!("child process error: {msg}")),
            };
            (TestStatus::Error, None, Some(timeout))
        }
    };

    let failure_class = if exit_code != Some(0) {
        if parse_state.compile_error_seen {
            "compile_error"
        } else if !parse_state.failures.is_empty() {
            "test_failed"
        } else {
            "nonzero_exit"
        }
    } else {
        ""
    };

    let mut failures = parse_state.failures.clone();
    if !failure_class.is_empty() && failures.is_empty() && exit_code != Some(0) {
        failures.push(crate::test_runner::types::TestFailure {
            name: None,
            file: None,
            line: None,
            message: format!("process exited with code {}", exit_code.unwrap_or(-1)),
            failure_class: failure_class.to_string(),
        });
    }

    let summary = build_summary(parse_state, exit_code);

    let output_truncated = {
        let total_bytes = std::fs::metadata(stdout_log).map(|m| m.len()).unwrap_or(0)
            + std::fs::metadata(stderr_log).map(|m| m.len()).unwrap_or(0);
        total_bytes > max_report_bytes as u64
    };

    TestReport {
        status,
        argv: resolved.argv.clone(),
        cwd: resolved.cwd.clone(),
        duration_ms: elapsed_ms,
        exit_code,
        summary,
        failures,
        timeout,
        log_dir: Some(log_dir.to_path_buf()),
        stdout_log: Some(stdout_log.to_path_buf()),
        stderr_log: Some(stderr_log.to_path_buf()),
        output_truncated,
    }
}

fn build_summary(parse_state: &TestParseState, exit_code: Option<i32>) -> String {
    if parse_state.tests_seen == 0 && parse_state.tests_passed == 0 && parse_state.tests_failed == 0
    {
        return match exit_code {
            Some(0) => "completed successfully".into(),
            Some(code) => format!("process exited with code {code}"),
            None => "process terminated without exit code".into(),
        };
    }

    let mut parts = Vec::new();
    if parse_state.tests_passed > 0 {
        parts.push(format!("{} passed", parse_state.tests_passed));
    }
    if parse_state.tests_failed > 0 {
        parts.push(format!("{} failed", parse_state.tests_failed));
    }
    if parts.is_empty() {
        format!("{} tests seen", parse_state.tests_seen)
    } else {
        parts.join(", ")
    }
}

pub async fn resolve_and_run_test(request: TestRunRequest) -> Result<TestReport, TestRunError> {
    let resolved = resolve_test_command(&request)?;
    run_resolved_test(&request, resolved).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_runner::types::TestScope;
    use std::fs;

    fn temp_dir_with_files(_name: &str, files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in files {
            let path = dir.path().join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, "").unwrap();
        }
        dir
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resolve_and_run_passing_test() {
        let dir = temp_dir_with_files("pass", &[]);
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec!["true".into()],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(60),
            stall_timeout_secs: Some(30),
            max_report_bytes: None,
        };
        let result = run_resolved_test(&request, resolved).await.unwrap();
        assert_eq!(result.status, TestStatus::Passed);
        assert_eq!(result.exit_code, Some(0));
        assert!(result.log_dir.is_some());
        assert!(result.stdout_log.is_some());
        assert!(result.stderr_log.is_some());

        let log_dir = result.log_dir.as_ref().unwrap();
        assert!(log_dir.join("stdout.log").exists());
        assert!(log_dir.join("stderr.log").exists());
        assert!(log_dir.join("report.json").exists());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resolve_and_run_failing_command() {
        let dir = temp_dir_with_files("fail-cmd", &[]);
        let request = TestRunRequest {
            scope: TestScope::CustomCommand("false".into()),
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(10),
            stall_timeout_secs: None,
            max_report_bytes: None,
        };
        let result = resolve_and_run_test(request).await.unwrap();
        assert_eq!(result.status, TestStatus::Failed);
        assert_eq!(result.exit_code, Some(1));
        assert!(!result.failures.is_empty());
        assert_eq!(result.failures[0].failure_class, "nonzero_exit");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resolve_and_run_times_out() {
        let dir = temp_dir_with_files("timeout", &[]);
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec!["/bin/sh".into(), "-c".into(), "sleep 10".into()],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(1),
            stall_timeout_secs: None,
            max_report_bytes: None,
        };
        let result = run_resolved_test(&request, resolved).await.unwrap();
        assert_eq!(result.status, TestStatus::TimedOut);
        assert!(result.timeout.is_some());
        let timeout = result.timeout.unwrap();
        assert_eq!(timeout.kind, TimeoutKind::WallClock);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resolve_and_run_stall_timeout() {
        let dir = temp_dir_with_files("stall", &[]);
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec!["/bin/sh".into(), "-c".into(), "sleep 10".into()],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(30),
            stall_timeout_secs: Some(1),
            max_report_bytes: None,
        };
        let result = run_resolved_test(&request, resolved).await.unwrap();
        assert_eq!(result.status, TestStatus::TimedOut);
        assert!(result.timeout.is_some());
        let timeout = result.timeout.unwrap();
        assert_eq!(timeout.kind, TimeoutKind::NoOutput);
    }

    #[test]
    fn empty_command_returns_error() {
        let dir = temp_dir_with_files("empty", &[]);
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(10),
            stall_timeout_secs: None,
            max_report_bytes: None,
        };
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec![],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(run_resolved_test(&request, resolved));
        assert!(matches!(err, Err(TestRunError::EmptyCommand)));
    }

    #[test]
    fn zero_timeout_returns_invalid_request() {
        let dir = temp_dir_with_files("zero-timeout", &[]);
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(0),
            stall_timeout_secs: None,
            max_report_bytes: None,
        };
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec!["echo".into(), "hi".into()],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(run_resolved_test(&request, resolved));
        assert!(matches!(err, Err(TestRunError::InvalidRequest(_))));
    }

    #[test]
    fn log_dir_layout_uses_timestamp_and_uuid() {
        let dir = temp_dir_with_files("log-layout", &[]);
        let log_dir = create_log_dir(dir.path()).unwrap();
        let rel = log_dir.strip_prefix(dir.path()).unwrap();
        let components: Vec<_> = rel.components().collect();
        assert!(components.len() >= 3);
        let dir_name = components.last().unwrap().as_os_str().to_string_lossy();
        assert!(dir_name.contains('T'));
        assert!(dir_name.len() > 16);
    }

    #[test]
    fn truncate_utf8_does_not_split_char_boundary() {
        let s = "hello world";
        assert_eq!(truncate_utf8(s, 5), "hello...");
        assert_eq!(truncate_utf8(s, 11), "hello world");
        assert_eq!(truncate_utf8(s, 20), "hello world");
        let s = "αβγδεζηθ";
        assert_eq!(truncate_utf8(s, 2), "α...");
        assert_eq!(truncate_utf8(s, 4), "αβ...");
        assert_eq!(truncate_utf8(s, 16), "αβγδεζηθ");
        assert_eq!(truncate_utf8("short", 100), "short");
    }

    #[test]
    fn build_summary_counts_from_parse_state() {
        let mut state = TestParseState::default();
        state.tests_seen = 10;
        state.tests_passed = 8;
        state.tests_failed = 2;

        let summary = build_summary(&state, Some(1));
        assert_eq!(summary, "8 passed, 2 failed");
    }

    #[test]
    fn build_summary_no_tests_exits() {
        let state = TestParseState::default();
        assert_eq!(build_summary(&state, Some(0)), "completed successfully");
        assert_eq!(build_summary(&state, Some(1)), "process exited with code 1");
    }
}
