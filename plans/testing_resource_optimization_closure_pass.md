# Testing Resource Optimization Closure Pass Plan

## Purpose

The first testing resource optimization pass landed the highest-value structural cleanup: duplicate shell projection CI reruns were removed, testing taxonomy documentation was added, initial nextest profiles were created, many async tests were converted to `current_thread`, and redundant snapshot migrations were removed.

This closure pass is for tightening the remaining gaps before treating the testing architecture as stable. The goal is not to maximize parallelism. The goal is to make the resource policy mechanically enforced, internally consistent, and resistant to regression.

Known constraints remain:

- Unbounded Rust test parallelism previously spawned approximately 50-70 threads plus many subprocesses.
- Some subprocesses/process-heavy paths can consume 1-2 GiB each.
- `--test-threads=1` remains a valid conservative full-suite baseline.
- LSP fake-stdio, plugin/Wasmtime, daemon/socket, and real-language-server tests must stay serial unless they are explicitly redesigned.

## Current State After First Pass

Implemented:

- CI no longer reruns shell projection tests after the full serial workspace test.
- `.config/nextest.toml` exists with `ci-fast`, `ci-heavy`, and `ci-release` profiles.
- `architecture/testing.md` documents resource classes and test authoring rules.
- Many bare `#[tokio::test]` annotations were converted to `#[tokio::test(flavor = "current_thread")]`.
- Snapshot tests no longer rerun migrations after `isolated_pool()`.
- LSP restart architecture documentation now supports mock/in-process testing through `RestartShared`.

Remaining risks:

- Normal CI still uses `cargo test --workspace --all-features -- --test-threads=1`; this is safe but still broad and may compile real-LSP support paths.
- Plugin-focused CI commands remain targeted but do not explicitly append `-- --test-threads=1`.
- Nextest profiles are present but not yet used for timing/resource observability or CI grouping.
- The large Tokio conversion needs a targeted audit for tests that use `tokio::spawn`, channel concurrency, background workers, timing, subprocess lifecycle, or socket/server behavior.
- Documentation says real-LSP tests are separate/manual/scheduled, but the workflow also runs on pushes to `main` touching LSP paths. That may be acceptable, but the docs should be precise.
- There is no guardrail to prevent future bare `#[tokio::test]` additions.

## Phase A: Make CI Resource Semantics Explicit

### A1. Add serial flags to plugin-focused tests

The plugin-focused job currently runs targeted plugin tests after the full serial test. Because plugin paths may instantiate Wasmtime/runtime state, make their serial behavior explicit.

Change:

```yaml
run: cargo test -p codegg --lib plugin::install --all-features
```

To:

```yaml
run: cargo test -p codegg --lib plugin::install --all-features -- --test-threads=1
```

Apply the same pattern to:

- `plugin::install`
- `plugin::management`
- `plugin::registry`
- `tui::commands::plugin_management`

Acceptance criteria:

- Every plugin-focused test command has `-- --test-threads=1`.
- `architecture/testing.md` and CI behavior agree that plugin-heavy tests are serial.
- The command still runs successfully locally.

### A2. Decide whether plugin-focused is duplicate validation or isolated diagnostics

Because the main test job runs `--workspace --all-features`, plugin-focused tests are likely already covered. Decide whether plugin-focused should remain as a diagnostic rerun or whether the main test lane should be narrowed in a later pass.

Options:

1. Keep plugin-focused as intentional duplicate diagnostics. Document that it exists for focused logs and core-boundary validation, not coverage uniqueness.
2. Remove the duplicate plugin test commands and keep only `scripts/check-core-boundary.sh` if the broad test lane remains all-features.
3. Narrow the broad test lane later and let plugin-focused own plugin coverage exactly once.

Recommended closure decision: keep plugin-focused for now, but document it as intentional targeted diagnostics until the main lane is split.

Acceptance criteria:

- CI comments or `architecture/testing.md` explain whether plugin-focused is duplicate-by-design or unique coverage.
- If duplicate-by-design, it remains serial and focused.

### A3. Align real-LSP documentation with workflow triggers

`architecture/testing.md` should not imply real-LSP tests are only manual/weekly if `lsp-real-server.yml` also runs on `push` to `main` for LSP path changes.

Update docs to say:

- Real-server compatibility is outside default PR CI.
- It runs manually, weekly, and on main pushes affecting LSP paths.
- It must not be pulled into routine PR validation.

Acceptance criteria:

- The docs accurately describe `workflow_dispatch`, weekly schedule, and main-path push behavior.
- The docs explicitly state that PR validation should not run real-language-server tests by default.

## Phase B: Isolate Real-LSP Feature Exposure

### B1. Verify `--all-features` behavior for real-LSP tests

Because root `all-features` includes `lsp-real-server-tests`, verify whether the default CI full serial command compiles and/or runs `crates/egglsp/tests/real_server_smoke.rs`.

