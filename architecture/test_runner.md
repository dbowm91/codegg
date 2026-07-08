# Test Runner Module

The `test_runner` module provides test command resolution, output parsing, report formatting, and streaming process execution for supervised test runs.

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

**Key Responsibilities**:
- Resolve `TestScope` into platform-specific shell commands
- Spawn and supervise test processes with streaming stdout/stderr capture
- Parse stdout/stderr into structured results with failure-class taxonomy
- Capture raw logs to `.codegg/test-runs/` directory
- Classify exit codes, panics, compile errors, and pytest failures
- Format `TestReport` into bounded, stable, model-facing text

**Phase**: 4 (Failure Extraction and Report Quality)

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
}
```

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
}
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
    PreviousFailuresUnsupported,
}
```

## Custom Command Allowlist

The `custom.rs` module defines the shared allowlist used by both the model-facing `test` tool and the `/test` slash command:

```rust
pub const CUSTOM_COMMAND_ALLOWLIST: &[&str] = &[
    "cargo test", "cargo nextest", "pytest", "uv run pytest",
    "go test", "zig build test", "make test", "make check",
    "npm test", "pnpm test", "yarn test", "bun test",
];

pub fn is_allowed_custom_command(cmd: &str) -> bool
```

Custom commands not in the allowlist are rejected at the tool and TUI layers. The resolver itself does not enforce the allowlist — it only rejects empty commands. This layered design allows the resolver to remain reusable while security enforcement happens at the presentation boundaries.

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

Spawns a tokio process with piped stdout/stderr and reads lines concurrently via `BufReader`. On Unix, the child is placed in its own session/process group via `setsid()` so that timeout kills target the entire process tree (`libc::kill(-pgid, SIGKILL)`), preventing grandchild process leaks. Raw bytes are written to log files. Decoded lines are fed to `TestParseState` via `Arc<Mutex<>>`. A `tokio::select!` supervisor enforces wall-clock timeout (default 300s) and no-output/stall timeout (default 120s). On completion, classifies exit code and builds `TestReport` with `FailureClass` from parsed results.

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

## Tests

56+ parser + formatter + runner + custom-allowlist tests:
- 7 resolver tests (auto rust, auto python, mixed ambiguity, package, file, changed fallback, custom command)
- 4 custom command allowlist tests (allowed, disallowed, empty, nonempty)
- 22 parser tests (rust count, ok/failed, panic file/line, assertion message with file:line:col, compile error code, compile error location, doctest failure, pytest collected, pytest failed, pytest file, pytest assertion, pytest collection error, pytest error vs failure, ANSI stripping)
- 10 formatter tests (stable sections, passed suppression, timeout details, failure limit, max bytes, log paths, truncation note, error status, compile error display)
- 11+ runner tests (pass, fail, wall-clock timeout, stall timeout, empty command, zero timeout, log layout, UTF-8 truncation, summary building, parser failures for nonzero exit, timeout excerpt)

```bash
cargo test -p codegg --lib test_runner
```

## Model-Facing Tool Integration

The `test` tool (`src/tool/test.rs`) wraps the test runner for model consumption:

- **Tool name**: `test`
- **Category**: `ShellExec` (conservative permission gating)
- **Input**: JSON with `scope` (required), plus optional `package`, `path`, `command`, `workdir`, `timeout`, `stall_timeout`
- **Output**: Compact text report via `format_test_report()`
- **Custom commands**: Allowlisted via `custom.rs` (cargo test, pytest, go test, etc.)
- **Failing tests**: Return success tool result with failure report; only infrastructure failures return `ToolError`
- **Provenance**: Native backend, `test_runner` implementation, `LocalTrusted`
- **No LLM involvement**: The test tool runs the resolved command directly via the supervised runner. No LLM call is made to interpret, summarize, or augment the output. The compact report is produced deterministically by `format_test_report()`.

The tool is registered in `ToolRegistry::with_options()` and categorized in `tool_category_for_name()`.

```bash
cargo test -p codegg --lib tool::test
```
