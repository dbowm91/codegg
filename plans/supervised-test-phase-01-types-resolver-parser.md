# Supervised Test Subsystem Phase 1: Types, Resolver, and Parser Skeleton

## Objective

Create the internal foundation for supervised test execution without spawning any test processes yet. This phase defines stable test-run data types, implements conservative command resolution for Rust/Python/generic test scopes, and adds parser skeletons that later phases can feed with streaming output.

The output of this phase should be pure and easily testable. No TUI integration, event-bus integration, model-facing tool, or process execution is required.

## Existing constraints

codegg currently exposes model capabilities through `src/tool/` and central registration in `ToolRegistry::with_options`. That will be used later, but this phase should avoid coupling the test model directly to the tool implementation.

The existing `bash` and `terminal` tools collect process output after completion. This phase should not modify those tools.

The agent loop already has an `is_test_command` helper. Do not depend on it directly yet. Treat it as a later integration point for agent guidance or rerouting.

## Files to add

Add a new module:

```text
src/test_runner/mod.rs
src/test_runner/types.rs
src/test_runner/resolve.rs
src/test_runner/parse.rs
src/test_runner/report.rs
```

If the project maintainers prefer keeping this local to tools for now, `src/tool/test_runner/` is acceptable, but the preferred location is `src/test_runner/` because the subsystem is expected to be reused by the model-facing tool, slash commands, and eventual daemon/TUI job state.

Wire the module from the crate root with the same visibility pattern used by nearby modules.

## Core types

Define a small first-pass type set in `types.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestScope {
    Auto,
    Workspace,
    Changed,
    Package(String),
    File(std::path::PathBuf),
    PreviousFailures,
    CustomCommand(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestLanguage {
    Rust,
    Python,
    Generic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestStatus {
    Passed,
    Failed,
    TimedOut,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeoutKind {
    WallClock,
    NoOutput,
    NoProgress,
}
```

Also add request, resolved-command, failure, timeout, and report structs with these fields:

```text
TestRunRequest:
  scope
  workdir
  timeout_secs
  stall_timeout_secs
  max_report_bytes

ResolvedTestCommand:
  language
  command argv vector
  cwd
  scope_label

TestFailure:
  optional name
  optional file
  optional line
  message
  failure_class

TestTimeout:
  kind
  elapsed_ms
  optional last_output

TestReport:
  status
  command argv vector
  cwd
  duration_ms
  optional exit_code
  summary
  failures
  optional timeout
  optional log_dir/stdout_log/stderr_log
  output_truncated
```

Keep serialization optional in this phase. If `report.json` writing is implemented in a later phase, derive `Serialize`/`Deserialize` then or add it now if it imposes no dependency churn.

## Resolver design

Implement `resolve.rs` around a pure resolver function:

```rust
pub fn resolve_test_command(request: &TestRunRequest) -> Result<ResolvedTestCommand, TestResolveError>
```

Define a local error type with `thiserror` covering:

```text
missing workdir
ambiguous or unknown command
missing package name
missing file path
empty custom command
unsupported scope for detected ecosystem
```

Resolver behavior:

```text
Auto:
  - If only Cargo.toml is detected, resolve to the default Rust test command.
  - If only Python test markers are detected, resolve to the default pytest command.
  - If both ecosystems are detected, return ambiguity unless explicit scope/path clarifies.
  - If neither is detected, return a clear unsupported/unknown error.

Workspace:
  - Rust: workspace-level cargo test command.
  - Python: root-level pytest command.

Changed:
  - First pass may resolve to Auto plus scope_label `changed-fallback`.
  - Do not implement git impact analysis in this phase.

Package(name):
  - Rust: package-scoped cargo test command.
  - Python: unsupported until a package mapping exists.

File(path):
  - Rust file: conservative root/package test fallback with a scope label naming the file.
  - Python file: pytest for that path.

PreviousFailures:
  - Unsupported until a report index exists.

CustomCommand(command):
  - Preserve the raw command intent in a Generic resolved command only as metadata.
  - Do not make this executable in Phase 1.
  - Phase 3 must add explicit permission/security gating before model exposure.
```

