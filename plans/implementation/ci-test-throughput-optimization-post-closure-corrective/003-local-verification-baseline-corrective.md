# CI/Test Throughput Post-Closure Corrective C003 — Local Verification Baseline Corrective

Status: closed

Closure record: `plans/closure/ci-test-throughput-optimization-post-closure-corrective/003-status.md`

Repository baseline reviewed: `e3beebe3`

Source corrective addendum:

- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`

Predecessor closures:

- `plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-status.md` (C001 measurements; conditional on exactly the items below)
- `plans/closure/ci-test-throughput-optimization-post-closure-corrective/002-status.md` (closed; owns no part of these items)

Primary class: polish / development infrastructure.

## 1. Objective

Resolve the two named conditional items from C001's closure record so C001 — and with it the post-closure corrective line — can close strictly:

1. `scheduler_cancellation::cancel_running_job_terminates_process_and_releases_permit` fails deterministically on local Rust 1.89 (reproduced isolated). Root cause, diagnosed but left for separate triage: `Running` is persisted (`mark_attempt_running`) before the executor task spawns, and the spawned task legitimately short-circuits via the scheduler's "cancelled before executor start" fast path (`src/scheduler/scheduler.rs`) when the token is already cancelled. Cancelling immediately after observing `Running` therefore hits the pre-start path and the executor never runs, failing the `cancel_observed` assertion. The same race affects `cancel_subagent_interrupts_attempt` and `cancellation_token_propagated_to_executor`. No production defect is indicated; the fix is test-harness-only.
2. Local Rust 1.89 `cargo clippy --workspace --all-targets --locked -- -D warnings` reports findings while hosted stable Clippy passes. C001 recorded four; that count was partial because the C001 run stopped at the first failing crates, masking further 1.89 findings in `codegg-core` and the root crate. All are mechanical lint-level rewrites (collapsible `else`, De Morgan/slice narrowings, one associated-function conversion, three `|| true` tautologies in tests, documentary const guards kept with targeted allows). No behavior change.

## 2. Current implementation evidence

At baseline `e3beebe3`:

- `tests/scheduler_cancellation.rs::setup_with_executor` registers the custom executor via a detached `tokio::spawn` without awaiting.
- The three token-observation tests cancel immediately after observing `Running` with no executor-started gate.
- The full 1.89 Clippy finding inventory is the baseline this pass must clear (see WP2 for the file-level list).
- Hosted stable Clippy is green (four consecutive PR #80 runs), so these fixes change no hosted behavior; they align the local 1.89 baseline with the already-green gate.

## 3. Correctness and compatibility invariants

Do not change:

- production scheduler dispatch, cancellation, executor-selection, or pre-start fast-path semantics;
- product code runtime behavior (Clippy fixes are semantics-preserving; the three `|| true` removals restore the evidently intended assertions only with focused-test proof, else stop and record);
- test assertions except started-gate additions and tautology removals proven by green focused suites; no coverage deletion;
- workflow, selectors, topology, Nextest policy, live-qualification policy.

## 4. Work packages

### WP1 — Deterministic scheduler-cancellation harness (test-only)

In `tests/scheduler_cancellation.rs`:

- `setup_with_executor`: await `register_executor` directly instead of the detached spawn.
- Add executor-started flags (`Arc<AtomicBool>` set on entry to `execute()`) to `SleepExecutor`, `FakeSubagentExecutor`, `CancellationObservingExecutor`; tests 1, 6, 8 wait for the flag (bounded 5s) after observing `Running` and before `request_cancel`.
- Test 8's pre-cancelled read stays after the gate, making it deterministic.
- Tests 2 (pre-admission) and 7 (queued) keep covering the genuine pre-start path untouched.

### WP2 — Rust 1.89 Clippy baseline to green (no behavior change)

Fix every finding required for green `cargo clippy --workspace --all-targets --locked -- -D warnings` under local 1.89:

- `crates/codegg-providers/src/sse_parser.rs`: collapse `else { if let }`.
- `crates/egglsp/src/cache.rs`: `!is_some_and` → `is_none_or`.
- `crates/egglsp/src/evidence_collector.rs`: unused `_notes: &mut Vec<String>` → `&mut [String]`.
- `crates/codegg-providers/src/openai_compatible.rs`: remove `|| true` tautology; prove with the focused bearer test (stop and record a leak finding if it fails).
- `crates/codegg-core/src/jobs/{schedule,mod}.rs`: `_ensure_pathbuf_used` anchors keep `&PathBuf` intentionally with targeted `#[allow(clippy::ptr_arg)]`.
- `crates/codegg-core/src/snapshot/diff.rs`: collapse `else { if } else`.
- `crates/codegg-core/src/tool_program/interpreter.rs`: `values_equal` becomes an associated function.
- Root crate: seven mechanical `else` collapses (`agent/request_preparation.rs`, `exec.rs`, `mcp/ide_server.rs`, `tui/commands/work_orders.rs`, `tui/components/dialogs/{connect,model,permission}.rs`); `managed_process.rs` `&PathBuf` → `&Path`; `plugin/manifest.rs` single-negation flip; `scheduler/permit.rs` direct return; `tui/app/mod.rs` `find/is_none` → `!any`; documentary const guards in `agent/definition.rs` + `test_runner/custom.rs` kept with targeted `#[allow(clippy::const_is_empty)]`.
- `tests/context_continuity_m004.rs`: two further `|| true` tautologies removed and proven by the focused binary (stop and record if either fails).

