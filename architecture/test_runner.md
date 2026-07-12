# Test Runner Module

The `test_runner` module provides test command resolution, output parsing, report formatting, streaming process execution for supervised test runs, and a bounded previous-failures index for automatic reruns.

## Overview

**Location**: `src/test_runner/`

**Files**:
| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports |
| `types.rs` | Core types with serde derives |
| `resolve.rs` | Command resolver |
| `custom.rs` | Shared custom command allowlist |
| `parse.rs` | Line-by-line output parser with ANSI escape stripping |
| `report.rs` | Text formatter |
| `runner.rs` | Streaming runner with process-group-aware log capture |
| `index.rs` | Bounded previous-failures index (Phase 06) |
| `bus_sink.rs` | `BusEventSink` — bridges `TestEventSink` trait to `GlobalEventBus` |
| `projection.rs` | `test_report_to_projection()` — converts `TestReport` to `ProjectionResult` (Phase 03) |

**Key Responsibilities**:
- Resolve `TestScope` into platform-specific shell commands
- Spawn and supervise test processes with streaming stdout/stderr capture
- Parse stdout/stderr into structured results with failure-class taxonomy
- Capture raw logs to `.codegg/test-runs/` directory
- Classify exit codes, panics, compile errors, and pytest failures
- Format `TestReport` into bounded, stable, model-facing text
- Maintain a bounded previous-failures index for automatic reruns
- Publish lifecycle events (`TestRunStarted`, `TestRunProgress`, `TestRunCompleted`) to `GlobalEventBus` for remote client visibility via the core protocol

**Phase**: 4 (Failure Extraction and Report Quality)
**Projection Adapter**: Phase 03 (`projection.rs`)

## Key Types

### FailureClass

```rust
pub enum FailureClass {
    Passed,
    RustTestFailure,
    RustPanic,
    RustCompileError,
    RustDoctestFailure,
    PytestFailure,
    PytestError,
    PytestCollectionError,
    NonzeroExit,
    TimeoutWallClock,
    TimeoutNoOutput,
    SpawnError,
    UnknownFailure,
}
```

Implements `Display` with snake_case strings. Has `from_str()` and `as_str()` helpers.

### TestScope

```rust
pub enum TestScope {
    Auto,
    Workspace,
    Changed,
    Package(String),
    File(PathBuf),
    PreviousFailures,
    CustomCommand(String),
    BashDispatch(Vec<String>),
}
```

`BashDispatch` is a pre-validated argv from BashTool's active routing dispatcher. The argv has already passed the planner's classification and validation, so the test-runner safety validator does NOT re-run (which would reject non-allowlisted test commands). Handled in `src/test_runner/resolve.rs:60-65`.

### TestLanguage

```rust
pub enum TestLanguage {
    Rust,
    Python,
    Generic,
}
```

### TestStatus

```rust
pub enum TestStatus {
    Passed,
    Failed,
    TimedOut,
    Cancelled,
    Error,
}
```

### TimeoutKind

```rust
pub enum TimeoutKind {
    WallClock,
    NoOutput,
    NoProgress,
}
```

### TestRunRequest

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

### ResolvedTestCommand

```rust
pub struct ResolvedTestCommand {
    pub language: TestLanguage,
    pub argv: Vec<String>,
    pub cwd: PathBuf,
    pub scope_label: String,
}
```

### TestFailure

```rust
pub struct TestFailure {
    pub name: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message: String,
    pub failure_class: FailureClass,
}
```

### TestTimeout

```rust
pub struct TestTimeout {
    pub kind: TimeoutKind,
    pub elapsed_ms: u64,
    pub last_output: Option<String>,
}
```

### TestReport

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

## Canonical Delegation Entry Point

### DelegatedTestRun

```rust
// src/test_runner/runner.rs:258-270
pub struct DelegatedTestRun {
    pub report: TestReport,
    pub run_id: Option<RunId>,
}
```

Returned by `run_resolved_test` and `resolve_and_run_test`. The `run_id` is `Some` when the canonical TestRunner persisted a `RunKind::Test` record; `None` when persistence failed or no `RunStore` was provided. This is the **proof-of-persistence contract**: callers use `run_id` to determine whether to suppress their own persistence.

```rust
impl DelegatedTestRun {
    pub fn into_report(self) -> TestReport { self.report }
    pub fn report(&self) -> &TestReport { &self.report }
}
```

### persist_to_run_store

