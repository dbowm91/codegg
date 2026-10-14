use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::test_runner::parse::{ingest_stderr_line, ingest_stdout_line, TestParseState};
use crate::test_runner::resolve::{resolve_test_command, TestResolveError};
use crate::test_runner::types::{
    FailureClass, ResolvedTestCommand, TestEventSink, TestReport, TestRunCompletedSnapshot,
    TestRunProgressSnapshot, TestRunRequest, TestRunStartedSnapshot, TestStatus, TestTimeout,
    TimeoutKind,
};

const DEFAULT_TIMEOUT_SECS: u64 = 300;
const DEFAULT_STALL_TIMEOUT_SECS: u64 = 120;
const DEFAULT_MAX_REPORT_BYTES: usize = 20_000;
const STALL_CHECK_INTERVAL: Duration = Duration::from_secs(5);
const TEST_PROCESS_OUTPUT_CAP: usize = 32 * 1024 * 1024;

#[derive(Error, Debug)]
pub enum TestRunError {
    #[error(transparent)]
    Resolve(#[from] TestResolveError),

    #[error("failed to create log directory: {0}")]
    LogDir(io::Error),

    #[error("managed test process failed: {0}")]
    Process(#[from] crate::managed_process::ManagedProcessError),

    #[error("failed to write to log file: {0}")]
    LogWrite(io::Error),

    #[error("empty command vector")]
    EmptyCommand,

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("failed to persist test run: {0}")]
    RunStore(String),
}

/// Context for event publishing during test supervision.
struct SupervisorContext<'a> {
    sink: Option<&'a dyn TestEventSink>,
    job_id: &'a str,
    session_id: &'a str,
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

fn elapsed_ms_saturating(start: &Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn usize_to_u64_saturating(v: usize) -> u64 {
    u64::try_from(v).unwrap_or(u64::MAX)
}

fn redact_text(text: &str) -> String {
    crate::git_network_policy::redact_url_credentials_in_text(text)
}

fn redact_bytes(data: Vec<u8>) -> Vec<u8> {
    let text = String::from_utf8_lossy(&data);
    redact_text(&text).into_bytes()
}

/// Read at most `cap` bytes from `path` without loading a runaway log
/// fully into memory. Oversized logs return a head/tail window with a
/// truncation marker so the persisted size stays bounded by `cap`.
fn read_capped_bytes(path: &Path, cap: usize) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let cap_u64 = u64::try_from(cap).unwrap_or(u64::MAX);
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    if cap == 0 {
        return Ok(Vec::new());
    }
    if len <= cap_u64 {
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        // Defensive: the file may have grown between metadata and read.
        if buf.len() > cap {
            buf.truncate(cap);
        }
        return Ok(buf);
    }
    let marker = format!("\n... [truncated {len} bytes total, showing head/tail] ...\n");
    let marker_bytes = marker.as_bytes();
    if cap <= marker_bytes.len() + 16 {
        let mut buf = vec![0u8; cap];
        file.read_exact(&mut buf)?;
        return Ok(buf);
    }
    let budget = cap - marker_bytes.len();
    let head_len = budget / 2;
    let tail_len = budget - head_len;
    let mut out = Vec::with_capacity(cap);
    let mut head = vec![0u8; head_len];
    file.read_exact(&mut head)?;
    out.extend_from_slice(&head);
    out.extend_from_slice(marker_bytes);
    let tail_start = len.saturating_sub(u64::try_from(tail_len).unwrap_or(u64::MAX));
    file.seek(SeekFrom::Start(tail_start))?;
    let mut tail = vec![0u8; tail_len];
    file.read_exact(&mut tail)?;
    out.extend_from_slice(&tail);
    Ok(out)
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
    sink: Option<&dyn TestEventSink>,
    run_store: Option<&Arc<dyn codegg_core::run_store::RunStore>>,
) -> Result<DelegatedTestRun, TestRunError> {
    run_resolved_test_with_profile(request, resolved, sink, run_store, None).await
}

pub async fn run_resolved_test_with_profile(
    request: &TestRunRequest,
    resolved: ResolvedTestCommand,
    sink: Option<&dyn TestEventSink>,
    run_store: Option<&Arc<dyn codegg_core::run_store::RunStore>>,
    sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
) -> Result<DelegatedTestRun, TestRunError> {
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

    // Keep the raw argv confined to the process-launch boundary. Reports,
    // events, the legacy index, and RunStore records all receive the typed
    // audit form, including URL credential redaction.
    let mut audit_resolved = resolved.clone();
    audit_resolved.argv = codegg_git::AuditSafeArgv::from_argv(resolved.argv.clone()).into_inner();

    // Create the durable child record before launching the process. In the
    // scheduler path this makes an accepted rerun visible as Running even if
    // the daemon or child process dies before completion.
    let run_handle = if let Some(store) = run_store {
        Some(
            begin_test_run(store, request, &audit_resolved)
                .await
                .map_err(|e| TestRunError::RunStore(e.to_string()))?,
        )
    } else {
        None
    };

    let job_id = Uuid::new_v4().to_string();
    let command_display = audit_resolved.argv.join(" ");

    if let Some(sink) = sink {
        sink.started(TestRunStartedSnapshot {
            session_id: request.session_id.clone().unwrap_or_default(),
            job_id: job_id.clone(),
            command: command_display,
            cwd: resolved.cwd.to_string_lossy().to_string(),
        });
    }

    let start = Instant::now();
    let shared = SharedState::new();
    let stdout_log = std::fs::File::create(&stdout_log_path).map_err(TestRunError::LogWrite)?;
    let stderr_log = std::fs::File::create(&stderr_log_path).map_err(TestRunError::LogWrite)?;
    let ctx = SupervisorContext {
        sink,
        job_id: &job_id,
        session_id: request.session_id.as_deref().unwrap_or(""),
    };
    let result = run_managed_test_process(
        request,
        &resolved,
        sandbox_profile,
        timeout_secs,
        stall_timeout_secs,
        start,
        &shared,
        stdout_log,
        stderr_log,
        &ctx,
    )
    .await?;

    let elapsed_ms = elapsed_ms_saturating(&start);
    let parse_state = shared.parse_state.lock().await.clone();

    let report = build_report(
        &result,
        &audit_resolved,
        elapsed_ms,
        &parse_state,
        &log_dir,
        &stdout_log_path,
        &stderr_log_path,
        max_report_bytes,
    );

    let report_json = serde_json::to_string_pretty(&report).unwrap_or_default();
    let report_path = log_dir.join("report.json");
    if let Err(e) = std::fs::write(&report_path, report_json) {
        tracing::warn!(
            error = %e,
            path = %report_path.display(),
            "failed to write test run report.json"
        );
    }

    // RunStore is the authoritative persistence layer for test runs.
    // The legacy .codegg/test-runs/index.json is retained for backward compatibility
    // with TestScope::PreviousFailures and TUI commands that read the index directly.
    //
    // Append to the previous-failures index (best-effort; index write failure
    // must not fail the test run).
    crate::test_runner::index::append_to_index(&report, &request.workdir).await;

    // Persist the terminal result to RunStore. A persistence failure is
    // surfaced to the scheduler: an accepted rerun must not be reported as a
    // successful, untraceable child.
    let run_id = if let (Some(store), Some(handle)) = (run_store, run_handle) {
        Some(
            persist_to_run_store(
                store,
                handle,
                request,
                &report,
                &audit_resolved,
                max_report_bytes,
            )
            .await
            .map_err(|e| TestRunError::RunStore(e.to_string()))?,
        )
    } else {
        None
    };

    if let Some(sink) = sink {
        sink.completed(TestRunCompletedSnapshot {
            session_id: request.session_id.clone().unwrap_or_default(),
            job_id,
            status: format!("{:?}", report.status).to_lowercase(),
            summary: report.summary.clone(),
            log_dir: report
                .log_dir
                .as_ref()
                .map(|p| p.to_string_lossy().to_string()),
            run_id: run_id.as_ref().map(ToString::to_string),
            parent_run_id: request.parent_run_id.as_ref().map(ToString::to_string),
        });
    }

    Ok(DelegatedTestRun { report, run_id })
}

/// Result of a delegated test run. The `run_id` is `Some` iff a `RunStore`
/// record was successfully begun — it is the proof of delegated ownership.
#[derive(Debug, Clone)]
pub struct DelegatedTestRun {
    pub report: TestReport,
    pub run_id: Option<codegg_core::run_store::RunId>,
}

impl DelegatedTestRun {
    pub fn into_report(self) -> TestReport {
        self.report
    }
    pub fn report(&self) -> &TestReport {
        &self.report
    }
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
                    // Redact URL credentials before persisting to the log
                    // file, before parser ingestion (which feeds the
                    // model-safe TestReport), and before the stall/timeout
                    // excerpt. Raw output never reaches durable surfaces.
                    let lossy_line = String::from_utf8_lossy(line.as_bytes()).to_string();
                    let redacted = redact_text(&lossy_line);
                    let _ = writeln_raw_line(&mut log_file, &redacted);