### WP3 — Closure and handoff to C001

Create `plans/closure/ci-test-throughput-optimization-post-closure-corrective/003-status.md`, mark C003 closed, and hand C001 its final-closure precondition (zero residual medium+ findings in C001's scope).

## 5. Required verification

```bash
git diff --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo nextest run -p codegg --test scheduler_cancellation --locked --profile ci
cargo nextest run -p codegg-providers --locked --profile ci
cargo nextest run -p egglsp --locked --profile ci
cargo nextest run -p codegg-core --locked --profile ci
cargo nextest run -p codegg --test context_continuity_m004 --locked --profile ci
cargo nextest run -p codegg --test session_family --locked --profile ci -E 'test(reaper_)'
scripts/verify.sh quick
```

The isolated Test-1 command from C001's record must pass three consecutive times. Hosted evidence: none required (no workflow/product change); the next PR #80 push will incidentally re-qualify the full suite.

## 6. Failure, cancellation, restart, and contention semantics

Started-gate waits are bounded (5s) and fail loudly. No scheduler-queue, permit, or contention posture change.

## 7. Security and supply-chain constraints

No new dependencies, actions, secrets, or permissions. The bearer-sentinel fix asserts on containment only.

## 8. Documentation effects

New: this plan + the C003 closure record. Updated: addendum (C003 section + table), registry (C003 row). Untouched unless stale: `architecture/testing.md`, workflow, Nextest config. Historical C001/C002/M001-M005 records immutable.

## 9. Acceptance criteria

- The three token-observation tests pass repeatedly (3 consecutive isolated Test-1 runs) plus the full binary green.
- Full workspace Clippy green under local 1.89.
- All focused suites green, including the strengthened bearer/trajectory assertions.
- `scripts/verify.sh quick` passes.
- Diff shows only the named files plus planning records.
- C003 closure records matrix, unblock audit (expected: C001 final closure unblocked, nothing else), and residual findings (expected: none).

## 10. Stop conditions

Stop and report rather than widen scope if the unweakened bearer/trajectory assertions fail, a Clippy fix needs behavior change, scheduler tests fail even with the gate (production defect, not harness race), or green requires coverage reduction.

## 11. Closure evidence required

Focused/broad commands and outcomes, 3x repeats, Clippy outcome, quick outcome, proof of no history rewrites, findings by severity, C001 handoff state.
