# Testing Resource Optimization Handoff Plan

## Context

The current CI configuration intentionally runs the broad Rust test suite with a single libtest worker:

```bash
cargo test --workspace --all-features -- --test-threads=1
```

That cap is not accidental. Unbounded test execution has been observed to spawn roughly 50-70 threads plus many subprocesses, with some processes consuming 1-2 GiB of memory. On the target build system this creates memory pressure and I/O wait rather than improved throughput. The goal of this plan is therefore not to remove serialization globally. The goal is to replace the current coarse global cap with an explicit resource model: keep process-heavy tests serialized, recover safe parallelism for cheap tests, reduce duplicated work, and make slow/resource-heavy behavior visible.

The repository currently has several categories of tests with very different resource profiles:

- Cheap pure/unit tests and fixture-only tests.
- SQLite/session/snapshot tests that repeatedly create isolated in-memory databases and run migrations.
- Shell projection tests that are mostly in-memory fixture corpus tests.
- LSP fake-stdio tests that spawn fake language-server subprocesses, create temp Rust workspaces, write scenario/transcript files, and exercise async shutdown/restart behavior.
- Plugin/Wasmtime tests that may instantiate heavyweight runtime state.
- Real language-server tests that are feature-gated but can still compile or run under broad all-features invocations.

The current workflow mixes these into one all-features serial lane, then reruns several shell projection tests after the broad pass. That protects the machine but inflates wall-clock time and hides where the real cost sits.

## Desired End State

Codegg should have a documented, reproducible test taxonomy with bounded resource classes:

1. `fast`: pure/unit tests, cheap crate tests, config/protocol/provider logic, shell projection fixture tests if profiling confirms they are cheap.
2. `storage`: SQLite/session/snapshot tests; concurrency limited unless all tests are migrated to unique IDs, transaction rollback, or cleaned shared pools.
3. `process-heavy`: fake LSP stdio, supervisor/restart/shutdown, daemon socket, server/WebSocket, tool execution that spawns subprocesses.
4. `plugin-heavy`: Wasmtime/plugin install/registry/management/TUI plugin command tests.
5. `real-lsp`: actual rust-analyzer/pyright/gopls/clangd/etc. compatibility smoke tests; manual or scheduled, not routine PR default.
6. `release-full`: conservative validation that may still run serially, intended for main/tags/manual release validation.

The resource model should be enforced by CI commands and optionally by `cargo nextest` groups. A future maintainer should be able to run fast feedback locally without accidentally launching dozens of multi-GiB subprocesses.

## Phase 1: Remove Obvious Duplicate Work While Preserving Serial Safety

### 1.1 Stop rerunning tests already covered by the broad test command

The current CI `test` job runs the whole workspace with all features and `--test-threads=1`, then separately reruns:

```bash
cargo test --all-features --test shell_projection_harness
cargo test --all-features --test shell_projection_phase10
cargo test --all-features --lib shell::redactor
cargo test --all-features --lib shell::rtk
```

If the broad command remains in place, these follow-up commands are duplicate execution unless the intent is to isolate logs. Replace them with either:

- no rerun at all, if broad test remains authoritative; or
- a split workflow where the broad command excludes shell projection tests and the named shell lane runs them exactly once.

Recommended minimal patch:

```yaml
- run: cargo test --workspace --all-features -- --test-threads=1
# Remove shell projection reruns here. Keep a separate shell-focused job only if the broad command is narrowed.
```

Acceptance criteria:

- CI still executes shell projection tests at least once.
- CI no longer executes `shell_projection_harness`, `shell_projection_phase10`, `shell::redactor`, or `shell::rtk` twice in the same workflow run.
- CI job summary or documentation explains where shell projection coverage lives.

### 1.2 Keep the current serial full command as a known-safe baseline

Do not remove `--test-threads=1` in the first pass. Treat it as the known-safe full validation command until a more granular resource model is implemented and measured.

Acceptance criteria:

- `cargo test --workspace --all-features -- --test-threads=1` remains available as the conservative local/full validation command.
- Documentation notes that this command intentionally trades wall-clock time for bounded memory/process count.

## Phase 2: Add Test Resource Classification Documentation

Create a test policy document, preferably `docs/testing.md` or `architecture/testing.md`, with these sections:

- Why unbounded Rust test parallelism is unsafe for this repository.
- Which test families are considered cheap versus resource-heavy.
- Expected local commands for fast feedback, full local validation, heavy LSP validation, plugin validation, and real-server validation.
- Rules for adding new tests:
  - Use `#[tokio::test(flavor = "current_thread")]` unless the test explicitly requires a multi-thread runtime.
  - If a multi-thread runtime is necessary, specify `worker_threads = 2` or another small explicit value.
  - Do not spawn real language servers in default tests.
  - Do not use fixed global paths, fixed ports, or shared environment variables without serializing the test.
  - Prefer deterministic fake transports over subprocesses where process lifecycle is not the behavior under test.
  - Keep long timeouts as failure bounds only; do not use sleeps as synchronization if a deterministic event can be awaited.

Acceptance criteria:

- The testing document distinguishes `fast`, `storage`, `process-heavy`, `plugin-heavy`, `real-lsp`, and `release-full` classes.
- The document explicitly states that `--test-threads=1` was intentional due to observed thread/process/memory amplification.
- New-test guidance includes Tokio runtime flavor rules.

## Phase 3: Introduce Measured Test Timing and Resource Visibility

Before changing concurrency, add measurement.

### 3.1 Add optional `cargo nextest` support

Add a `.config/nextest.toml` file with initial profiles. Keep this additive; do not immediately remove the existing `cargo test` lane.

Suggested initial shape:

```toml
[profile.default]
slow-timeout = { period = "30s", terminate-after = 2 }

[profile.ci-fast]
failure-output = "immediate-final"
success-output = "never"
slow-timeout = { period = "20s", terminate-after = 2 }

[profile.ci-heavy]
failure-output = "immediate-final"
success-output = "never"
test-threads = 1
slow-timeout = { period = "60s", terminate-after = 2 }

[profile.ci-release]
failure-output = "immediate-final"
success-output = "never"
test-threads = 1
slow-timeout = { period = "120s", terminate-after = 2 }
```

If nextest grouping is used, add groups for heavyweight tests. Exact syntax should be validated against the nextest version used by CI before committing. If group syntax is uncertain, first add simple profiles and use separate command invocations to isolate heavy tests.

### 3.2 Add a timing-only CI/manual job

Add a manual workflow or optional job that runs nextest in list/timing mode and emits slow-test information. This should be safe to run manually when diagnosing regressions.

Acceptance criteria:

- Maintainers can run a command that reports slow tests without changing the normal safe CI behavior.
- The repository has an initial nextest profile file or a documented reason to defer it.
- Heavy tests are identifiable by name/module even before full group enforcement.

## Phase 4: Split CI Into Safe Lanes Without Increasing Peak Resource Use

The next CI shape should preserve a serial heavy lane while avoiding one monolithic all-features lane for everything.

Recommended structure:

```yaml
jobs:
  test-safe-full:
    # Conservative baseline. Can remain until split lanes prove stable.
    run: cargo test --workspace --all-features -- --test-threads=1

  test-fast:
    # Introduce only after profiling. Keep low concurrency.
    run: cargo test --workspace --no-default-features -- --test-threads=2

  test-plugin-focused:
    # Already mostly exists; keep serial if Wasmtime memory is high.
    run: |
      cargo test -p codegg --lib plugin::install --all-features -- --test-threads=1
      cargo test -p codegg --lib plugin::management --all-features -- --test-threads=1
      cargo test -p codegg --lib plugin::registry --all-features -- --test-threads=1
      cargo test -p codegg --lib tui::commands::plugin_management --all-features -- --test-threads=1

  test-lsp-fake:
    # Run fake-stdio and supervisor tests serially.
    run: |
      cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio -- --test-threads=1
      cargo test -p egglsp --features lsp-test-support --test production_semantic_stdio -- --test-threads=1
      cargo test -p egglsp --features lsp-test-support --test production_service_stdio -- --test-threads=1
      cargo test -p egglsp --features lsp-test-support --test supervisor_restart_stdio -- --test-threads=1
      cargo test -p codegg --features lsp-test-support --test lsp_composite_stdio -- --test-threads=1

  test-real-lsp:
    # Manual or scheduled only.
    if: github.event_name == 'workflow_dispatch'
    run: cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- --test-threads=1
```

