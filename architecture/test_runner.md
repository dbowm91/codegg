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
| `parse.rs` | Line-by-line output parser |
| `report.rs` | Text formatter |
| `runner.rs` | Streaming runner with log capture |

**Key Responsibilities**:
- Resolve `TestScope` into platform-specific shell commands
- Spawn and supervise test processes with streaming stdout/stderr capture
- Parse stdout/stderr into structured results
- Capture raw logs to `.codegg/test-runs/` directory
- Classify exit codes and build `TestReport`
- Format `TestReport` into human-readable text

**Phase**: 2 (Streaming Runner + Log Capture)

## Key Types

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
    pub failure_class: String,
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
    pub compile_error_seen: bool,
}
```

### Free functions

```rust
pub fn ingest_stdout_line(state: &mut TestParseState, line: &str)
pub fn ingest_stderr_line(state: &mut TestParseState, line: &str)
```

Parses test output line-by-line. Recognizes:
- **Rust**: `running N tests`, `test ... ok/FAILED`, `panicked at`, `error[E`, `failures:`
- **Python/pytest**: `collected N items`, `PASSED/FAILED/ERROR`, `E   message`

Uses a `rust_matched` flag to prevent Python patterns from matching Rust output.

## Runner API (Phase 2)

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

Spawns a tokio process with piped stdout/stderr and reads lines concurrently via `BufReader`. Raw bytes are written to log files. Decoded lines are fed to `TestParseState` via `Arc<Mutex<>>`. A `tokio::select!` supervisor enforces wall-clock timeout (default 300s) and no-output/stall timeout (default 120s). On completion, classifies exit code (0 → Passed, nonzero with failures → Failed, nonzero with compile error → Failed) and writes `report.json` to the log directory.

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

Formats a `TestReport` into a human-readable string. Shows status, argv, cwd, duration, exit code, failure/timeout info, up to 5 failures, log dir, and truncation note.

## Tests

28 tests total:
- 7 resolver tests (auto rust, auto python, mixed ambiguity, package, file, changed fallback, custom command)
- 7 parser tests (rust running count, ok/failed, panic file/line, compile error, pytest collected, pytest failed, pytest assertion)
- 4 formatter tests (status/command/duration, failure limit, timeout class, log path)
- 10 runner tests (pass, fail, wall-clock timeout, stall timeout, empty command, zero timeout, log layout, UTF-8 truncation, summary building)

```bash
cargo test -p codegg --lib test_runner
```

## Model-Facing Tool Integration

The `test` tool (`src/tool/test.rs`) wraps the test runner for model consumption:

- **Tool name**: `test`
- **Category**: `ShellExec` (conservative permission gating)
- **Input**: JSON with `scope` (required), plus optional `package`, `path`, `command`, `workdir`, `timeout`, `stall_timeout`
- **Output**: Compact text report via `format_test_report()`
- **Custom commands**: Allowlisted (cargo test, pytest, go test, etc.)
- **Failing tests**: Return success tool result with failure report; only infrastructure failures return `ToolError`
- **Provenance**: Native backend, `test_runner` implementation, `LocalTrusted`

The tool is registered in `ToolRegistry::with_options()` and categorized in `tool_category_for_name()`.

```bash
cargo test -p codegg --lib tool::test
```
