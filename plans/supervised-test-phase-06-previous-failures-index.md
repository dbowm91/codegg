# Supervised Test Phase 06: Previous-Failures Index and Rerun Support

## Context

The minimal supervised test subsystem is now in good shape:

```text
- `src/test_runner/` owns command resolution, parsing, reporting, and supervised execution.
- The model-facing `test` tool is registered as `ShellExec` and returns compact deterministic reports.
- `/test` runs supervised tests outside the LLM path.
- Logs and report JSON are written under `.codegg/test-runs/<run-id>/`.
- Custom commands use strict argv-prefix allowlisting and execute without shell interpretation.
- Timeout cleanup, ANSI parsing, stale TUI completion protection, and bypass regression tests have been added.
```

The next useful feature in this line of work is `previous_failures`: the existing API and `/test previous` concept should become functional. The goal is not to build a full test database, RTK memory, or impact-analysis engine. The goal is a small deterministic index over prior supervised test reports so codegg can rerun the last known failing test scope or command without asking the model to inspect logs manually.

## Primary objective

Add a local, bounded, deterministic previous-failures index for supervised test runs and wire it into:

```text
- `TestScope::PreviousFailures`
- model-facing `test` tool scope `previous_failures`
- `/test previous`
- report/log metadata
- docs and validation tests
```

The implementation should support rerunning the last actionable failing command from the supervised test runner. It should not infer test impact from git changes, invoke the LLM, or perform semantic clustering.

## Non-goals

Do not implement:

```text
- RTK-backed memory
- vector search over test logs
- semantic failure clustering
- flaky-test heuristics
- coverage-aware test selection
- changed-file impact analysis
- cross-session daemon jobs
- automatic model resumption when a rerun completes
```

Those can be layered later. This phase is a local JSON index and rerun resolver.

## Desired user behavior

### Model-facing tool

A model call like:

```json
{"scope":"previous_failures"}
```

should resolve to the most recent actionable failed supervised test command, rerun it through the same supervised runner, and return a compact report.

If no actionable prior failure exists, return an infrastructure-level tool error or resolver error with a clear message:

```text
No previous supervised test failure is available to rerun. Run `/test`, `/test workspace`, or the `test` tool first.
```

### TUI slash command

`/test previous` should behave the same way: run the most recent actionable failure through the supervised runner, display progress and compact completion, and leave full logs under `.codegg/test-runs/`.

### Log directory

Each supervised test run should continue to write:

```text
.codegg/test-runs/<run-id>/stdout.log
.codegg/test-runs/<run-id>/stderr.log
.codegg/test-runs/<run-id>/report.json
```

The new index should live near the logs:

```text
.codegg/test-runs/index.json
```

## Data model

Add a small index module:

```text
src/test_runner/index.rs
```

Recommended public API:

```rust
pub struct TestRunIndex {
    pub version: u32,
    pub updated_at: String,
    pub runs: Vec<TestRunIndexEntry>,
}

pub struct TestRunIndexEntry {
    pub run_id: String,
    pub created_at: String,
    pub status: TestStatus,
    pub failure_class: FailureClass,
    pub language: TestLanguage,
    pub scope_label: String,
    pub cwd: PathBuf,
    pub argv: Vec<String>,
    pub summary: String,
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
    pub message_preview: String,
    pub failure_class: FailureClass,
}
```

Keep this structure deliberately redundant. It should remain useful even if a log directory is later pruned. Store previews, not entire logs.

### Versioning

Set `version: 1` initially. On incompatible future changes, the loader should safely ignore unknown versions or migrate explicitly.

### Size bounds

The index must stay bounded. Recommended defaults:

```text
max entries: 100
max failure entries per run: 10
max message preview bytes per failure: 500
max summary bytes: 1000
```

If these become config values later, keep hardcoded constants for this phase.

## Index write path

### Tasks

Update `src/test_runner/runner.rs` after `TestReport` creation and `report.json` write.

The runner should append/update the index for every completed supervised test run, including pass, fail, timeout, and error statuses. However, `previous_failures` should only consider entries that are actionable failures.

Actionable failure statuses:

```text
TestStatus::Failed
TestStatus::TimedOut
```

