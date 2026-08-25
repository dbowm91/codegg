# Test Runner Module

The `test_runner` module provides test command resolution, output parsing,
report formatting, streaming process execution for supervised test runs,
and a bounded previous-failures index for automatic reruns.

## Purpose

Supervised test execution with structured output parsing,
process-group-aware timeout handling, lifecycle event publishing,
and a bounded previous-failures index. The module is the domain
authority for test-run semantics; callers reach it exclusively
through the scheduler.

## Where It Lives

`src/test_runner/` (10 files)

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports |
| `types.rs` | Core types: enums, structs, `TestEventSink` trait, lifecycle snapshots |
| `resolve.rs` | Command resolver: maps `TestScope` → `ResolvedTestCommand` |
| `custom.rs` | Shared custom command allowlist (12 entries, argv-prefix bounded) |
| `parse.rs` | Line-by-line output parser with ANSI escape stripping |
| `report.rs` | Text formatter: `format_test_report()` |
| `runner.rs` | Streaming runner with process-group-aware log capture |
| `index.rs` | Bounded previous-failures index (legacy, superseded by RunStore) |
| `bus_sink.rs` | `BusEventSink` — bridges `TestEventSink` to `GlobalEventBus` |
| `projection.rs` | `test_report_to_projection()` + `compute_test_delta()` |

## How It Works