Run locally or in CI diagnostic mode:

```bash
cargo test --workspace --all-features -- --list | rg 'real_server|rust_analyzer|basedpyright|gopls|clangd|typescript'
```

Also test:

```bash
cargo test -p egglsp --all-features --test real_server_smoke -- --list
```

Acceptance criteria:

- We know whether `real_server_smoke` is compiled/listed under routine all-features.
- If it is listed, confirm whether every test skips unless the requested server binary is available.
- Capture this behavior in `architecture/testing.md`.

### B2. Consider feature split: `lsp-real-server-tests` should not be part of normal all-features lanes

If all-features compiles/runs real-server smoke tests in the normal CI lane, consider restructuring features so release/manual real-LSP compatibility is separate from routine `--all-features`.

Potential approaches:

1. Keep `lsp-real-server-tests` as-is, but ensure `real_server_smoke` tests are `#[ignore]` and only run with explicit filters/workflows.
2. Move real-server tests to a separate package or separate workflow command that does not depend on root `--all-features`.
3. Avoid using `--all-features` in routine CI once feature-matrix lanes exist.

Recommended near-term closure: do not restructure features immediately unless tests unexpectedly run real servers in routine CI. First document and verify actual behavior.

Acceptance criteria:

- Routine PR CI cannot unexpectedly launch real language servers.
- The separate real-LSP workflow remains the authoritative compatibility lane.

## Phase C: Audit Broad Tokio Runtime Conversion

### C1. Inventory concurrency-sensitive tests now using `current_thread`

Search for tests that use any of these inside a `#[tokio::test(flavor = "current_thread")]` module/test:

- `tokio::spawn`
- `spawn_blocking`
- `tokio::process`
- `tokio::sync::broadcast`
- `tokio::sync::mpsc`
- `tokio::sync::oneshot`
- `tokio::time::sleep`
- `timeout(`
- socket/server start helpers
- fake LSP server helpers
- daemon transport helpers
- shell command execution helpers

Use a script rather than manual grepping so the results can be repeated. Example:

```bash
python3 scripts/audit_tokio_tests.py
```

If no script exists, add one under `scripts/` that reports candidate files and line numbers without failing CI initially.

Acceptance criteria:

- A repeatable audit report identifies concurrency-sensitive `current_thread` tests.
- Each candidate is classified as safe-current-thread, needs-multi-thread, or process-heavy-serial.

### C2. Restore explicit multi-thread runtimes where required

For tests that genuinely require concurrent scheduler workers, change:

```rust
#[tokio::test(flavor = "current_thread")]
```

to:

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
```

Do this only for tests where single-thread scheduling can hide race behavior or cause deadlock/starvation.

Likely candidates:

- server/WebSocket lifecycle tests,
- daemon socket integration tests,
- LSP supervisor/restart tests that spawn background work,
- shell execution tests with subprocess lifecycle,
- agent loop tests with multiple concurrent tasks.

Acceptance criteria:

- Every restored multi-thread test has an explicit small worker count.
- No test uses implicit Tokio runtime flavor.
- The broad serial command still passes.

### C3. Add a regression guard for bare `#[tokio::test]`

Add a script such as `scripts/check-tokio-test-flavors.py` that fails if it finds bare `#[tokio::test]` without `flavor = ...`, except in an allowlist.

Wire it into either:

- `agent-assets`,
- a lightweight `test-policy` CI job,
- or `scripts/check-core-boundary.sh` if that is the repository’s existing policy-check pattern.

Acceptance criteria:

- New bare Tokio tests are caught in CI.
- The allowlist, if any, is small and documented.

## Phase D: Start Using Nextest for Measurement, Not Immediate Replacement

### D1. Add a manual nextest timing workflow or documented command

Nextest is currently configured but unused. Add one of:

- a manual workflow, e.g. `.github/workflows/test-timing.yml`, or
- a documented local command in `architecture/testing.md` and `AGENTS.md`.

Manual workflow example:

```yaml
name: Test Timing
on:
  workflow_dispatch:

jobs:
  nextest-timing:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@nextest
      - run: cargo nextest run --workspace --profile ci-heavy --all-features
```

If adding nextest installation is undesirable, keep it as local documentation only for now.

Acceptance criteria:

- Maintainers have a repeatable way to get slow-test timing data.
- Nextest is not yet required for the default CI pass unless the team chooses to adopt it.

### D2. Capture baseline metrics

For at least one run, record:

- total wall-clock time,
- slowest 20 tests,
- peak memory if available,
- any tests that hit slow-timeout warnings,
- whether nextest output identifies obvious subprocess-heavy bottlenecks.

Store findings in either:

- a new `plans/testing_resource_metrics_notes.md`, or
- an appendix in `architecture/testing.md`.