Non-actionable statuses:

```text
TestStatus::Passed
TestStatus::Cancelled
TestStatus::Error caused by spawn/resolve/log infrastructure failure
```

If current status typing cannot distinguish infrastructure error from test error, use conservative behavior: only `Failed` and `TimedOut` are rerunnable.

### Atomic write

Write the index atomically:

```text
1. read existing index if present
2. update in memory
3. write `.codegg/test-runs/index.json.tmp`
4. fsync if practical but not mandatory for this phase
5. rename tmp to `index.json`
```

If index writing fails, the test run itself should still complete. Index write failure should be logged as a warning and optionally included in the report metadata. It should not turn a passing/failing test into an infrastructure failure.

### Concurrency note

The minimal implementation may not support concurrent supervised test runs perfectly. Still, protect the index write in-process with a static async mutex:

```rust
static TEST_RUN_INDEX_LOCK: OnceCell<tokio::sync::Mutex<()>> = ...
```

If using `OnceLock` is available in the MSRV, prefer it. Avoid adding a heavy dependency solely for this.

Cross-process locking can be documented as deferred.

### Acceptance criteria

```text
- `index.json` is created after supervised test runs.
- The index remains bounded.
- Index write failures do not fail the test run.
- Existing reports/logs continue to be written exactly as before.
- Tests cover missing index, malformed index, and bounded truncation.
```

## Previous-failures resolution

### Resolver behavior

Update `src/test_runner/resolve.rs` to implement `TestScope::PreviousFailures`.

Resolution should:

```text
1. locate `.codegg/test-runs/index.json` under request.workdir
2. load the index safely
3. scan entries newest-first
4. select the first actionable failing/timed-out entry whose cwd still exists
5. ensure argv is non-empty
6. return `ResolvedTestCommand` using the indexed argv and cwd
7. mark scope_label as `previous-failures:<run_id>` or similar
```

Do not attempt to inspect the original log files to reconstruct commands. The index should contain the rerun argv.

### Safety

Because the index is local file data, treat it as untrusted input. Validate indexed argv before rerun.

Generated commands previously produced by the resolver may include known safe command prefixes. For this phase, validate previous-failure argv with a small internal policy:

```text
- argv[0] must be one of known test executables/prefixes generated by the resolver or allowed custom validator
- no empty argv tokens
- no path traversal checks are needed for argv itself, but cwd must be under or equal to request.workdir unless explicitly documented otherwise
```

Recommended helper:

```rust
pub fn validate_indexed_rerun_command(argv: &[String], request_workdir: &Path, cwd: &Path) -> Result<(), TestIndexError>
```

Keep it stricter than necessary. It is acceptable to reject old/malformed index entries and continue scanning older entries.

### Error cases

Add explicit resolver errors:

```rust
PreviousFailuresIndexMissing(PathBuf)
PreviousFailuresIndexUnreadable(PathBuf)
PreviousFailuresIndexMalformed(PathBuf)
NoPreviousFailures(PathBuf)
PreviousFailureCommandInvalid(String)
```

If the index exists but all entries are stale or invalid, return `NoPreviousFailures` with a message that notes stale/invalid entries were ignored.

### Acceptance criteria

```text
- `TestScope::PreviousFailures` no longer returns unsupported by default.
- Missing index returns a clear resolver error.
- Malformed index returns a clear resolver error or is treated as no previous failures, but behavior is documented.
- Stale/invalid entries are skipped safely.
- The selected command is rerun through normal supervised runner path.
```

## Tool and TUI integration

### Model-facing `test` tool

Ensure `scope = previous_failures` maps to `TestScope::PreviousFailures` and needs no `package`, `path`, or `command` argument.

Expected behavior:

```text
- if previous failure exists: rerun and return compact report
- if not: ToolError from resolver with clear message
```

Do not synthesize a prompt telling the LLM to inspect logs. This path remains deterministic.

### `/test previous`

Ensure command parser maps `/test previous` to `TestScope::PreviousFailures` and uses the same runner path as other `/test` commands.

If no prior failure is available, the UI should show a clear message in the command result/status area and should not start an LLM turn.

### Acceptance criteria

