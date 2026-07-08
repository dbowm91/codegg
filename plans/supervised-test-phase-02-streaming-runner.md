# Supervised Test Subsystem Phase 2: Streaming Runner and Log Capture

## Objective

Implement the supervised async process runner that executes resolved test commands, streams stdout/stderr incrementally, writes full logs to `.codegg/test-runs/`, applies wall-clock and no-output timeout policy, and produces a raw `TestReport` object.

This phase should still avoid model-facing tool registration and TUI command wiring. It should be callable from tests and from future `TestTool` code, but it should not yet alter the model tool surface.

## Precondition

Phase 1 should have added `src/test_runner/` with core types, resolver, parser skeleton, and report formatter. If Phase 1 landed under a different path, adapt imports but preserve the same conceptual separation.

## Files to add or modify

Add:

```text
src/test_runner/runner.rs
src/test_runner/logs.rs
```

Modify:

```text
src/test_runner/mod.rs
src/test_runner/types.rs
src/test_runner/parse.rs
src/test_runner/report.rs
```

Optional if the module uses a single file for logs:

```text
src/test_runner/logs.rs may be folded into runner.rs for the first pass.
```

## Runner API

Expose a primary async function:

```rust
pub async fn run_resolved_test(
    request: &TestRunRequest,
    resolved: ResolvedTestCommand,
) -> Result<TestReport, TestRunError>
```

Add a convenience wrapper:

```rust
pub async fn resolve_and_run_test(request: TestRunRequest) -> Result<TestReport, TestRunError>
```

Define `TestRunError` separately from `TestResolveError`:

```text
resolve error wrapper
log directory creation failure
process spawn failure
stdout pipe unavailable
stderr pipe unavailable
log write failure
process wait failure
invalid command vector
```

Do not collapse timeout into `TestRunError`. Timeout should generally produce a `TestReport` with `TestStatus::TimedOut` unless the runner could not spawn or supervise the process at all.

## Command execution model

Generated commands from the resolver should be argv vectors. The runner should execute the first vector element as the program and pass the rest as args.

For the minimal phase, avoid accepting arbitrary custom raw commands from external callers unless Phase 1 clearly marked them inert. If a `CustomCommand` path is enabled, gate it behind an explicit `allow_custom_command` field or internal-only constructor so later model exposure cannot accidentally execute raw commands without permission work.

The runner should:

```text
- canonicalize or validate cwd
- create log directory
- spawn process with stdout/stderr piped
- read stdout and stderr concurrently
- write raw bytes or decoded text to separate log files
- feed decoded lines into TestParseState
- track last_output_at
- track last_output_excerpt
- await process exit concurrently with timeout checks
- kill process on timeout
- synthesize TestReport
```

## Log directory layout

Use a deterministic but collision-resistant layout:

```text
.codegg/test-runs/<utc-timestamp>-<short-uuid>/
  stdout.log
  stderr.log
  report.json
```

The timestamp should be filesystem-safe. If chrono is already available, use UTC. If avoiding chrono use in this module, use system time plus UUID. The root `.codegg/test-runs` path should be relative to the request `workdir`.

Ensure `.codegg` directory creation is best-effort but explicit. If log creation fails, return a run error before spawning. Do not run tests without log capture in this phase, because preserving full logs outside model context is a core requirement.

## Streaming implementation details

Use `tokio::process::Command` with piped stdout/stderr and `tokio::io` readers.

Recommended shape:

```text
spawn child
move stdout reader into task
move stderr reader into task
move parser state behind Arc<Mutex<_>> or route parsed line events through mpsc
run supervisor select loop over:
  - child completion
  - stdout task completion
  - stderr task completion
  - wall-clock timer
  - no-output interval checks
```

Avoid holding a mutex while writing to disk. Prefer one of these approaches:

```text
A. each stream task writes to its own log file and sends parsed line events to supervisor
B. each stream task writes and directly updates independent parser state through small critical sections
```

Approach A is cleaner but more code. Approach B is acceptable for the minimal pass if tests remain deterministic.

## Timeout policy

Implement two timeout classes in this phase:

```text
wall-clock timeout:
  elapsed >= request.timeout_secs

no-output timeout:
  no stdout/stderr bytes or lines observed for request.stall_timeout_secs
```

Default values should be conservative and overridable by caller:

```text
timeout_secs: 300
stall_timeout_secs: 120
max_report_bytes: 20000
```

If `stall_timeout_secs == 0`, disable no-output timeout. If `timeout_secs == 0`, reject the request as invalid rather than allowing infinite execution.

On timeout:

```text
- attempt to kill the child
- wait for child termination if possible with a short bounded grace window
- mark report status TimedOut
- attach TimeoutKind::WallClock or TimeoutKind::NoOutput
- include last output excerpt if available
- keep log paths in report
```

Cross-platform process-group cleanup can be deferred, but add comments where child-process cleanup is incomplete. The first version may use `child.kill().await` and `kill_on_drop(true)`.

## Exit classification

After normal process completion:

```text
exit code 0 -> TestStatus::Passed
nonzero exit + parser failures -> TestStatus::Failed
nonzero exit + compile error marker -> TestStatus::Failed with compile/build class
nonzero exit + no parser failures -> TestStatus::Failed with generic nonzero_exit summary
```

If the process cannot be spawned, return `TestRunError::Spawn` rather than a failed report. Spawn failure means the test command itself was invalid or unavailable, not that tests failed.

## Parser integration

Feed decoded stdout/stderr lines to the Phase 1 parser. Preserve raw logs even when lines are invalid UTF-8. For parser input, use lossy UTF-8 conversion if necessary. The report should note if invalid UTF-8 was observed only if this is easy to track.

The runner should update parse state with enough information for `report.rs` to produce:

```text
tests_seen
tests_passed
tests_failed
failure list
compile_error_seen
last_progress_line
```

## Report JSON

Write `report.json` after process completion or timeout if serialization was added in Phase 1. If serialization was not added, either add it in this phase or defer JSON writing. At minimum, the returned report must include log paths.

Recommended report JSON fields:

```text
status
command
cwd
duration_ms
exit_code
summary
failure_count
failures
timeout
stdout_log
stderr_log
output_truncated
```

Do not write provider/model/session secrets into report JSON.

## Tests to add

Add integration tests that use temporary directories and small local commands that are safe and deterministic.

Test cases:

```text
runner_captures_stdout_to_log
runner_captures_stderr_to_log
runner_reports_success_on_zero_exit
runner_reports_failure_on_nonzero_exit
runner_enforces_wall_clock_timeout
runner_enforces_no_output_timeout
runner_includes_last_output_on_timeout
runner_preserves_full_log_when_report_is_compact
runner_rejects_empty_command_vector
```

Use tiny commands already available in normal Unix environments where possible, but keep Windows portability in mind if the repository supports Windows. If shell portability is too difficult, isolate platform-specific tests behind `cfg(unix)` and leave pure parser/resolver tests cross-platform.

## Avoided work

Do not add the model-facing tool in this phase.

Do not add `/test` in this phase.

Do not add event-bus lifecycle events in this phase unless required for internal diagnostics.

Do not implement process-group cleanup beyond a clearly documented minimal child kill path.

Do not implement RTK/context artifact integration.

## Acceptance criteria

This phase is complete when:

```text
- A resolved Rust/Python/generic command can be supervised by the new runner.
- stdout and stderr are streamed into separate logs.
- parser state is updated while output is read.
- wall-clock timeout yields a TimedOut report.
- no-output timeout yields a distinct TimedOut report.
- nonzero exit yields a Failed report, not a tool execution error.
- full logs are preserved outside the model-facing summary.
- existing `bash` and `terminal` tools are untouched.
```

## Validation

Run:

```text
cargo fmt
targeted test_runner tests
cargo check
```

If adding async tests, use `#[tokio::test]` and avoid long sleeps. Timeout tests should use very short durations and deterministic commands to avoid slowing the suite.

## Handoff notes

The key technical decision in this phase is to make the runner streaming from the beginning. Do not use process-output collection followed by parsing; that recreates the limitation this subsystem is meant to fix.