Acceptance criteria:

- The next optimization pass is data-driven rather than based only on code inspection.

## Phase E: Storage Test Follow-Up

### E1. Confirm no redundant migrations remain after `isolated_pool()`

Search for patterns where tests call `common::pool::isolated_pool().await` followed by `schema::migrate(&pool)`.

Suggested search:

```bash
rg 'isolated_pool\(\).*|schema::migrate|migrate\(&pool' tests crates src
```

Acceptance criteria:

- No accidental double migration remains except explicit migration-idempotency tests.
- Any intentional double migration has a comment explaining why.

### E2. Identify shared-pool candidates

The first pass removed redundant migrations but did not convert storage tests to `shared_pool()`. Do not rush this. Identify low-risk candidates first:

- tests that already use generated UUIDs or unique namespaces,
- tests that only append and clean up their own rows,
- tests that do not assert exact global counts or absence of rows.

Acceptance criteria:

- A short candidate list exists for future shared-pool migration.
- No shared-pool conversion is made without cleanup/namespace guarantees.

## Phase F: LSP Subprocess Reduction Follow-Up

### F1. Use `RestartShared` mock path for at least one restart behavior

The `egglsp::restart` design now supports lightweight mock implementations. Add or identify tests that validate restart behavior without spawning a fake server process.

Good candidates:

- restart ownership acquisition/release,
- cancellation intent versus completion semantics,
- stale generation rejection,
- restart budget exhaustion,
- readiness degraded outcome.

Acceptance criteria:

- At least one behavior currently covered by a subprocess test is also covered by a pure/mock restart coordinator test.
- This does not replace stdio/process lifecycle coverage; it reduces the need to add more subprocess tests for pure coordinator logic.

### F2. Mark process-heavy test files in docs or comments

For files such as `tests/lsp_composite_stdio.rs`, `crates/egglsp/tests/supervisor_restart_stdio.rs`, and real-server smoke tests, add short header comments or docs indicating:

- resource class: process-heavy or real-lsp,
- must run serially,
- do not broaden coverage here if a mock/in-process test can cover the behavior.

Acceptance criteria:

- Future contributors see the resource class before adding more subprocess-heavy tests.

## Phase G: CI Lane Roadmap Decision

After the above closure steps, decide whether to keep the current conservative CI topology or split it.

### Option 1: Conservative keep

Keep:

```bash
cargo test --workspace --all-features -- --test-threads=1
```

as the primary CI test lane, with improved docs and policy checks. This is simplest and safest, but wall-clock remains high.

### Option 2: Split but safe

Replace the monolithic test lane with sequential resource lanes:

1. fast/default lane with low concurrency,
2. storage lane serial or low concurrency,
3. process-heavy LSP lane serial,
4. plugin-heavy lane serial,
5. release-full serial on main/tags/manual.

Do not run heavy lanes concurrently on constrained runners.

Acceptance criteria for choosing split:

- Timing data shows meaningful wall-clock savings.
- Peak memory/process count does not regress.
- Coverage mapping is documented.
- Release-full remains available.

## Validation Commands

Minimum closure validation:

```bash
cargo fmt --check --all
cargo check --workspace --all-features --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1
```

Targeted validation:

```bash
cargo test -p codegg --lib plugin::install --all-features -- --test-threads=1
cargo test -p codegg --lib plugin::management --all-features -- --test-threads=1
cargo test -p codegg --lib plugin::registry --all-features -- --test-threads=1
cargo test -p codegg --lib tui::commands::plugin_management --all-features -- --test-threads=1
cargo test -p egglsp --features lsp-test-support --test scenario_engine -- --test-threads=1
cargo test -p egglsp --features lsp-test-support --test supervisor_restart_stdio -- --test-threads=1
cargo test --features lsp-test-support --test lsp_composite_stdio -- --test-threads=1
```

Policy/audit validation once scripts exist:

```bash
python3 scripts/check-tokio-test-flavors.py
python3 scripts/audit_tokio_tests.py
```

## Definition of Done

This closure pass is complete when:

- CI and docs agree on which tests are serial, duplicated intentionally, manual, or release-only.
- Plugin-focused tests are explicitly serial or deliberately removed as duplicate coverage.
- Real-LSP workflow behavior is accurately documented and cannot surprise routine PR CI.
- There is a repeatable audit for bare Tokio tests and concurrency-sensitive `current_thread` tests.
- Any necessary multi-thread Tokio tests use explicit `worker_threads`.
- Nextest is either used for manual timing or clearly documented as optional measurement tooling.
- Storage tests do not redundantly rerun migrations after `isolated_pool()`.
- At least one LSP restart/coordinator behavior is covered without adding more subprocess pressure.

The final state should retain the safe full serial command while making the resource budget enforceable and progressively more granular.