```text
- `test` tool supports previous_failures end-to-end.
- `/test previous` supports previous-failure rerun end-to-end.
- No LLM turn is created for `/test previous`.
- Resolver errors are displayed clearly.
```

## Report metadata

Update `TestReport` or report formatting only if needed. Avoid broad type churn.

Useful additions:

```rust
pub previous_run_id: Option<String>
pub scope_label: String
```

If `scope_label` already exists on `ResolvedTestCommand` and is visible in reports, reuse it.

Report text for previous failure reruns should include a concise line:

```text
Scope:
previous-failures:<run_id>
```

or:

```text
Rerun source:
previous failed run <run_id>
```

Do not embed full prior failure details in the new report. The index is only for selecting the rerun command.

### Acceptance criteria

```text
- Rerun reports clearly show that the scope came from a previous failure.
- Full prior log output is not inserted into the model-facing report.
```

## Tests

### Index module tests

Add tests under `src/test_runner/index.rs`:

```text
index_load_missing_returns_empty_or_missing_error_as_designed
index_load_malformed_returns_error
index_append_creates_file
index_append_bounds_entries_to_max
index_entry_truncates_summary_and_failure_messages
index_newest_actionable_failure_selected
index_skips_passed_entries
index_skips_invalid_empty_argv
index_skips_missing_cwd
```

### Resolver tests

Add tests under `src/test_runner/resolve.rs`:

```text
resolver_previous_failures_missing_index_is_clear
resolver_previous_failures_malformed_index_is_clear
resolver_previous_failures_selects_newest_failed_entry
resolver_previous_failures_selects_timed_out_entry
resolver_previous_failures_skips_passed_entry
resolver_previous_failures_skips_stale_cwd
resolver_previous_failures_rejects_invalid_argv
```

### Runner integration tests

Add a test using a temporary project or simple deterministic command already used by runner tests:

```text
runner_writes_index_after_failed_run
runner_writes_index_after_timed_out_run
runner_index_failure_does_not_fail_report
```

### Tool/TUI tests

Add or update:

```text
test_tool_previous_failures_maps_scope
test_tool_previous_failures_missing_index_returns_tool_error
tui_test_previous_maps_scope
tui_test_previous_missing_index_displays_error
```

### Acceptance criteria

```text
- Index behavior is covered in unit tests.
- Resolver behavior is covered without spawning real long-running tests.
- Runner write path has at least one integration-style test.
- Tool and TUI mappings are covered.
```

## Documentation updates

Update:

```text
architecture/test_runner.md
architecture/tool.md
architecture/command.md
README.md if `/test previous` is documented there
AGENTS.md validation commands
```

Docs should state:

```text
- `.codegg/test-runs/index.json` stores bounded metadata for supervised test runs
- previous-failures rerun selects the newest actionable failed/timed-out supervised test run
- full logs remain in per-run directories, not the index
- index data is validated before rerun
- malformed/stale index entries are skipped or produce clear errors
- cross-process index write locking is deferred if not implemented
```

## Validation commands

Run targeted validation:

```text
cargo fmt --check
cargo check
cargo test -p codegg --lib test_runner
cargo test -p codegg --lib tool::test
cargo test -p codegg --lib tui::commands::test
```

If module targeting is available:

```text
cargo test -p codegg --lib test_runner::index
cargo test -p codegg --lib test_runner::resolve
```

## Final acceptance checklist

This phase is complete when:

```text
- `TestScope::PreviousFailures` resolves from index metadata.
- `test` tool scope `previous_failures` works end-to-end.
- `/test previous` works end-to-end without invoking the LLM.
- Index writes are bounded and nonfatal on failure.
- Index reads validate commands and cwd before rerun.
- Malformed, stale, missing, and empty index cases are tested.
- Reports remain compact and do not include previous full logs.
- Docs describe index behavior and limitations.
- Targeted validation commands pass.
```

## Deferred follow-up

After this phase, reasonable future improvements are:

```text
- flaky retry policy keyed by failure signature
- richer per-test-node rerun command generation
- RTK/context-artifact integration for long-term test history
- garbage collection for old test-run directories
- cross-process index locking
```

Do not include those in this phase unless required by a correctness bug.
