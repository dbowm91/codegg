# Test Runner Module

The `test_runner` module provides test command resolution, output parsing, and report formatting for supervised test execution.

## Overview

**Location**: `src/test_runner/`

**Key Responsibilities**:
- Resolve `TestScope` into platform-specific shell commands
- Parse stdout/stderr from test executions into structured results
- Format `TestReport` into human-readable text

**Phase**: 1 (Types + Resolver + Parser Skeleton + Formatter). No process spawning.

## Key Types

### TestScope

```rust
pub enum TestScope {
    Auto,
    Workspace,
    Changed,
    Package(String),
    File(String),
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
    pub language: Option<TestLanguage>,
    pub package: Option<String>,
    pub file: Option<String>,
    pub timeout_secs: Option<u64>,
    pub retries: Option<u8>,
    pub extra_args: Vec<String>,
}
```

### ResolvedTestCommand

```rust
pub struct ResolvedTestCommand {
    pub command: String,
    pub language: TestLanguage,
    pub cwd: PathBuf,
    pub timeout_secs: u64,
}
```

### TestFailure

```rust
pub struct TestFailure {
    pub name: String,
    pub message: String,
    pub file: Option<String>,
    pub line: Option<u32>,
}
```

### TestTimeout

```rust
pub struct TestTimeout {
    pub kind: TimeoutKind,
    pub after_secs: u64,
}
```

### TestReport

```rust
pub struct TestReport {
    pub status: TestStatus,
    pub command: String,
    pub cwd: PathBuf,
    pub duration_ms: u64,
    pub exit_code: Option<i32>,
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub ignored: u32,
    pub failures: Vec<TestFailure>,
    pub timeout: Option<TestTimeout>,
    pub log_path: Option<PathBuf>,
    pub truncated_output: bool,
}
```

## Key Functions

### resolve_test_command

```rust
pub fn resolve_test_command(request: &TestRunRequest) -> Result<ResolvedTestCommand, TestResolveError>
```

Resolves a `TestScope` into a concrete shell command. For `Auto` scope, detects language by checking for `Cargo.toml` (Rust) or Python test markers (`pyproject.toml`, `pytest.ini`, `tox.ini`, `noxfile.py`, `tests/`). Returns `AmbiguousLanguage` if both ecosystems are detected.

### TestParseState

```rust
pub struct TestParseState { /* private fields */ }

impl TestParseState {
    pub fn new() -> Self;
    pub fn ingest_stdout_line(&mut self, line: &str);
    pub fn ingest_stderr_line(&mut self, line: &str);
    pub fn finish(self, exit_code: Option<i32>) -> TestReport;
}
```

Parses test output line-by-line. Recognizes:
- **Rust**: `running N tests`, `test ... ok/FAILED`, `panicked at`, `error[E`, `failures:`
- **Python/pytest**: `collected N items`, `PASSED/FAILED/ERROR`, `E   message`

Uses a `rust_matched` flag to prevent Python patterns from matching Rust output.

### format_test_report

```rust
pub fn format_test_report(report: &TestReport) -> String
```

Formats a `TestReport` into a human-readable string. Shows status, command, cwd, duration, exit code, failure/timeout info, up to 5 failures, log path, and truncation note.

## Error Types

### TestResolveError

```rust
#[derive(Error, Debug)]
pub enum TestResolveError {
    #[error("working directory not found: {0}")]
    CwdNotFound(PathBuf),
    #[error("cannot detect language: no Cargo.toml or Python test markers found in {0}")]
    NoLanguageDetected(PathBuf),
    #[error("ambiguous language: both Cargo.toml and Python test markers found in {0}")]
    AmbiguousLanguage(PathBuf),
    #[error("unsupported language for scope: {scope:?} with {language:?}")]
    UnsupportedScope { scope: TestScope, language: TestLanguage },
}
```

## Tests

18 tests total:
- 7 resolver tests (auto rust, auto python, mixed ambiguity, package, file, changed fallback, custom command)
- 7 parser tests (rust running count, ok/failed, panic file/line, compile error, pytest collected, pytest failed, pytest assertion)
- 4 formatter tests (status/command/duration, failure limit, timeout class, log path)

```bash
cargo test -p codegg --lib test_runner
```