1. **Resolution**: `resolve_test_command()` maps `TestScope` to a concrete argv + cwd.
   Auto scope detects Rust (Cargo.toml) or Python (pyproject.toml / pytest.ini / tests/*.py).
   `Changed` scope is currently a fallback to `Auto` with a prefixed label.
2. **Custom validation**: `validate_custom_command()` enforces the 12-entry allowlist
   via argv-token-bounded prefix matching. Shell metacharacters, command substitution,
   quoting, and bidi Unicode control characters are all rejected.
3. **Defense-in-depth**: The resolver re-runs the strict validator before producing
   `ResolvedTestCommand.argv` — even if a presentation-boundary caller forgets to
   validate, the runner still rejects injection attempts.
4. **Execution**: `run_resolved_test()` spawns a tokio process via
   `Command::new(argv[0]).args(&argv[1..])` — never via shell. On Unix, the child is
   placed in its own session/process group via `setsid()` so timeout kills target the
   entire tree using `libc::kill(-pgid, SIGKILL)`.
5. **Supervision**: A `tokio::select!` loop enforces wall-clock timeout (default 300s),
   no-output stall timeout (default 120s), and emits throttled progress events through
   the `TestEventSink`.
6. **Parsing**: stdout/stderr lines are fed to `TestParseState` via `Arc<Mutex<>>`.
   ANSI escape sequences are stripped before pattern matching.
7. **Reporting**: `build_report()` classifies exit codes, panics, compile errors,
   and pytest failures into `FailureClass`. `format_test_report()` produces bounded
   model-facing text.
8. **Persistence**: `persist_to_run_store()` writes to RunStore (authoritative) and
   `append_to_index()` writes to `.codegg/test-runs/index.json` (legacy, deprecated).
9. **Event publishing**: `BusEventSink` bridges `TestEventSink` → `GlobalEventBus` →
   `CoreEvent::TestRun*` for remote client visibility.

### Scheduler integration

All production model-facing test execution flows through the scheduler:

- `TestJobExecutor` (`src/scheduler/executors.rs:68`) implements `JobExecutor` for `JobKind::Test`.
  It constructs a `TestScope::BashDispatch(argv)` from the job payload and delegates to
  `resolve_and_run_test()`.
- `src/tool/test.rs` submits jobs via `JobSubmissionService` and waits for completion.
- `src/tool/bash.rs` submits planner-validated `TestScope::BashDispatch` jobs.
- `src/tui/commands/test.rs` submits through `CoreRequest::JobSubmit` and waits.
- The scheduler owns admission and attempt lifecycle; TestRunner remains the domain
  authority for framework discovery, stall handling, reports, artifacts, and RunStore
  persistence.

## Key Types & APIs

### FailureClass (`types.rs:44-59`)

13 variants: `Passed`, `RustTestFailure`, `RustPanic`, `RustCompileError`,
`RustDoctestFailure`, `PytestFailure`, `PytestError`, `PytestCollectionError`,
`NonzeroExit`, `TimeoutWallClock`, `TimeoutNoOutput`, `SpawnError`, `UnknownFailure`.

Implements `Display` with snake_case strings. Has `from_display_str()` and `as_str()`.
Implements `Hash` for use in `HashSet`-based dedup (projection delta).

### TestScope (`types.rs:5-19`)

```rust
pub enum TestScope {
    Auto,
    Workspace,
    Changed,
    Package(String),
    File(PathBuf),
    PreviousFailures,
    CustomCommand(String),
    BashDispatch(Vec<String>),  // pre-validated argv from BashTool
}
```

`BashDispatch` is a pre-validated argv from BashTool's active routing dispatcher.
The test-runner safety validator does NOT re-run for BashDispatch.

### TestLanguage (`types.rs:22-26`)

`Rust | Python | Generic`

### TestStatus (`types.rs:29-35`)

`Passed | Failed | TimedOut | Cancelled | Error`

### TimeoutKind (`types.rs:38-42`)

`WallClock | NoOutput | NoProgress`

### TestRunRequest (`types.rs:121-128`)

```rust
pub struct TestRunRequest {
    pub scope: TestScope,
    pub workdir: PathBuf,
    pub timeout_secs: Option<u64>,
    pub stall_timeout_secs: Option<u64>,
    pub max_report_bytes: Option<usize>,
    pub session_id: Option<String>,
}
```

### ResolvedTestCommand (`types.rs:131-136`)

```rust
pub struct ResolvedTestCommand {
    pub language: TestLanguage,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub scope_label: String,
}
```

### TestFailure (`types.rs:138-146`)

```rust
pub struct TestFailure {
    pub name: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    pub failure_class: FailureClass,
}
```

### TestTimeout (`types.rs:148-153`)

```rust
pub struct TestTimeout {
    pub kind: TimeoutKind,
    pub elapsed_ms: u64,
    pub last_output: Option<String>,
}
```

### TestReport (`types.rs:156-173`)

```rust
pub struct TestReport {
    pub status: TestStatus,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub summary: String,
    pub failures: Vec<TestFailure>,
    pub timeout: Option<TestTimeout>,
    pub log_dir: Option<PathBuf>,
    pub stdout_log: Option<PathBuf>,
    pub stderr_log: Option<PathBuf>,
    pub output_truncated: bool,
    pub scope_label: Option<String>,
    pub previous_run_id: Option<String>,
}
```

### TestEventSink trait (`types.rs:204-208`)

```rust
pub trait TestEventSink: Send + Sync {
    fn started(&self, snapshot: TestRunStartedSnapshot);
    fn progress(&self, snapshot: TestRunProgressSnapshot);
    fn completed(&self, snapshot: TestRunCompletedSnapshot);
}
```

Snapshot types carry `session_id`, `job_id`, and event-specific fields.

### DelegatedTestRun (`runner.rs:260-272`)

Returned by `run_resolved_test` and `resolve_and_run_test`. The `run_id` is `Some`
when the canonical TestRunner successfully began a `RunKind::Test` record; `None`
when no record could be begun or no `RunStore` was provided. This is the
**record-ownership contract**: callers suppress duplicate persistence only when
`run_id` is present.

### TestResolveError (`resolve.rs:11-34`)

```rust
pub enum TestResolveError {
    MissingWorkdir,
    MissingPackageName,
    MissingFilePath,
    EmptyCustomCommand,
    CustomCommandInvalid(CustomCommandValidationError),
    AmbiguousEcosystem,
    UnsupportedEcosystem(String),
    UnsupportedScopeForEcosystem { scope: &'static str, language: TestLanguage },
    PreviousFailures(TestIndexError),
}
```

### TestRunError (`runner.rs:30-58`)

```rust
pub enum TestRunError {
    Resolve(TestResolveError),
    LogDir(io::Error),
    Spawn(io::Error),
    StdoutPipe(io::Error),
    StderrPipe(io::Error),
    LogWrite(io::Error),
    ProcessWait(String),
    EmptyCommand,
    InvalidRequest(String),
}
```

### TestParseState (`parse.rs:20-30`)

```rust
pub struct TestParseState {
    pub language: Option<TestLanguage>,
    pub tests_seen: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub last_progress_line: Option<String>,
    pub failures: Vec<TestFailure>,
    pub compile_errors: Vec<TestFailure>,
    pub collection_error_seen: bool,
}
```

### Free functions

```rust
// resolve.rs
pub fn resolve_test_command(request: &TestRunRequest) -> Result<ResolvedTestCommand>
pub fn has_cargo_manifest(workdir: &Path) -> bool
pub fn has_python_test_markers(workdir: &Path) -> bool
pub fn detect_language_for_auto(workdir: &Path) -> Result<TestLanguage>

// runner.rs
pub async fn resolve_and_run_test(...) -> Result<DelegatedTestRun, TestRunError>
pub async fn run_resolved_test(...) -> Result<DelegatedTestRun, TestRunError>

// parse.rs
pub fn ingest_stdout_line(state: &mut TestParseState, line: &str)
pub fn ingest_stderr_line(state: &mut TestParseState, line: &str)
pub fn failure_class_summary(
    failures: &[TestFailure],
    compile_errors: &[TestFailure],
) -> FailureClass

// report.rs
pub fn format_test_report(report: &TestReport) -> String
pub fn format_test_report_with_cap(report: &TestReport, max_report_bytes: usize) -> String

// projection.rs
pub fn test_report_to_projection(report: &TestReport) -> ProjectionResult
pub fn test_report_to_projection_with_delta(
    report: &TestReport,
    previous: Option<&TestReport>,
) -> ProjectionResult
pub fn compute_test_delta(current: &TestReport, previous: &TestReport) -> TestDelta
```

## Configuration Surface

### Runner constants (`runner.rs:24-28`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `DEFAULT_TIMEOUT_SECS` | 300 | Wall-clock timeout |
| `DEFAULT_STALL_TIMEOUT_SECS` | 120 | No-output stall timeout |
| `DEFAULT_MAX_REPORT_BYTES` | 20,000 | Report body cap |
| `STALL_CHECK_INTERVAL` | 5s | Polling interval for stall detection |
| `GRACEFUL_KILL_TIMEOUT` | 3s | Wait after SIGKILL before reporting |

### Formatter constants (`report.rs:3-8`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `MAX_DISPLAY_FAILURES` | 5 | Primary failures shown in report |
| `MAX_FAILURE_MESSAGE_BYTES` | 2000 | Truncation limit per failure message |
| `MAX_TIMEOUT_EXCERPT_BYTES` | 2000 | Truncation limit for timeout last_output |
| `DEFAULT_MAX_REPORT_BYTES` | 20,000 | Total report body cap |

### Index constants (`index.rs:13-17`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `INDEX_VERSION` | 1 | Schema version |
| `MAX_INDEX_ENTRIES` | 100 | Max runs retained |
| `MAX_FAILURE_ENTRIES_PER_RUN` | 10 | Max failures per run entry |
| `MAX_MESSAGE_PREVIEW_BYTES` | 500 | Truncation for failure messages in index |
| `MAX_SUMMARY_BYTES` | 1000 | Truncation for summaries in index |

### Language detection (`resolve.rs`)

- **Rust**: `workdir.join("Cargo.toml").exists()`
- **Python**: `pyproject.toml`, `pytest.ini`, `tox.ini`, `noxfile.py`,
  or `tests/` directory containing `.py` files
- **Ambiguous**: both Rust and Python markers → `AmbiguousEcosystem` error

## Invariants & Gotchas

### Shell execution

**Never via shell.** Both generated and custom commands execute as direct argv via
`Command::new(argv[0]).args(&argv[1..])`. The custom validator rejects shell
metacharacters (`;`, `&&`, `||`, `|`, `>`, `<`, backticks, `$()`, `${}`, quotes,
globs, tilde, `#`, `!`, control characters, bidi Unicode) and enforces
argv-token-bounded prefix matching (so `pytestevil` does NOT match `pytest`).

### Process-group cleanup (Unix only)

On Unix, the child is placed in its own session/process group via `setsid()`
in `pre_exec` (`runner.rs:290-297`). Timeout kills target the entire process
tree using `libc::kill(-pgid, SIGKILL)` (`runner.rs:478-486`).

**Non-Unix fallback**: `spawn_child` skips `setsid()`, and `kill_child` falls
back to `child.kill().await`, which only kills the direct child. Grandchildren
can outlive the timeout. This is a known limitation.

### Previous-failures index (legacy)

The legacy `.codegg/test-runs/index.json` is retained for backward compatibility.
RunStore is the authoritative persistence layer. The legacy index is deprecated
and will be removed once `PreviousFailures` reads from RunStore directly.

Writing is serialized via `OnceLock<tokio::sync::Mutex<()>>` (`index.rs:112`).
Atomic writes use `.tmp` + rename (`index.rs:149-160`).

### Stale completion protection

The TUI `/test` command uses `AsyncUiRequestState` for stale-completion protection.
Each `/test` invocation calls `begin()` to allocate a monotonically increasing
request ID; `finish(request_id)` returns `false` (silently dropping the result)
if the request has been superseded.

### Record-ownership contract

`DelegatedTestRun.run_id` is `Some` when the runner successfully began a RunStore
record. Callers check `run_id` to suppress duplicate persistence. The runner calls
`ownership_for_outcome()` with `PlannedBackend::TestRunner` and
`ActualBackend::TestRunner` — BashTool must NOT persist its own record.

### Validation re-run

The resolver (`resolve_validated_custom_command` at `resolve.rs:229-237`) re-runs
the strict validator as defense-in-depth. Empty input is mapped to the legacy
`EmptyCustomCommand` variant so existing callers keep working.

## Testing

Narrowest run:

```bash
cargo test -p codegg --lib test_runner
```

Submodule targeting:

```bash
cargo test -p codegg --lib test_runner::custom    # 30+ tests
cargo test -p codegg --lib test_runner::parse      # 22 tests
cargo test -p codegg --lib test_runner::report     # 10 tests
cargo test -p codegg --lib test_runner::runner     # 13+ tests
cargo test -p codegg --lib test_runner::index      # 18 tests
cargo test -p codegg --lib test_runner::projection # 11 tests
cargo test -p codegg --lib test_runner::resolve    # 11 tests
```

Integration test suites:

```bash
cargo test -p codegg-protocol -- core_event_test_run
cargo test -p codegg --lib core::tests::test_run
```

## Related Docs

- `architecture/command_intent.md` — how commands are classified and routed
- `architecture/scheduler.md` — scheduler admission and attempt lifecycle
- `architecture/human_shell.md` — projection pipeline
- `architecture/tool_programs.md` — tool program execution (separate subsystem)
