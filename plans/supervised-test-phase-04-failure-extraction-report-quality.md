# Supervised Test Subsystem Phase 4: Failure Extraction and Report Quality

## Objective

Improve the supervised test subsystem's report quality so the model receives concise, actionable summaries for common Rust and Python test outcomes without needing full raw logs in context.

This phase assumes the `test` tool is already model-facing and returns a compact report. The goal is to make that report substantially more useful for agent repair loops by extracting primary failures, distinguishing failure classes, and preserving enough context to act.

## Precondition

Phases 1 through 3 should be landed:

```text
- Internal test runner types, resolver, parser skeleton, and report formatter exist.
- Streaming runner writes full stdout/stderr logs and returns TestReport.
- Model-facing `test` tool is registered and returns compact report text.
```

## Files to modify

Likely files:

```text
src/test_runner/parse.rs
src/test_runner/report.rs
src/test_runner/types.rs
src/test_runner/runner.rs
src/tool/test.rs
```

Optional test fixtures:

```text
tests/fixtures/test_runner/rust_failure_stdout.txt
tests/fixtures/test_runner/rust_compile_error_stderr.txt
tests/fixtures/test_runner/pytest_failure_stdout.txt
tests/fixtures/test_runner/pytest_error_stdout.txt
```

If the repository avoids text fixtures, inline representative output in unit tests.

## Failure-class taxonomy

Add a small taxonomy to avoid free-form report drift. This can be an enum or a stable string set. Recommended initial classes:

```text
passed
rust_test_failure
rust_panic
rust_compile_error
rust_doctest_failure
pytest_failure
pytest_error
pytest_collection_error
nonzero_exit
timeout_wall_clock
timeout_no_output
spawn_error
unknown_failure
```

If implemented as an enum, expose a stable display string for report formatting.

Do not overfit. The taxonomy should improve compact reporting, not become a full test framework abstraction.

## Rust parser improvements

Enhance Rust output parsing for common cargo test output.

Recognize:

```text
running N tests
test path::name ... ok
test path::name ... FAILED
failures:
---- path::name stdout ----
thread 'path::name' panicked at path/to/file.rs:line:col:
assertion `left == right` failed
left: ...
right: ...
error[E1234]: ...
error: could not compile ...
test result: FAILED. X passed; Y failed; ...
```

Extract for each failure when possible:

```text
name
file
line
message
failure_class
context excerpt, bounded
```

For compile errors, extract:

```text
error code if present
primary compiler message
file:line:col if present
first help/note line if nearby and useful
```

Avoid dumping the complete compiler diagnostic. The report should contain the first one to three actionable diagnostics and point to the full stderr log.

## Pytest parser improvements

Enhance pytest output parsing for common verbose and normal output.

Recognize:

```text
collected N items
path/test_file.py::test_name PASSED
path/test_file.py::test_name FAILED
path/test_file.py::test_name ERROR
FAILED path/test_file.py::test_name - AssertionError: ...
ERROR path/test_file.py::test_name - ...
E   AssertionError: ...
E   SomeException: ...
short test summary info
```

Extract for each failure when possible:

```text
name
file
line if present
message
failure_class
bounded traceback/assertion excerpt
```

Handle collection errors distinctly from normal test failures. If pytest never reaches collected tests and emits import/syntax/config errors, classify as `pytest_collection_error` or `pytest_error` depending on available signal.

## Timeout report improvements

Improve timeout reports from the runner/report formatter.

For wall-clock timeout, include:

```text
kind: timeout_wall_clock
elapsed
configured timeout
last output excerpt
last known test/progress line if parser observed one
log paths
```

For no-output timeout, include:

```text
kind: timeout_no_output
elapsed
stall timeout
last output excerpt
last known test/progress line if parser observed one
log paths
```

The last output excerpt should be bounded by bytes and lines. Prefer tail context, not head context.

## Report formatting rules

Update `format_test_report` so all model-facing reports follow a stable shape:

```text
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
report: <path if available>
```

Suppress empty sections. Do not include raw full output. Limit primary failures to a small number, ideally 5 by default. Add a note when more failures exist:

```text
... 7 additional failures omitted; see full log.
```

## Model-facing behavior

The `test` tool should continue to return successful tool output for failed tests. The report text must be explicit enough that the model can act without misinterpreting the tool call as successful test validation.

Good first line examples:

```text
Test run failed.
Test run timed out.
Test run passed.
Test run could not be started.
```

Do not use ambiguous phrasing such as `Tool completed successfully` in model-facing report text.

## Output bounds

Add or enforce output bounds in the report formatter:

```text
max primary failures: 5
max failure message bytes: 2000
max timeout excerpt bytes: 2000
max total report bytes: request.max_report_bytes
```

When trimming, cut on UTF-8 character boundaries. If a helper already exists elsewhere in the codebase for safe truncation, reuse it.

## Tests to add

Parser tests:

```text
rust_extracts_failed_test_name
rust_extracts_panic_file_line
rust_extracts_assertion_message
rust_extracts_compile_error_code_and_location
rust_detects_doctest_failure_if_supported
pytest_extracts_failed_test_name
pytest_extracts_failed_file
pytest_extracts_assertion_message
pytest_detects_collection_error
pytest_detects_error_vs_failure
```

Formatter tests:

```text
failed_report_has_stable_sections
passed_report_suppresses_failure_sections
timeout_report_includes_timeout_kind_and_last_output
report_omits_extra_failures_after_limit
report_respects_max_report_bytes
report_includes_full_log_paths
```

Runner integration tests if practical:

```text
runner_report_uses_parser_failures_for_nonzero_exit
runner_report_uses_generic_nonzero_when_parser_finds_nothing
runner_timeout_report_includes_last_output_excerpt
```

## Acceptance criteria

This phase is complete when:

```text
- Common Rust test assertion failures produce named primary failures.
- Rust panics include file/line when available.
- Rust compile errors are classified separately from test assertion failures.
- Common pytest failures produce named primary failures.
- Pytest collection/import errors are not flattened into generic failure when recognizable.
- Timeout reports include timeout class and last output/progress context.
- Model-facing reports are bounded, stable, and do not dump full logs.
- Full logs remain available under `.codegg/test-runs/`.
```

## Validation

Run targeted parser and formatter tests first, then runner/tool integration tests:

```text
cargo fmt
targeted test_runner parser/report tests
targeted test tool tests
cargo check
```

If full workspace tests are expensive, do not require them for this phase. The new tests should be narrow and deterministic.

## Handoff notes

The purpose of this phase is not to perfectly parse every runner format. The purpose is to give the model a compact, high-signal failure report for common Rust/Python loops. Unknown output should degrade to `unknown_failure` plus log paths, not fail the tool or dump the log.
