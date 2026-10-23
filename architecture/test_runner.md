# Test Runner Module

The `test_runner` module provides test command resolution, output parsing,
report formatting, streaming process execution for supervised test runs,
and a bounded previous-failures index for automatic reruns.

## Purpose

Supervised test execution with structured output parsing,
managed-child timeout handling, lifecycle event publishing,
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
| `runner.rs` | Streaming runner over a managed child (timeout, stall, redaction, RunStore) |
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
4. **Execution**: `run_resolved_test()` launches the argv through
   `ManagedProcessService::run_streaming()` — never via a shell. The managed
   process owns the child: it applies the requested `SandboxProfile` (a
   `ReadOnly` profile is refused — test execution needs `WorkspaceWrite`),
   the `NetworkIntent`, an inherited-or-sanitized environment policy, and a
   32 MiB per-stream output cap with `ContinueDrain` overflow.
5. **Supervision**: A `tokio::select!` loop enforces wall-clock timeout
   (default 300s), no-output stall timeout (default 120s, polled every 5s),
   and emits throttled progress events through the `TestEventSink`.
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

- `TestJobExecutor` (`src/scheduler/executors.rs`) implements `JobExecutor` for
  `JobKind::Test`.
  It constructs a `TestScope::BashDispatch(argv)` from the job payload and delegates to
  `resolve_and_run_test()`.
- `src/tool/test.rs` submits jobs via `JobSubmissionService` and waits for completion.
- `src/tool/bash/process.rs` submits planner-validated `TestScope::BashDispatch` jobs.
- `src/tui/commands/test.rs` submits through `CoreRequest::JobSubmit` and waits.
- The scheduler owns admission and attempt lifecycle; TestRunner remains the domain
  authority for framework discovery, stall handling, reports, artifacts, and RunStore
  persistence.

## Key Types & APIs

### FailureClass (`types.rs:45-59`)

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

### TestRunRequest (`types.rs:121-139`)

```rust
pub struct TestRunRequest {
    pub scope: TestScope,
    pub workdir: PathBuf,
    pub execution_cwd: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
    pub stall_timeout_secs: Option<u64>,
    pub max_report_bytes: Option<usize>,
    pub session_id: Option<String>,
    pub parent_run_id: Option<AgentRunId>,
    pub cancellation: Option<CancellationToken>,
}
```

### ResolvedTestCommand (`types.rs:142-147`)

```rust
pub struct ResolvedTestCommand {
    pub language: TestLanguage,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub scope_label: String,
}
```

### TestFailure (`types.rs:150-157`)

```rust
pub struct TestFailure {
    pub name: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    pub failure_class: FailureClass,
}
```

### TestTimeout (`types.rs:160-164`)

```rust
pub struct TestTimeout {
    pub kind: TimeoutKind,
    pub elapsed_ms: u64,
    pub last_output: Option<String>,
}
```

### TestReport (`types.rs:167-184`)

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

### TestEventSink trait (`types.rs:217-221`)

```rust
pub trait TestEventSink: Send + Sync {
    fn started(&self, snapshot: TestRunStartedSnapshot);
    fn progress(&self, snapshot: TestRunProgressSnapshot);
    fn completed(&self, snapshot: TestRunCompletedSnapshot);
}
```

Snapshot types carry `session_id`, `job_id`, and event-specific fields.

### DelegatedTestRun (`src/test_runner/runner.rs`)

Returned by `run_resolved_test` and `resolve_and_run_test`. The `run_id` is `Some`
when the canonical TestRunner successfully began a `RunKind::Test` record; `None`
when no record could be begun or no `RunStore` was provided. This is the
**record-ownership contract**: callers suppress duplicate persistence only when
`run_id` is present.

### TestResolveError (`src/test_runner/resolve.rs`)

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

### TestRunError (`src/test_runner/runner.rs`)

```rust
pub enum TestRunError {
    Resolve(TestResolveError),
    LogDir(io::Error),
    Process(ManagedProcessError),
    LogWrite(io::Error),
    EmptyCommand,
    InvalidRequest(String),
    RunStore(String),
}
```

`Process` wraps `crate::managed_process::ManagedProcessError` — the launch,
containment, and termination failures of the managed child. `RunStore` carries
the stringified store error: a persistence failure is surfaced to the scheduler
rather than swallowed, because an accepted rerun must not be reported as a
successful, untraceable child.

### TestParseState (`parse.rs:21-30`)

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
pub fn resolve_validated_custom_command(cmd: &str) -> Result<ValidatedCustomCommand>
pub fn has_cargo_manifest(workdir: &Path) -> bool
pub fn has_python_test_markers(workdir: &Path) -> bool
pub fn detect_language_for_auto(workdir: &Path) -> Result<TestLanguage>