                    let mut state = shared.parse_state.lock().await;
                    if is_stdout {
                        ingest_stdout_line(&mut state, &redacted);
                    } else {
                        ingest_stderr_line(&mut state, &redacted);
                    }
                    drop(state);

                    shared.record_output(&redacted).await;
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
    Cancelled,
    ChildFailed(String),
}

#[allow(clippy::too_many_arguments)]
async fn run_managed_test_process(
    request: &TestRunRequest,
    resolved: &ResolvedTestCommand,
    sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
    timeout_secs: u64,
    stall_timeout_secs: u64,
    start: Instant,
    shared: &SharedState,
    stdout_log: std::fs::File,
    stderr_log: std::fs::File,
    ctx: &SupervisorContext<'_>,
) -> Result<SupervisorResult, TestRunError> {
    use std::ffi::OsString;

    if sandbox_profile == Some(codegg_core::approval::SandboxProfile::ReadOnly) {
        return Err(TestRunError::InvalidRequest(
            "test execution requires WorkspaceWrite authority".into(),
        ));
    }
    let argv: Vec<OsString> = resolved.argv.iter().map(OsString::from).collect();
    let mut process = crate::managed_process::ManagedProcessRequest::new(
        argv,
        resolved.cwd.clone(),
        crate::managed_process::ProcessProvenance::new(
            "test-runner",
            request.session_id.clone().unwrap_or_default(),
        ),
    );
    process.environment_policy =
        if sandbox_profile.is_some_and(|profile| profile.requires_filesystem_containment()) {
            crate::managed_process::EnvironmentPolicy::sanitized()
        } else {
            crate::managed_process::EnvironmentPolicy::inherited()
        };
    process.timeout = Some(Duration::from_secs(timeout_secs));
    process.output_policy = crate::managed_process::OutputPolicy {
        stdout_limit: TEST_PROCESS_OUTPUT_CAP,
        stderr_limit: TEST_PROCESS_OUTPUT_CAP,
        overflow: crate::managed_process::OverflowPolicy::ContinueDrain,
    };
    process.required_sandbox_profile = sandbox_profile;
    if let Some(profile) =
        sandbox_profile.filter(|profile| profile.requires_filesystem_containment())
    {
        let config =
            crate::security::sandbox::sandbox_config_for_profile(profile, &request.workdir)
                .ok_or_else(|| {
                    TestRunError::InvalidRequest(format!(
                    "filesystem containment is unavailable for accepted {profile:?} test profile"
                ))
                })?;
        process.sandbox = crate::managed_process::SandboxRequest::Required(
            config
                .launch_spec(&resolved.argv[0], &resolved.argv[1..], Some(&resolved.cwd))
                .map_err(|error| TestRunError::InvalidRequest(error.to_string()))?,
        );
    }

    let (stdout_writer, stdout_reader) = tokio::io::duplex(64 * 1024);
    let (stderr_writer, stderr_reader) = tokio::io::duplex(64 * 1024);
    let stdout_task = spawn_reader_task(stdout_reader, stdout_log, shared.clone(), true);
    let stderr_task = spawn_reader_task(stderr_reader, stderr_log, shared.clone(), false);
    let (output_tx, mut output_rx) =
        mpsc::channel::<crate::managed_process::ManagedProcessOutputChunk>(128);
    let bridge_task = tokio::spawn(async move {
        let mut stdout_writer = stdout_writer;
        let mut stderr_writer = stderr_writer;
        while let Some(chunk) = output_rx.recv().await {
            let writer = match chunk.stream {
                crate::managed_process::OutputStream::Stdout => &mut stdout_writer,
                crate::managed_process::OutputStream::Stderr => &mut stderr_writer,
            };
            if writer.write_all(&chunk.bytes).await.is_err() {
                break;
            }
        }
    });

    let process_cancellation = tokio_util::sync::CancellationToken::new();
    process.cancellation = process_cancellation.clone();
    let mut managed =
        Box::pin(crate::managed_process::ManagedProcessService::run_streaming(process, output_tx));
    let mut stop_reason = None;
    let mut last_progress = Instant::now();
    let mut last_tests_seen = 0;
    let mut last_tests_failed = 0;
    let mut had_failure = false;
    let result = loop {
        tokio::select! {
            result = &mut managed => break result?,
            _ = tokio::time::sleep(STALL_CHECK_INTERVAL) => {
                if stall_timeout_secs > 0 {
                    let last_at = *shared.last_output_at.lock().await;
                    if last_at.elapsed() > Duration::from_secs(stall_timeout_secs) {
                        stop_reason = Some(SupervisorStopReason::Stall);
                        process_cancellation.cancel();
                        break managed.await?;
                    }
                }
                if let Some(sink) = ctx.sink {
                    if last_progress.elapsed() >= Duration::from_millis(500) {
                        let state = shared.parse_state.lock().await;
                        let tests_seen = usize_to_u64_saturating(state.tests_seen);
                        let tests_failed = usize_to_u64_saturating(state.tests_failed);
                        let tests_passed = usize_to_u64_saturating(state.tests_passed);
                        drop(state);
                        let changed = tests_seen != last_tests_seen
                            || tests_failed != last_tests_failed
                            || (tests_failed > 0 && !had_failure);
                        if changed {
                            let mut parts = Vec::new();
                            if tests_seen > 0 { parts.push(format!("{tests_seen} tests seen")); }
                            if tests_passed > 0 { parts.push(format!("{tests_passed} passed")); }
                            if tests_failed > 0 { parts.push(format!("{tests_failed} failed")); }
                            if parts.is_empty() { parts.push("running tests...".into()); }
                            sink.progress(TestRunProgressSnapshot {
                                session_id: ctx.session_id.to_string(),
                                job_id: ctx.job_id.to_string(),
                                message: parts.join(", "),
                            });
                            last_tests_seen = tests_seen;
                            last_tests_failed = tests_failed;
                            had_failure = tests_failed > 0;
                            last_progress = Instant::now();
                        }
                    }
                }
            }
            _ = async {
                match request.cancellation.as_ref() {
                    Some(token) => token.cancelled().await,
                    None => std::future::pending().await,
                }
            } => {
                stop_reason = Some(SupervisorStopReason::Cancelled);
                process_cancellation.cancel();
                break managed.await?;
            }
        }
    };
    let _ = bridge_task.await;
    let _ = stdout_task.await;
    let _ = stderr_task.await;

    let elapsed_ms = elapsed_ms_saturating(&start);
    let last_output = shared.last_output_excerpt.lock().await.clone();
    Ok(match result.termination {
        crate::managed_process::TerminationReason::Exited => SupervisorResult::Completed {
            exit_code: result.exit_status.code(),
        },
        crate::managed_process::TerminationReason::TimedOut => SupervisorResult::WallClockTimeout {
            elapsed_ms,
            last_output,
        },
        crate::managed_process::TerminationReason::Cancelled => match stop_reason {
            Some(SupervisorStopReason::Stall) => SupervisorResult::StallTimeout {
                elapsed_ms,
                last_output,
            },
            Some(SupervisorStopReason::Cancelled) | None => SupervisorResult::Cancelled,
        },
        crate::managed_process::TerminationReason::OutputLimitExceeded { .. } => {
            SupervisorResult::ChildFailed("managed output limit exceeded".into())
        }
    })
}

#[derive(Clone, Copy)]
enum SupervisorStopReason {
    Stall,
    Cancelled,
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
                last_output: last_output.clone().map(|s| redact_text(&s)),
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
                last_output: last_output.clone().map(|s| redact_text(&s)),
            };
            (TestStatus::TimedOut, None, Some(timeout))
        }
        SupervisorResult::Cancelled => (TestStatus::Cancelled, None, None),
        SupervisorResult::ChildFailed(msg) => {
            let timeout = TestTimeout {
                kind: TimeoutKind::WallClock,
                elapsed_ms,
                last_output: Some(redact_text(&format!("child process error: {msg}"))),
            };
            (TestStatus::Error, None, Some(timeout))
        }
    };

    let mut failures: Vec<_> = parse_state
        .failures
        .iter()
        .chain(parse_state.compile_errors.iter())
        .cloned()
        .map(|mut f| {
            f.message = redact_text(&f.message);
            f
        })
        .collect();

    if failures.is_empty() && exit_code != Some(0) && status != TestStatus::Passed {
        failures.push(crate::test_runner::types::TestFailure {
            name: None,
            file: None,
            line: None,
            message: format!("process exited with code {}", exit_code.unwrap_or(-1)),
            failure_class: FailureClass::NonzeroExit,
        });
    }

    let summary = redact_text(&build_summary(parse_state, exit_code));

    let output_truncated = {
        let total_bytes = std::fs::metadata(stdout_log).map(|m| m.len()).unwrap_or(0)
            + std::fs::metadata(stderr_log).map(|m| m.len()).unwrap_or(0);
        total_bytes > u64::try_from(max_report_bytes).unwrap_or(u64::MAX)
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
        scope_label: Some(resolved.scope_label.clone()),
        previous_run_id: None,
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

pub async fn resolve_and_run_test(
    request: TestRunRequest,
    sink: Option<&dyn TestEventSink>,
    run_store: Option<&Arc<dyn codegg_core::run_store::RunStore>>,
) -> Result<DelegatedTestRun, TestRunError> {
    resolve_and_run_test_with_profile(request, sink, run_store, None).await
}

pub async fn resolve_and_run_test_with_profile(
    request: TestRunRequest,
    sink: Option<&dyn TestEventSink>,
    run_store: Option<&Arc<dyn codegg_core::run_store::RunStore>>,
    sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
) -> Result<DelegatedTestRun, TestRunError> {
    let resolved = resolve_test_command(&request)?;
    run_resolved_test_with_profile(&request, resolved, sink, run_store, sandbox_profile).await
}

/// Begin and complete a test run in the RunStore. The begin step is separate
/// so the scheduler has a durable Running child before process launch; the
/// completion step adds artifacts and the rerun descriptor.
async fn begin_test_run(
    store: &Arc<dyn codegg_core::run_store::RunStore>,
    request: &TestRunRequest,
    resolved: &ResolvedTestCommand,
) -> Result<codegg_core::run_store::RunHandle, codegg_core::error::RunStoreError> {
    use codegg_core::run_store::*;

    let draft = RunDraft {
        kind: RunKind::Test,
        invocation: RunInvocation {
            command: resolved.argv.join(" "),
            argv: Some(resolved.argv.clone()),
            script_hash: None,
        },
        session_id: request.session_id.clone(),
        parent_run_id: request.parent_run_id.clone(),
        workspace_root: request.workdir.clone(),
        cwd: resolved.cwd.clone(),
        backend: BackendRecord {
            family: "test_runner".to_string(),
            detail: Some(resolved.scope_label.clone()),
        },
        risk: RiskRecord {
            level: "low".to_string(),
            has_subprocess: true,
            has_git_mutation: false,
            has_destructive_mutation: false,
        },
        // Workstream A: TestRunner is the canonical delegated backend for
        // test runs. The runner owns the record and BashTool must NOT persist
        // its own record (BashTool checks `ownership_for_outcome()`).
        planned_backend: Some(codegg_core::run_store::PlannedBackend::TestRunner),
        actual_backend: Some(codegg_core::run_store::ActualBackend::TestRunner),
        ownership: request
            .parent_run_id
            .clone()
            .map(codegg_core::run_store::RunOwnership::ChildOf)
            .unwrap_or(codegg_core::run_store::RunOwnership::DelegatedBackend),
        asset_provenance: None,
    };

    store.begin_run(draft).await
}

async fn persist_to_run_store(
    store: &Arc<dyn codegg_core::run_store::RunStore>,
    handle: codegg_core::run_store::RunHandle,
    request: &TestRunRequest,
    report: &TestReport,
    resolved: &ResolvedTestCommand,
    max_report_bytes: usize,
) -> Result<codegg_core::run_store::RunId, codegg_core::error::RunStoreError> {
    use codegg_core::run_store::*;

    let status = match report.status {
        TestStatus::Passed => RunStatus::Complete,
        TestStatus::Failed => RunStatus::Failed,
        TestStatus::TimedOut => RunStatus::TimedOut,
        TestStatus::Error => RunStatus::Failed,
        TestStatus::Cancelled => RunStatus::Cancelled,
    };

    // Bound persisted output by `max_report_bytes` (BUG-002) and redact
    // URL credentials defense-in-depth (BUG-001). Logs on disk are
    // already redacted at write time; this covers pre-existing raw logs
    // and keeps `write_artifact` from loading GB-scale files fully.
    // Never persist more than the RunStore absolute limit either.
    let artifact_cap = std::cmp::min(
        max_report_bytes,
        usize::try_from(codegg_core::run_store::MAX_ARTIFACT_BYTES).unwrap_or(usize::MAX),
    );

    // Write stdout artifact
    if let Some(ref stdout_path) = report.stdout_log {
        if let Ok(data) = read_capped_bytes(stdout_path, artifact_cap) {
            if !data.is_empty() {
                let data = redact_bytes(data);
                let _ = store
                    .write_artifact(
                        &handle,
                        ArtifactInput {
                            kind: ArtifactKind::Stdout,
                            data,
                            mime_type: "text/plain".to_string(),
                            // Workstream G: raw stdout is NOT model-safe.
                            // Only the test-report JSON (below) and any
                            // post-redaction projection are model-safe.
                            safe_for_model: false,
                        },
                    )
                    .await;
            }
        }
    }

    // Write stderr artifact
    if let Some(ref stderr_path) = report.stderr_log {
        if let Ok(data) = read_capped_bytes(stderr_path, artifact_cap) {
            if !data.is_empty() {
                let data = redact_bytes(data);
                let _ = store
                    .write_artifact(
                        &handle,
                        ArtifactInput {
                            kind: ArtifactKind::Stderr,
                            data,
                            mime_type: "text/plain".to_string(),
                            // Workstream G: raw stderr is NOT model-safe.
                            safe_for_model: false,
                        },
                    )
                    .await;
            }
        }
    }

    // Write test report JSON artifact
    if let Ok(report_json) = serde_json::to_vec_pretty(report) {
        let _ = store
            .write_artifact(
                &handle,
                ArtifactInput {
                    kind: ArtifactKind::TestReport,
                    data: report_json,
                    mime_type: "application/json".to_string(),
                    // TestReport is structured and safe to surface to the model.
                    safe_for_model: true,
                },
            )
            .await;
    }

    // Build rerun descriptor so can_rerun works from the TUI. Test
    // argv is already credential-free, but we route through
    // `AuditSafeArgv` to keep the type-level invariant (every
    // RerunDescriptor.argv is audit-safe) uniform.
    let rerun = Some(RerunDescriptor {
        argv: Some(codegg_git::AuditSafeArgv::from_argv(resolved.argv.clone())),
        script_source_ref: None,
        backend_family: "test_runner".to_string(),
        cwd: resolved.cwd.clone(),
        workspace_root: request.workdir.clone(),
        mode: Some(resolved.scope_label.clone()),
        config_profile: None,
        parent_run_id: request.parent_run_id.clone(),
    });

    store
        .complete_run(
            handle.clone(),
            RunCompletion {
                status,
                completed_at: Utc::now(),
                permissions: vec![],
                sandbox: None,
                projection: None,
                changes: vec![],
                rerun,
                // Workstream A: actual backend is TestRunner (canonical).
                actual_backend: Some(codegg_core::run_store::ActualBackend::TestRunner),
                fallback: None,
            },
        )
        .await
        .map(|_| handle.run_id)
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
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = run_resolved_test(&request, resolved, None, None)
            .await
            .unwrap()
            .into_report();
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

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread")]
    async fn constrained_test_profile_writes_only_inside_workspace() {
        let dir = temp_dir_with_files("constrained-profile", &[]);
        let outside = dir.path().with_extension("outside-marker");
        let inside = dir.path().join("inside-marker");
        let command = format!(
            "touch '{}' && (touch '{}' >/dev/null 2>&1 || true)",
            inside.display(),
            outside.display()
        );
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec!["/bin/sh".into(), "-c".into(), command],
            cwd: dir.path().to_path_buf(),
            scope_label: "workspace".into(),
        };
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            execution_cwd: None,
            timeout_secs: Some(30),
            stall_timeout_secs: Some(10),
            max_report_bytes: None,
            session_id: None,
            parent_run_id: None,
            cancellation: None,
        };
        let result = run_resolved_test_with_profile(
            &request,
            resolved,
            None,
            None,
            Some(codegg_core::approval::SandboxProfile::WorkspaceWrite),
        )
        .await;
        if crate::security::sandbox::platform_sandbox_capability().is_available() {
            let report = result
                .expect("available workspace sandbox should run the test")
                .into_report();
            assert_eq!(report.status, TestStatus::Passed);
            assert!(inside.exists());
            assert!(!outside.exists(), "test process escaped its workspace root");
        } else {
            assert!(
                result.is_err(),
                "unavailable containment must refuse launch"
            );
            assert!(!inside.exists());
            assert!(!outside.exists());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resolve_and_run_failing_command() {
        let dir = temp_dir_with_files("fail-cmd", &[]);
        let request = TestRunRequest {
            scope: TestScope::CustomCommand("cargo test --bogus-flag".into()),
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(10),
            stall_timeout_secs: None,
            max_report_bytes: None,
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = resolve_and_run_test(request, None, None)
            .await
            .unwrap()
            .into_report();
        assert_eq!(result.status, TestStatus::Failed);
        assert!(result.exit_code.unwrap_or(0) != 0);
        assert!(!result.failures.is_empty());
        assert_eq!(result.failures[0].failure_class, FailureClass::NonzeroExit);
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
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = run_resolved_test(&request, resolved, None, None)
            .await
            .unwrap()
            .into_report();
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
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = run_resolved_test(&request, resolved, None, None)
            .await
            .unwrap()
            .into_report();
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
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec![],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(run_resolved_test(&request, resolved, None, None));
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
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec!["echo".into(), "hi".into()],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt.block_on(run_resolved_test(&request, resolved, None, None));
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
        let state = TestParseState {
            tests_seen: 10,
            tests_passed: 8,
            tests_failed: 2,
            ..Default::default()
        };

        let summary = build_summary(&state, Some(1));
        assert_eq!(summary, "8 passed, 2 failed");
    }

    #[test]
    fn build_summary_no_tests_exits() {
        let state = TestParseState::default();
        assert_eq!(build_summary(&state, Some(0)), "completed successfully");
        assert_eq!(build_summary(&state, Some(1)), "process exited with code 1");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runner_report_uses_parser_failures_for_nonzero_exit() {
        let dir = temp_dir_with_files("runner-fail", &[]);
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "echo 'FAIL: bad test' && exit 1".into(),
            ],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(10),
            stall_timeout_secs: None,
            max_report_bytes: None,
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = run_resolved_test(&request, resolved, None, None)
            .await
            .unwrap()
            .into_report();
        assert_eq!(result.status, TestStatus::Failed);
        assert_eq!(result.exit_code, Some(1));
        assert_eq!(result.failures[0].failure_class, FailureClass::NonzeroExit);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runner_timeout_report_includes_last_output_excerpt() {
        let dir = temp_dir_with_files("runner-timeout", &[]);
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "echo 'starting tests...' && sleep 10".into(),
            ],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(1),
            stall_timeout_secs: None,
            max_report_bytes: None,
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = run_resolved_test(&request, resolved, None, None)
            .await
            .unwrap()
            .into_report();
        assert_eq!(result.status, TestStatus::TimedOut);
        let timeout = result.timeout.unwrap();
        assert!(timeout.last_output.is_some());
    }

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct TestSink {
        started_called: AtomicBool,
        progress_count: AtomicUsize,
        completed_called: AtomicBool,
    }

    impl TestSink {
        fn new() -> Self {
            Self {
                started_called: AtomicBool::new(false),
                progress_count: AtomicUsize::new(0),
                completed_called: AtomicBool::new(false),
            }
        }
    }

    impl crate::test_runner::types::TestEventSink for TestSink {
        fn started(&self, _snapshot: crate::test_runner::types::TestRunStartedSnapshot) {
            self.started_called.store(true, Ordering::SeqCst);
        }
        fn progress(&self, _snapshot: crate::test_runner::types::TestRunProgressSnapshot) {
            self.progress_count.fetch_add(1, Ordering::SeqCst);
        }
        fn completed(&self, _snapshot: crate::test_runner::types::TestRunCompletedSnapshot) {
            self.completed_called.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runner_publishes_started_and_completed_when_sink_present() {
        let dir = temp_dir_with_files("sink-present", &[]);
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
            session_id: Some("test-session".into()),
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let sink = TestSink::new();
        let result = run_resolved_test(&request, resolved, Some(&sink), None)
            .await
            .unwrap()
            .into_report();
        assert_eq!(result.status, TestStatus::Passed);
        assert!(sink.started_called.load(Ordering::SeqCst));
        assert!(sink.completed_called.load(Ordering::SeqCst));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn runner_does_not_require_sink() {
        let dir = temp_dir_with_files("no-sink", &[]);
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
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = run_resolved_test(&request, resolved, None, None).await;
        assert!(result.is_ok());
    }

    /// Confirms the timeout path still produces a `TestReport` after the
    /// managed process owner terminates and reaps the process tree.
    #[tokio::test(flavor = "multi_thread")]
    async fn runner_timeout_path_still_returns_report_after_kill() {
        let dir = temp_dir_with_files("kill-fallback", &[]);
        let resolved = ResolvedTestCommand {
            language: crate::test_runner::types::TestLanguage::Generic,
            argv: vec![
                "/bin/sh".into(),
                "-c".into(),
                "trap '' TERM; sleep 10".into(),
            ],
            cwd: dir.path().to_path_buf(),
            scope_label: "test".into(),
        };
        let request = TestRunRequest {
            scope: TestScope::Auto,
            workdir: dir.path().to_path_buf(),
            timeout_secs: Some(1),
            stall_timeout_secs: None,
            max_report_bytes: None,
            session_id: None,
            parent_run_id: None,
            execution_cwd: None,
            cancellation: None,
        };
        let result = run_resolved_test(&request, resolved, None, None)
            .await
            .unwrap()
            .into_report();
        assert_eq!(result.status, TestStatus::TimedOut);
        assert!(result.timeout.is_some());
        assert!(result.log_dir.is_some());
    }
}