Do not run all of these in parallel until peak memory is known. If GitHub Actions or the local runner runs jobs concurrently, add `needs:` dependencies or workflow-level concurrency limits so `test-lsp-fake`, `plugin-focused`, and release builds do not overlap on small machines.

Acceptance criteria:

- Heavy LSP/plugin lanes remain serial.
- Cheap lanes may use low concurrency only after measurement.
- Real-server smoke tests are not part of default PR validation.
- CI no longer compiles/runs real-server compatibility tests merely because all features were requested in a routine lane, unless that lane is explicitly release/manual.

## Phase 5: Cap Tokio Runtime Width in Tests

Audit async tests for Tokio runtime flavor. The default should be `current_thread` unless a test genuinely requires concurrent worker threads.

### 5.1 Convert cheap async tests

For async tests that only await storage calls, config calls, in-memory services, or deterministic futures, convert:

```rust
#[tokio::test]
async fn test_name() {
    // ...
}
```

to:

```rust
#[tokio::test(flavor = "current_thread")]
async fn test_name() {
    // ...
}
```

Candidates likely include storage/session/snapshot/config/provider/parser tests. Validate each conversion with the conservative full command.

### 5.2 Explicitly bound truly multi-threaded async tests

For tests that need multi-thread scheduling, background workers, cancellation races, or supervisor tasks, use:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_name() {
    // ...
}
```

Do not leave heavyweight tests with implicit runtime worker counts.

Acceptance criteria:

- No async test uses an implicit Tokio runtime unless there is a documented reason.
- Process-heavy async tests have explicit worker counts.
- Local full test peak thread count is measurably lower than before.

## Phase 6: Reduce SQLite Migration and Pool Setup Churn

The shared pool helper already documents two modes: a process-wide in-memory pool with migrations once, and isolated in-memory pools that rerun migrations per call. The current tests appear to mostly use `isolated_pool()` and some snapshot helpers call migrations again after isolated pool creation.

### 6.1 Remove redundant migrations

Where `common::pool::isolated_pool().await` is used, do not immediately call `codegg::session::schema::migrate(&pool)` again unless the test is specifically asserting migration idempotency.

Acceptance criteria:

- Snapshot/session/storage tests do not run migrations twice for the same newly isolated pool.
- Migration idempotency, if desired, is covered by a single explicit migration test.

### 6.2 Migrate safe storage tests to shared pool or transaction rollback

For storage tests that only require seeded project/session rows and use unique IDs, prefer a shared in-memory pool plus cleanup or a transaction rollback harness. Keep isolated pools for tests that assert absence of pre-existing data or migration behavior.

Possible helper additions:

```rust
pub async fn seeded_shared_pool(test_namespace: &str) -> &'static SqlitePool
```

or

```rust
pub async fn with_rollback_pool<F, Fut>(f: F)
where
    F: FnOnce(&mut Transaction<'_, Sqlite>) -> Fut,
```

The transaction approach may require store APIs that can accept an executor/transaction. If that refactor is too invasive, use unique IDs plus cleanup first.

Acceptance criteria:

- Storage tests are classified as `isolated-required` or `shared-safe`.
- Shared-safe tests stop running the full migration set per test.
- Test isolation remains deterministic under `--test-threads=1` and low parallelism.

## Phase 7: Reduce LSP Subprocess Churn Without Losing Coverage

LSP fake-stdio tests are expensive because they validate process lifecycle and JSON-RPC framing through real subprocesses. Keep those tests, but avoid using subprocesses for logic that can be validated in process.

### 7.1 Split LSP coverage by responsibility

Create or formalize these layers:

1. Pure state-machine tests: restart policy, generation checks, stale event handling, state transitions. No subprocess.
2. Fake transport tests: JSON-RPC request/response behavior without OS process lifecycle. No subprocess.
3. Fake stdio subprocess tests: process spawn, shutdown, forced kill, stderr tail, transcript behavior. Serial only.
4. Real-server compatibility tests: manual/scheduled only.

### 7.2 Consolidate scenarios where safe

Some root composite LSP tests spawn a fresh fake server only to validate constructibility or a basic operation. Where one initialized harness can safely support multiple assertions without hiding failure diagnostics, combine them into a single scenario/test. Do not consolidate tests that intentionally exercise independent crash/restart states unless the combined test preserves clear diagnostics.

Acceptance criteria:

- At least one restart/state-machine behavior is covered without spawning a fake-server subprocess.
- Fake stdio tests are reserved for stdio/process semantics.
- LSP test documentation states which layer a new LSP test belongs to.

## Phase 8: Tighten Deterministic Timeouts and Sleeps

Current LSP supervisor and real-server tests use long failure bounds. Long bounds are acceptable for real-server/manual compatibility, but deterministic fake-server tests should not routinely wait 10-15 seconds.

Actions:

- Replace sleeps with event-driven waits where possible.
- For fake-server deterministic tests, reduce common bounds to 1-3 seconds after verifying stability on the target build system.
- Keep longer bounds only around known slow shutdown/kill paths or real-server readiness/indexing.
- Add diagnostic output on timeout that includes scenario name, transcript tail, process start count, operational state, generation, and stderr tail.

Acceptance criteria:

- Deterministic fake-server tests fail quickly when the expected event does not occur.
- Real-server tests retain appropriately generous bounds but stay outside default CI.
- Timeout diagnostics remain actionable.

## Phase 9: Feature Matrix Cleanup

Routine use of `--all-features` pulls in plugin, image, server, LSP test support, and real-server feature gates. This is useful for release validation, but excessive for every feedback cycle.

Actions:

- Define feature-specific checks:
  - default features
  - `server`
  - `plugins`
  - `image`
  - `lsp-test-support`
  - `lsp-real-server-tests` only manual/scheduled
- Avoid `--all-features` in fast PR tests.
- Keep one release/manual `--all-features` serial command until feature matrix coverage is trusted.

Acceptance criteria:

- Fast CI does not compile Wasmtime/image/LSP real-server code unless relevant files or explicit lanes require it.
- Release/manual CI still provides all-features confidence.
- README or testing docs describe which command maintainers should run before release.

## Phase 10: Validation and Rollout Sequence

Implement this in small commits to avoid destabilizing CI:

1. Remove duplicate shell projection reruns after the broad all-features serial test.
2. Add testing architecture documentation.
3. Add nextest config and a manual timing workflow or documented local timing command.
4. Convert obvious async tests to `current_thread` in small batches.
5. Remove redundant migration calls after `isolated_pool()`.
6. Split CI into safe lanes while keeping the original serial full lane as a fallback.
7. Add process-heavy and real-server lanes with explicit serial execution.
8. Start replacing subprocess-heavy LSP logic tests with in-process/fake-transport tests.
9. Revisit whether the broad serial full lane can move from every PR to main/manual once lane coverage is proven.

At each step, record:

- total wall-clock time,
- peak memory if available,
- process count/thread count if available,
- slowest tests,
- any flakes or timeout regressions.

## Success Metrics

A successful implementation should show:

- Lower total CI wall-clock time without increasing peak memory pressure.
- No recurrence of 50-70 uncontrolled test threads/processes on the constrained build system.
- No duplicate test execution within a single workflow.
- Heavy LSP/plugin/process tests still serialized or otherwise explicitly resource-capped.
- Cheap tests able to run with limited safe concurrency.
- Real-server compatibility tests moved out of default PR validation.
- Storage tests avoiding redundant migrations.
- Tokio async tests using explicit runtime flavors and worker counts.

## Non-Goals

This plan does not require removing `--test-threads=1` from full validation. It also does not require making every test parallel-safe. Some tests should remain serial because they intentionally exercise process lifecycle, restart timing, global state, or heavyweight runtime behavior.

The central change is to make the resource policy explicit and granular rather than implicit and global.

## Follow-Up: Shell Projection Fixture Review

After completing this plan, a follow-up review of the shell projection fixture corpus was conducted. The review found 19 existing fixtures covering ~60% of redaction rules and native projectors, with 4 of 6 redaction rules and GitLogProjector having no fixture coverage at all.

See **[Shell Projection Fixture Corpus Review](shell-projection-fixture-review.md)** for the full analysis and plan to add 12 new fixtures covering the gaps.