// runner.rs
pub async fn resolve_and_run_test(...) -> Result<DelegatedTestRun, TestRunError>
pub async fn resolve_and_run_test_with_profile(...) -> Result<DelegatedTestRun, TestRunError>
pub async fn resolve_and_run_test_with_profile_and_network(...) -> Result<DelegatedTestRun, TestRunError>
pub async fn run_resolved_test(...) -> Result<DelegatedTestRun, TestRunError>
pub async fn run_resolved_test_with_profile(...) -> Result<DelegatedTestRun, TestRunError>
pub async fn run_resolved_test_with_profile_and_network(...) -> Result<DelegatedTestRun, TestRunError>

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

### Runner constants (`src/test_runner/runner.rs`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `DEFAULT_TIMEOUT_SECS` | 300 | Wall-clock timeout |
| `DEFAULT_STALL_TIMEOUT_SECS` | 120 | No-output stall timeout |
| `DEFAULT_MAX_REPORT_BYTES` | 20,000 | Report body cap |
| `STALL_CHECK_INTERVAL` | 5s | Polling interval for stall detection |
| `TEST_PROCESS_OUTPUT_CAP` | 32 MiB | Per-stream stdout/stderr cap on the managed child |

### Formatter constants (`src/test_runner/report.rs`)

| Constant | Value | Purpose |
|----------|-------|---------|
| `MAX_DISPLAY_FAILURES` | 5 | Primary failures shown in report |
| `MAX_FAILURE_MESSAGE_BYTES` | 2000 | Truncation limit per failure message |
| `MAX_TIMEOUT_EXCERPT_BYTES` | 2000 | Truncation limit for timeout last_output |
| `DEFAULT_MAX_REPORT_BYTES` | 20,000 | Total report body cap |

### Index constants (`src/test_runner/index.rs`)

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

### Process ownership: the managed child, not a raw `tokio::process`

The runner no longer spawns a bare `tokio::process::Command` and no longer
manages a process group itself. It builds a
`crate::managed_process::ManagedProcessRequest` and runs it through
`ManagedProcessService::run_streaming()`, so timeout, cancellation,
containment, output caps, and reaping are owned by the shared managed-process
subsystem rather than by `src/test_runner/`.

What the runner does own:

- **Sandbox profile.** `SandboxProfile::ReadOnly` is refused outright
  ("test execution requires WorkspaceWrite authority"). A profile that
  requires filesystem containment builds its launch spec through
  `crate::security::sandbox::sandbox_config_for_durable_payload()` while the
  config is still being assembled — asking afterwards produced a spec with no
  captured identity, and the launch failed closed.
- **Network intent.** `NetworkIntent::LegacyUnrestricted` by default; the
  scheduler may pass a stricter intent.
- **Environment policy.** Inherited by default; sanitized when the profile
  requires filesystem containment.
- **Output caps.** 32 MiB per stream with `OverflowPolicy::ContinueDrain`.
  Exceeding the cap is reported as `ChildFailed("managed output limit
  exceeded")`.
- **Cancellation.** The request's `CancellationToken` cancels the managed
  child; the supervisor classifies the resulting termination as
  `WallClockTimeout`, `StallTimeout`, or `Cancelled` depending on why it
  stopped.

**Non-Unix fallback:** no longer applies — the managed process owns the
child on every platform, so the old "grandchildren can outlive the timeout"
caveat is a property of the managed-process subsystem, not of this module.

### Previous-failures index (legacy)

The legacy `.codegg/test-runs/index.json` is retained for backward compatibility.
RunStore is the authoritative persistence layer. The legacy index is deprecated
and will be removed once `PreviousFailures` reads from RunStore directly.

Writing is serialized via `OnceLock<tokio::sync::Mutex<()>>` (`src/test_runner/index.rs`).
Atomic writes use a per-process `.tmp` + rename (`write_index_atomic()` in the
same file).

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

The resolver (`resolve_validated_custom_command()` in
`src/test_runner/resolve.rs`) re-runs the strict validator as defense-in-depth.
Empty input is mapped to the legacy `EmptyCustomCommand` variant so existing
callers keep working.

`TestScope::BashDispatch` is the one scope that does **not** re-validate: its
argv was already validated by the planner, and the resolver consumes it
directly (using `request.execution_cwd` when present, otherwise the workdir).

## Testing

Narrowest run:

```bash
cargo test -p codegg --lib test_runner
```

Submodule targeting:

```bash
cargo test -p codegg --lib test_runner::custom    # 29 tests
cargo test -p codegg --lib test_runner::parse      # 23 tests
cargo test -p codegg --lib test_runner::report     # 12 tests
cargo test -p codegg --lib test_runner::runner     # 17 tests
cargo test -p codegg --lib test_runner::index      # 17 tests
cargo test -p codegg --lib test_runner::projection # 21 tests
cargo test -p codegg --lib test_runner::resolve    # 16 tests
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

## Source Verification

Verified 2026-10-06 against source. Corrected: `TestRunRequest` 6 → 9 fields
(added `execution_cwd`, `parent_run_id`, `cancellation`). Line refs (the prior
"off by 96-117 lines" estimate was far too large — real drift was +11 to +13 on
the later `types.rs` structs): `FailureClass` `44-59` → `45-59`,
`TestRunRequest` `121-128` → `121-139`, `ResolvedTestCommand` `131-136` →
`142-147`, `TestFailure` `138-146` → `150-157`, `TestTimeout` `148-153` →
`160-164`, `TestReport` `156-173` → `167-184`, `TestEventSink` `204-208` →
`217-221`, `TestResolveError` `resolve.rs:11-34` → `12-34`, `TestRunError`
`runner.rs:30-58` → `31-61`, `TestParseState` `parse.rs:20-30` → `21-30`,
`DelegatedTestRun` `runner.rs:356-368` → `356-359`, index constants
`index.rs:13-17` → `14-18`, `setsid` `runner.rs:290-297` → `386-391`,
`libc::kill` `runner.rs:478-486` → `591-608`, index lock `index.rs:112` →
`113`, atomic write `index.rs:149-160` → `194-199`,
`resolve_validated_custom_command` `resolve.rs:229-237` → `232`; per-module
test counts replaced with measured values (custom 29, parse 25, report 12,
runner 8, index 17, projection 21, resolve 15); the BashDispatch submitter is
`src/tool/bash/process.rs`, not `src/tool/bash.rs`. Verified accurate: 10
files, 13 `FailureClass` variants, the four enums at `types.rs:5/22/29/38`,
`ResolvedTestCommand` 4 fields, `TestFailure` 5, `TestTimeout` 3, `TestReport`
14, `TestParseState` 8, `TestScope` 8 variants, 12-entry allowlist, runner
constants (`runner.rs:24-28`), formatter constants, index constant values,
`TestJobExecutor` (`src/scheduler/executors.rs:68`/`83`).

Verified 2026-10-23 against `src/test_runner/{types,resolve,runner,parse,report,index,custom,bus_sink,projection}.rs`,
`src/scheduler/executors.rs`, and the root `Cargo.toml` test targets.
Corrected:

- **The process model was wrong.** The document claimed the runner spawns a
  bare `tokio::process::Command`, puts the child in its own session via
  `setsid()` in `pre_exec`, and kills the tree with
  `libc::kill(-pgid, SIGKILL)`, with a "non-Unix fallback" caveat. None of
  that is in the source any more: `run_resolved_test*()` builds a
  `crate::managed_process::ManagedProcessRequest` and runs it through
  `ManagedProcessService::run_streaming()`. The section is now titled "Process
  ownership: the managed child, not a raw `tokio::process`" and documents what
  the runner actually owns — sandbox profile (with `ReadOnly` refused),
  network intent, environment policy, the 32 MiB per-stream output cap, and
  cancellation.
- **`TestRunError` rewritten.** The old variant list (`Spawn`, `StdoutPipe`,
  `StderrPipe`, `ProcessWait`) does not exist. The current enum is
  `Resolve`, `LogDir`, `Process(ManagedProcessError)`, `LogWrite`,
  `EmptyCommand`, `InvalidRequest(String)`, `RunStore(String)`.
- **Runner constants corrected.** `GRACEFUL_KILL_TIMEOUT` is gone (it belonged
  to the removed raw-spawn path); `TEST_PROCESS_OUTPUT_CAP` (32 MiB) was
  missing and is now listed.
- **Test counts re-measured** with a Rust-aware scan of the `#[test]` /
  `#[tokio::test]` attributes: parse 25 → 23, runner 8 → 17, resolve 15 → 16.
  custom 29, index 17, projection 21, and report 12 are unchanged.
- **Stale line refs replaced by symbol names**: `DelegatedTestRun`,
  `TestResolveError`, `TestRunError`, the runner/formatter/index constant
  blocks, `resolve_validated_custom_command()`, the index lock, and the atomic
  write.
- **`TestJobExecutor` line refs dropped** (`executors.rs:68`/`:83` had drifted;
  the `impl JobExecutor` block is now at `:112`).
- **API surface completed**: `resolve_validated_custom_command()` and the
  `*_with_profile` / `*_with_profile_and_network` variants of
  `resolve_and_run_test()` and `run_resolved_test()` are now listed, and the
  `BashDispatch` scope's explicit no-re-validation contract is stated in
  "Validation re-run".

Verified accurate: 10 files; 13 `FailureClass` variants with `Display`,
`from_display_str()`, `as_str()`, and `Hash`; `TestScope`'s 8 variants and the
`BashDispatch` bypass; the four scalar enums; the `TestRunRequest` (9 fields),
`ResolvedTestCommand` (4), `TestFailure` (5), `TestTimeout` (3), `TestReport`
(14), and `TestParseState` (8) field lists; the `TestEventSink` trait shape;
the 12-entry allowlist and its argv-token-bounded matching; the formatter and
index constant values; the language-detection rules (including the
`tests/`-must-contain-`.py` refinement that keeps Rust workspaces out of
`AmbiguousEcosystem`); the `Resolve` → `Reject` backends; the record-ownership
contract; and the legacy-index status.