`persist_to_run_store` in `src/test_runner/runner.rs:622` now returns `Option<RunId>` — `Some` on successful `complete_run` that yields the canonical run identity, `None` on failure (logged, non-fatal).

### Callers

All callers of `resolve_and_run_test` / `run_resolved_test` now call `.into_report()` to extract the `TestReport`:

- `src/tool/test.rs` (model-facing test tool)
- `src/tui/commands/test.rs:110` (TUI `/test` slash command)
- Internal tests in `src/test_runner/runner.rs`

Callers that suppress persistence (e.g., `BashTool::dispatch_to_test_runner`) inspect the `run_id` field on `DelegatedTestRun` before deciding whether to persist their own outer record.

### Called by BashTool

`BashTool::dispatch_to_test_runner` at `src/tool/bash.rs:542-572` constructs a `TestScope::BashDispatch(argv)` from the planner's validated argv, calls `resolve_and_run_test` with the shared `RunStore`, and uses the returned `DelegatedTestRun` to determine persistence ownership.

```bash
cargo test -p codegg --lib tool::bash
```

## Resolver API

### resolve_test_command

```rust
pub fn resolve_test_command(request: &TestRunRequest) -> Result<ResolvedTestCommand, TestResolveError>
```

Resolves a `TestScope` into a concrete command (`argv` + `cwd`). For `Auto` scope, detects language via `detect_language_for_auto`.

### Helper functions

```rust
pub fn has_cargo_manifest(workdir: &Path) -> bool
pub fn has_python_test_markers(workdir: &Path) -> bool
pub fn detect_language_for_auto(workdir: &Path) -> Result<TestLanguage, TestResolveError>
```

### TestResolveError

```rust
pub enum TestResolveError {
    MissingWorkdir,
    MissingPackageName,
    MissingFilePath,
    EmptyCustomCommand,
    AmbiguousEcosystem,
    UnsupportedEcosystem(String),
    UnsupportedScopeForEcosystem { scope: &'static str, language: TestLanguage },
    PreviousFailures(TestIndexError),
}
```

### TestIndexError

```rust
pub enum TestIndexError {
    IndexMissing(PathBuf),
    IndexUnreadable(PathBuf, io::Error),
    IndexMalformed(PathBuf, serde_json::Error),
    NoPreviousFailures(String),
    CommandInvalid(String),
}
```

## Custom Command Allowlist

The `custom.rs` module defines the shared allowlist used by both the model-facing `test` tool and the `/test` slash command. The allowlist is **argv-prefix based**, NOT raw-string-prefix based. Each entry is a sequence of argv tokens; a custom command is accepted only if it tokenizes into an argv vector whose prefix matches one of the allowed `argv_prefix` sequences exactly.

```rust
pub struct AllowedTestCommand {
    pub label: &'static str,
    pub argv_prefix: &'static [&'static str],
}

pub const CUSTOM_COMMAND_ALLOWLIST: &[AllowedTestCommand] = &[
    AllowedTestCommand { label: "cargo test",     argv_prefix: &["cargo", "test"] },
    AllowedTestCommand { label: "cargo nextest",  argv_prefix: &["cargo", "nextest"] },
    AllowedTestCommand { label: "pytest",         argv_prefix: &["pytest"] },
    AllowedTestCommand { label: "uv run pytest",  argv_prefix: &["uv", "run", "pytest"] },
    AllowedTestCommand { label: "go test",        argv_prefix: &["go", "test"] },
    AllowedTestCommand { label: "zig build test", argv_prefix: &["zig", "build", "test"] },
    AllowedTestCommand { label: "make test",      argv_prefix: &["make", "test"] },
    AllowedTestCommand { label: "make check",     argv_prefix: &["make", "check"] },
    AllowedTestCommand { label: "npm test",       argv_prefix: &["npm", "test"] },
    AllowedTestCommand { label: "pnpm test",      argv_prefix: &["pnpm", "test"] },
    AllowedTestCommand { label: "yarn test",      argv_prefix: &["yarn", "test"] },
    AllowedTestCommand { label: "bun test",       argv_prefix: &["bun", "test"] },
];

pub fn validate_custom_command(cmd: &str)
    -> Result<ValidatedCustomCommand, CustomCommandValidationError>

pub fn is_allowed_custom_command(cmd: &str) -> bool
```

### Validation contract

The strict validator enforces these invariants on every custom command string:

1. **Reject empty / whitespace-only input** — returns `Empty`.
2. **Reject forbidden shell syntax** — returns `ForbiddenShellSyntax` if the input contains any of:
   - Shell control operators: `;`, `&&`, `||`, `&`, `|`, `>`, `<`
   - Command substitution: `` ` ``, `$(`, `${`
   - Quoting: `'`, `"`, `\`
   - Grouping / redirection: `(`, `)`, `{`, `}`, `[`, `]`
   - Globbing: `*`, `?`
   - Expansion / history: `~`, `#`, `!`
   - Newlines, carriage returns, NUL bytes, and other ASCII control characters
   - Bidirectional Unicode control characters (U+200E–U+200F, U+202A–U+202E, U+2066–U+2069)
3. **Tokenize as whitespace-separated argv** — quote handling is intentionally absent. If a user wants to pass a literal space, they must use a different scope.
4. **Match the token prefix against the allowlist** — argv-token-bounded match, so `pytestevil` is NOT a hit for `pytest` and `cargo testify` is NOT a hit for `cargo test`.
5. **Return the validated argv vector** — the validator returns a `ValidatedCustomCommand { argv, label }` ready for direct `Command::new(argv[0]).args(&argv[1..])` execution.

### Defense-in-depth re-validation

Both the model-facing `test` tool and the TUI `/test` slash command call `validate_custom_command` at the presentation boundary. As a defense-in-depth measure, the resolver (`src/test_runner/resolve.rs::resolve_validated_custom_command`) **also** re-runs the strict validator before producing `ResolvedTestCommand.argv`. The resolver never accepts raw text into argv — even if a caller forgets to validate, the resolver still rejects shell metacharacters, redirection, command substitution, and allowlist-prefix smuggling.

### What is NOT supported

- Quoted arguments with embedded spaces (`-- 'name with space'`)
- Glob patterns (`--tests-*`)
- Tilde expansion (`~/tmp`)
- Environment variable references (`${HOME}`, `$VAR`)
- Shell history expansion (`!`)
- Pipes, redirections, backgrounding, subshells
- Anything that isn't a plain whitespace-separated argv vector

This is the intended behavior. Custom scope means "allowlisted test command with ordinary arguments," not arbitrary shell syntax after an allowlisted prefix. If you need shell semantics, do not use the test runner — use the bash tool directly.

## Previous-Failures Index (Phase 06)

> **RunStore is the authoritative persistence layer for test runs.** The legacy `.codegg/test-runs/index.json` is retained for backward compatibility with `TestScope::PreviousFailures` and TUI commands that read the index directly. The legacy index is deprecated and will be removed once `PreviousFailures` reads from RunStore directly.

The `index.rs` module maintains a bounded, local index of recent test runs so that the `PreviousFailures` scope can automatically find and rerun the most recent failing test command.

### Index file location

```
.codegg/test-runs/index.json
```

### Key types

```rust
pub struct TestRunIndex {
    pub version: u32,           // always 1
    pub updated_at: String,     // RFC3339 timestamp
    pub runs: Vec<TestRunIndexEntry>,
}

pub struct TestRunIndexEntry {
    pub run_id: String,         // directory name (e.g. "20260708T123456Z-abc12345")
    pub created_at: String,     // RFC3339 timestamp
    pub status: TestStatus,
    pub failure_class: FailureClass,
    pub language: String,       // "rust", "python", "go", etc.
    pub scope_label: String,
    pub cwd: PathBuf,
    pub argv: Vec<String>,
    pub summary: String,        // truncated to MAX_SUMMARY_BYTES (1000)
    pub failures: Vec<TestFailureIndexEntry>,
    pub log_dir: PathBuf,
    pub stdout_log: Option<PathBuf>,
    pub stderr_log: Option<PathBuf>,
    pub report_json: Option<PathBuf>,
}

pub struct TestFailureIndexEntry {
    pub name: Option<String>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub message_preview: String, // truncated to MAX_MESSAGE_PREVIEW_BYTES (500)
    pub failure_class: FailureClass,
}
```

### Constants

| Constant | Value | Purpose |
|----------|-------|---------|
| `MAX_INDEX_ENTRIES` | 100 | Max runs retained in the index |
| `MAX_FAILURE_ENTRIES_PER_RUN` | 10 | Max failures captured per run entry |
| `MAX_MESSAGE_PREVIEW_BYTES` | 500 | Truncation limit for failure messages |
| `MAX_SUMMARY_BYTES` | 1000 | Truncation limit for run summaries |

### Writing

After every test run completes, `runner.rs` calls `append_to_index()` which:

1. Loads the existing index (or creates a new one if missing/malformed)
2. Appends a new `TestRunIndexEntry` built from the `TestReport`
3. Bounds the index to `MAX_INDEX_ENTRIES` (keeps newest entries)
4. Writes atomically via `.tmp` + rename

A static `OnceLock<Mutex<()>>` serializes concurrent writes in-process.

### Resolution

`resolve_previous_failures()` in `resolve.rs`:

1. Loads the index from `.codegg/test-runs/index.json`
2. Scans entries newest-first for the first "actionable" failure (status = `Failed` or `TimedOut`)
3. Validates the entry's cwd exists and is within the request workdir
4. Validates argv is non-empty and argv[0] is from a known test runner
5. Returns `ResolvedTestCommand` with `scope_label: "previous-failures:{run_id}"`

### Safety

- `validate_indexed_rerun_command()` rejects empty argv, empty tokens, cwd outside workdir, and unrecognized argv[0]
- Only `Failed` and `TimedOut` entries are actionable — `Passed`, `Cancelled`, and `Error` entries are skipped
- `truncate_utf8()` ensures stored summaries and failure messages respect byte limits without splitting UTF-8 char boundaries

## Protocol Event Integration (Phase 07)

The test runner publishes lifecycle events to the `GlobalEventBus` via the `BusEventSink` implementation (`src/test_runner/bus_sink.rs`). This bridges the `TestEventSink` trait to the core protocol so remote clients (WebSocket, stdio) can observe test runs.

### BusEventSink

```rust
pub struct BusEventSink;

impl TestEventSink for BusEventSink {
    fn started(&self, snapshot: TestRunStartedSnapshot);
    fn progress(&self, snapshot: TestRunProgressSnapshot);
    fn completed(&self, snapshot: TestRunCompletedSnapshot);
}
```

Each method publishes the corresponding `AppEvent::TestRun*` variant to `GlobalEventBus::publish()`.

### Event flow

1. `resolve_and_run_test()` calls `BusEventSink::started()` / `progress()` / `completed()` at lifecycle boundaries
2. `GlobalEventBus` delivers `AppEvent::TestRun*` to subscribers
3. `map_app_event_to_core_event()` in `src/core/mod.rs` converts to `CoreEvent::TestRun*`
4. `bridge_app_event()` in `src/core/daemon.rs` wraps in `EventEnvelope` and sends to remote clients

### Protocol wire events

| AppEvent | CoreEvent | Wire type string |
|----------|-----------|------------------|
| `TestRunStarted` | `TestRunStarted` | `"test_run_started"` |
| `TestRunProgress` | `TestRunProgress` | `"test_run_progress"` |
| `TestRunCompleted` | `TestRunCompleted` | `"test_run_completed"` |

The TUI handler (`src/tui/commands/test.rs`) and tool handler (`src/tool/test.rs`) both pass `Some(&BusEventSink)` to `resolve_and_run_test()`.

```bash
cargo test -p codegg-protocol -- core_event_test_run
cargo test -p codegg --lib core::tests::test_run
```

## Parser API

### TestParseState

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
pub fn ingest_stdout_line(state: &mut TestParseState, line: &str)
pub fn ingest_stderr_line(state: &mut TestParseState, line: &str)
pub fn failure_class_summary(failures: &[TestFailure], compile_errors: &[TestFailure]) -> FailureClass
```

Parses test output line-by-line. Lines are first stripped of ANSI escape sequences (CSI codes) before pattern matching, ensuring color-enabled test output is parsed correctly. Recognizes:

- **Rust**: `running N tests`, `test ... ok/FAILED`, `panicked at`, `error[E`, `--> file:line:col` (compile error location), doctest failures (`test file - func (line N) ... FAILED`)
- **Python/pytest**: `collected N items`, `PASSED/FAILED/ERROR`, `ERROR collecting`, `E   message`
- **Panic extraction**: Extracts message, file, and line from `panicked at 'msg', file:line:col` and `panicked at 'msg' (file:line)` formats. Handles messages containing commas, colons, and backticks.
- **Compile error extraction**: Extracts error code (e.g. `E0432`), message, file:line from `error[E0432]: msg` and `--> file:line:col` lines.
- **Pytest distinction**: `ERROR` lines with `::` are `PytestError`. Lines with `ERROR` but no `::` are `PytestCollectionError`. Lines with `FAILED` are `PytestFailure`.
- **`failure_class_summary`**: Returns the most severe failure class from a list of failures.

## Runner API

### TestRunError

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

### resolve_and_run_test

```rust
pub async fn resolve_and_run_test(request: TestRunRequest) -> Result<TestReport, TestRunError>
```

Convenience wrapper: resolves the request, then delegates to `run_resolved_test`.

### run_resolved_test

```rust
pub async fn run_resolved_test(
    request: &TestRunRequest,
    resolved: ResolvedTestCommand,
) -> Result<TestReport, TestRunError>
```

Spawns a tokio process with piped stdout/stderr and reads lines concurrently via `BufReader`. **Both generated commands and validated custom commands are executed as direct `argv` via `Command::new(argv[0]).args(&argv[1..])` — never via a shell.** Raw bytes are written to log files. Decoded lines are fed to `TestParseState` via `Arc<Mutex<>>`. A `tokio::select!` supervisor enforces wall-clock timeout (default 300s) and no-output/stall timeout (default 120s). On completion, classifies exit code and builds `TestReport` with `FailureClass` from parsed results.

#### Process-group cleanup (Unix only)

On Unix, the child is placed in its own session/process group via `setsid()` in `pre_exec`, so that timeout kills target the entire process tree using `libc::kill(-pgid, SIGKILL)`, preventing grandchild process leaks. The `setsid()` call and the negative-`pgid` kill are both `#[cfg(unix)]` gated.

**Non-Unix fallback**: On non-Unix targets, `spawn_child` skips the `setsid()` step, and `kill_child` falls back to `child.kill().await`, which only kills the direct child — grandchildren can outlive the timeout. This is a known limitation. Cross-platform process-tree cleanup is not implemented.

## Log Directory Layout

Logs are written to `.codegg/test-runs/<utc-timestamp>-<short-uuid>/`:

```
.codegg/test-runs/
  20260708T123456Z-a1b2c3/
    stdout.log      # raw stdout bytes
    stderr.log      # raw stderr bytes
    report.json     # serialized TestReport
```

## Formatter API

```rust
pub fn format_test_report(report: &TestReport) -> String
```

Formats a `TestReport` into a bounded, stable, model-facing string with these sections:

```
Test run <passed|failed|timed out|errored>.

Command:
<command>

Duration:
<duration>

Exit code:
<exit code or unavailable>

Failure class:
<class>

Summary:
<one short paragraph>

Primary failures:
1. <test/file/line/message>
...

Logs:
stdout: <path>
stderr: <path>
report: <path>
```

**Bounds**:
- Max primary failures displayed: 5 (extra omitted with note)
- Max failure message bytes: 2000 (truncated with `...`)
- Max timeout excerpt bytes: 2000
- Total report controlled by `max_report_bytes`

Empty sections are suppressed. Full logs always available under `.codegg/test-runs/`.

## Projection Adapter (Phase 03)

```rust
pub fn test_report_to_projection(report: &TestReport) -> ProjectionResult
```

Converts a structured `TestReport` into a `ProjectionResult` for the shell-output projection pipeline. This avoids re-parsing raw logs — the report is already structured.

### Mapping

| `TestReport` field | `ProjectionResult` field |
|---------------------|--------------------------|
| `status` (Passed/Failed/TimedOut/Error) | `text` (formatted report body), `projector` = `"test-report"`, `kind` = `Structured`, `exactness` = `Exact` |
| `failures[].name` | Lines in `text` with failure details (max 20 displayed) |
| `failures[].failure_class` | `warnings` (failure class labels for non-passed runs) |
| `output_truncated` | `warnings` includes truncation notice |
| `previous_run_id` | `warnings` includes rerun source label |
| `summary` | Included in `text` body |

The adapter does NOT apply redaction (handled at `ProjectionSelector::project()` level). Sets `RedactionState::NotApplied`.

### Bounds

- Max failures displayed: 20 (extra omitted with count)
- Max failure message bytes: 2000 (truncated with `...`)
- Max timeout excerpt bytes: 2000

```bash
cargo test -p codegg --lib test_runner::projection
```

## Tests

118+ parser + formatter + runner + custom-allowlist + index + projection + bypass-regression tests:
- 11 resolver tests (auto rust, auto python, mixed ambiguity, package, file, changed fallback, custom command tokenization, forbidden-shell-syntax rejection, unsupported-command rejection, empty mapping, prefix-collision rejection)
- 30+ custom command validator tests (allowed argv prefixes, disallowed commands, empty/whitespace, allowlist invariants, semicolon/`&&`/`||`/`|`/`>`/`<` suffix rejection, `$(...)` and `${...}` substitution rejection, backtick substitution, newline/CR rejection, `&` backgrounding, leading-disallowed rejection, prefix-collision rejection for `pytestevil`/`cargo testify`/`make testcase`, quoted-argument rejection, glob metacharacter rejection, tilde/`#` rejection, history `!` rejection, NUL and other control characters, bidi Unicode control rejection, `is_allowed_custom_command` wrapper agreement)
- 22 parser tests (rust count, ok/failed, panic file/line, assertion message with file:line:col, compile error code, compile error location, doctest failure, pytest collected, pytest failed, pytest file, pytest assertion, pytest collection error, pytest error vs failure, ANSI stripping for rust/pytest/compile-error lines)
- 10 formatter tests (stable sections, passed suppression, timeout details, failure limit, max bytes, log paths, truncation note, error status, compile error display)
- 13+ runner tests (pass, fail, wall-clock timeout, stall timeout, empty command, zero timeout, log layout, UTF-8 truncation, summary building, parser failures for nonzero exit, timeout excerpt, sink present, sink absent, custom-tokenized argv path, timeout-after-kill)
- 18 index tests (load missing, load malformed, append creates file, bounds entries, truncation, newest actionable failure selected, non-actionable skipped, cwd validation, argv validation, empty argv rejected, empty token rejected, cwd outside workdir rejected, unknown argv[0] rejected, language detection, UTF-8 boundary handling)
- 11 projection adapter tests (passed/failed/timeout/error projections, output_bytes matching, truncation warnings, many-failures warning, previous-failures rerun source, redaction state, no omitted ranges, compile error class preservation)
- 14 `AsyncUiRequestState` tests covering stale completion protection for `/test` and other dialog async paths (`new_is_idle`, `begin_increments_and_sets_loading`, `begin_clears_previous_error`, `finish_returns_true_for_current`, `finish_returns_false_for_stale`, `finish_returns_false_when_cancelled`, `cancel_increments_and_clears_loading`, `fail_stores_error_for_current`, `fail_ignores_stale`, `fail_ignores_cancelled`, `clear_loading_does_not_affect_request_id`, `default_matches_new`, `begin_after_cancel_resets_cancelled`, `multiple_lifecycle_cycles`)

```bash
cargo test -p codegg --lib test_runner
```

## Model-Facing Tool Integration

The `test` tool (`src/tool/test.rs`) wraps the test runner for model consumption:

- **Tool name**: `test`
- **Category**: `ShellExec` (conservative permission gating)
- **Input**: JSON with `scope` (required), plus optional `package`, `path`, `command`, `workdir`, `timeout`, `stall_timeout`
- **Output**: Compact text report via `format_test_report()`
- **Custom commands**: Validated via `custom.rs::validate_custom_command`. Only argv-token-prefix matches against the 12-entry allowlist pass; shell metacharacters, redirection, pipes, command substitution, newlines, and prefix collisions are rejected. The validator returns the validated argv vector — both generated and custom commands execute via direct `Command::new(argv[0]).args(&argv[1..])` with no shell interpretation.
- **Defense-in-depth**: The resolver re-runs the strict validator before producing argv, so even if a presentation-boundary caller forgets to validate, the runner still rejects shell-injection attempts.
- **Failing tests**: Return success tool result with failure report; only infrastructure failures return `ToolError`
- **Provenance**: Native backend, `test_runner` implementation, `LocalTrusted`
- **No LLM involvement**: The test tool runs the resolved command directly via the supervised runner. No LLM call is made to interpret, summarize, or augment the output. The compact report is produced deterministically by `format_test_report()`.

The tool is registered in `ToolRegistry::with_options()` and categorized in `tool_category_for_name()`.

```bash
cargo test -p codegg --lib tool::test
```

The TUI `/test` slash command (`src/tui/commands/test.rs`) shares the same `validate_custom_command` function — there is exactly one source of truth for the validation contract.

### Stale completion protection

The TUI `/test` command uses `AsyncUiRequestState` (`src/tui/app/state/async_request.rs`) for stale-completion protection. Each `/test` invocation calls `begin()` to allocate a monotonically increasing request ID; when the result comes back, `finish(request_id)` returns `false` (silently dropping the result) if the request has been superseded or cancelled. This guarantees that a slow `/test custom cargo test` from an earlier invocation cannot overwrite the UI state of a newer `/test` invocation.