Prefer not to add shell-word parsing as a new dependency in this phase. Generated commands should be argv vectors. Custom command handling should remain inert metadata until the supervised runner and tool gating exist.

## Detection helpers

Add small helpers:

```rust
fn has_cargo_manifest(workdir: &Path) -> bool
fn has_python_test_markers(workdir: &Path) -> bool
fn detect_language_for_auto(workdir: &Path) -> Result<TestLanguage, TestResolveError>
```

Python markers should include:

```text
pyproject.toml
pytest.ini
tox.ini
noxfile.py
tests/
```

Rust marker:

```text
Cargo.toml
```

If both Rust and Python are detected under `Auto`, return an ambiguity error unless explicit scope/path clarifies the ecosystem. This avoids surprising mixed-workspace behavior.

## Parser skeleton

Implement `parse.rs` with lightweight parser state:

```rust
#[derive(Debug, Clone, Default)]
pub struct TestParseState {
    pub language: Option<TestLanguage>,
    pub tests_seen: usize,
    pub tests_passed: usize,
    pub tests_failed: usize,
    pub last_progress_line: Option<String>,
    pub failures: Vec<TestFailure>,
    pub compile_error_seen: bool,
}

pub fn ingest_stdout_line(state: &mut TestParseState, line: &str)
pub fn ingest_stderr_line(state: &mut TestParseState, line: &str)
```

For this phase, parser behavior can be deliberately partial.

Rust recognition:

```text
running N tests
test <name> ... ok
test <name> ... FAILED
thread '<name>' panicked at <file>:<line>
error[E....]
failures:
```

Pytest recognition:

```text
collected N items
<path>::<test> PASSED
<path>::<test> FAILED
FAILED <path>::<test>
ERROR <path>::<test>
E   <message>
```

Do not attempt complete parsing. The parser only needs enough signal to support useful compact reports later.

## Report formatting skeleton

Add `report.rs` with a formatter that converts a `TestReport` into stable text:

```rust
pub fn format_test_report(report: &TestReport) -> String
```

The formatter should include:

```text
status
command
cwd
duration
exit code
failure class / timeout class
up to first 5 failures
log path if present
truncation note if true
```

This formatter will be used by the model-facing tool in Phase 3.

## Tests to add

Resolver tests:

```text
resolves_auto_rust_when_cargo_toml_exists
resolves_auto_python_when_pytest_markers_exist
returns_ambiguity_for_mixed_rust_python_root
resolves_rust_package_scope
resolves_python_file_scope
changed_scope_uses_auto_fallback
custom_command_is_preserved_but_not_executable
```

Parser tests:

```text
rust_parser_detects_running_count
rust_parser_detects_ok_and_failed_tests
rust_parser_detects_panic_file_line
rust_parser_detects_compile_error
pytest_parser_detects_collection_count
pytest_parser_detects_failed_summary
pytest_parser_extracts_assertion_message
```

Formatter tests:

```text
format_report_includes_status_command_duration
format_report_limits_failure_count
format_report_includes_timeout_class
format_report_includes_log_path
```

## Acceptance criteria

This phase is complete when:

```text
- `src/test_runner/` exists and compiles.
- The resolver returns deterministic commands for common Rust/Python scopes.
- Mixed Rust/Python auto detection fails explicitly rather than guessing.
- Parser skeleton unit tests cover Rust and pytest common lines.
- Report formatter produces bounded stable text.
- No process spawning has been introduced yet.
- Existing `bash` and `terminal` behavior is unchanged.
```

## Validation commands

Run formatting, targeted unit tests for the new module, and a workspace check. If the workspace test set is too heavy locally, at minimum run targeted tests for the new module and a compile check.

## Handoff notes

Keep this phase boring. The value is establishing clear internal contracts before async process supervision adds complexity. Avoid introducing TUI, event bus, RTK, daemon persistence, or model tool behavior in this phase.
