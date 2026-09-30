# C002 Closure — Hosted CI Timing-Flake Stabilization

Status: closed

Source implementation plan: `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/002-hosted-ci-timing-flake-stabilization.md`

Source subsystem roadmap: `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`

Repository baseline reviewed: `9c88b9b80852022b39daa80a6b9fb7797fcaa824`

Implementation commits (branch `ci-throughput-c001-measurements`, PR #80):

- `a3e36910` plans(ci): reconcile C001/C002 status after rebase
- `9ef15bb0` implement C002 reaper readiness and helper diagnostics

## 1. Executive finding

C002 is closed. The turn-reaper lost-event race is fixed at the production lifecycle boundary with deterministic regression coverage; Eggwork helper-startup failures are now phase-diagnostic with guaranteed child cleanup; the session-family consolidation is exonerated; and the final configuration passed three consecutive full hosted `CI / verify` executions with live Eggwork included. No assertion was weakened, no coverage deleted, no timeout blindly inflated (the 60s readiness bound is unchanged), and no sccache experiment begun. C001 is unblocked back to ready.

## 2. Defect classification

- Controller lost-event race: **fixed** (`src/core/daemon_control.rs`; WP1 + WP2).
- Eggwork readiness flake: **bounded with evidence** — phase diagnostics installed (WP3); the stall did not reproduce across three full hosted executions, so the existing 60s bound is retained without change (WP4, outcome: no remedy warranted).
- Session-family consolidation: **exonerated** — negative coupling audit (WP5); family left intact.
- Hosted stability: **qualified** — three consecutive greens (WP6).

## 3. Code-level explanation

Pre-fix, `CoreDaemon::spawn_turn_reaper` cloned the event log, spawned the reaper task, and created the broadcast receiver *inside* the spawned future. The caller received no guarantee the task had reached `subscribe()` before a following terminal publication. Broadcast senders do not buffer for not-yet-subscribed receivers, so a `TurnCompleted` published immediately after `spawn_turn_reaper` returned could be lost permanently — exactly the sequence the integration test performs. The one-second test poll could not repair a lost event. This was a production lifecycle race, not a flaky assertion.

Post-fix, the receiver is created synchronously in `spawn_turn_reaper` before `tokio::spawn` and moved into the task. Return from the function establishes: the terminal-event receiver for this session/turn already exists. Filtering, `Lagged` handling (skip stale, keep waiting), turn-match release, active-turn clearing, and `Idle` transition are unchanged.

## 4. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Subscriber readiness before return | `daemon_control.rs`: `subscribe()` precedes `spawn`; readiness contract documented on the method | Complete |
| Immediate-publication regression | New `reaper_releases_on_turn_failed_event`, `reaper_ignores_unrelated_terminal_events`, `reaper_immediate_publish_repeated` (10x alternating); they deterministically exercise the previously racy ordering (immediate publish, no yield) plus the uncovered TurnFailed/unrelated branches | Complete; logical transition asserted, bounded waits are failure bounds only |
| No weakened assertions | Existing `reaper_releases_on_turn_completed_event` untouched; `git diff` shows additions only in the `.inc` file | Complete |
| Phase-diagnostic readiness failures | Helper prints `PHASE args-parsed/tls-ready/storage-ready/runner-ready/server-starting`; harness timeout reports path, elapsed, liveness/exit status, last phase, last stdout, stderr tail, and kills/reaps the child | Complete |
| No blind timeout inflation | 60s bound unchanged | Complete |
| Session-family audit | No shared process-global state (per-test nextest processes, per-test in-memory pools, guarded env mutation); failure fully explained by the production race | Exonerated; family intact |
| Focused local verification | `cargo test --test session_family control_m004_controller_lease -- --test-threads=1`: 26/26; `nextest -E 'binary(session_family)'`: 96/96 | Complete |
| Broad local verification | `cargo fmt --check` clean; `verify.sh quick` green; `cargo check --workspace --all-targets` green | Complete |
| Hosted stability | Run `36252873923` attempts 1-3, same tree `9ef15bb0`, all green with live included | Qualified (3/3) |
| No sccache experiment | Workflow untouched by C002 | Complete |

## 5. Hosted stability evidence (WP6)

Run `36252873923` (PR #80, branch `ci-throughput-c001-measurements`, tree `9ef15bb0`), same final configuration, three consecutive full executions, no superseded/cancelled runs in the sequence:

| Attempt | Total | Test build/exec | Tests | Live | Clippy |
|---|---|---|---|---|---|
| 1 | 17m17s | exec 365.187s | 11,729 passed / 1 skipped | included (`live_required=true`) | green (hosted stable) |
| 2 (rerun) | 17m22s | exec 364.756s | 11,729 passed / 1 skipped | included | green |
| 3 (rerun) | 17m15s | exec 365.459s | 11,729 passed / 1 skipped | included | green |

Attempt-3 log confirms the two originally failing tests pass: `eggwork_remote_execution_live::live_blob_upload_and_workspace_materialization` PASS and `session_family::control_m004_controller_lease::reaper_releases_on_turn_completed_event` PASS (plus the three new reaper regression tests, which also passed in attempts 1-2 scope runs). Test count rose 11,726 → 11,729 exactly by the three added regression tests; no other scope change.

Reruns were used per the plan's permitted mechanism on the identical tree; no source change occurred between attempts.

## 6. Eggwork disposition (WP4)

The 60-second stall did not reproduce in any of the three qualification runs (or in the earlier C001 green runs after the helper-timeout incidents). With phase markers now installed, any future stall will report its exact phase. No startup-correctness bug was found; no runner-load latency was proven; no contention or upstream defect was identified. Per the permitted outcomes, the smallest evidence-supported remedy is **no change**: retain the 60s failure bound, keep the new diagnostics. A future timeout will arrive with actionable evidence instead of requiring another instrumentation pass.

Secret hygiene: timeout diagnostics contain only the helper path, elapsed time, liveness/exit status, phase names, stdout lines, and the bounded stderr tail. TLS material lives in temp files whose paths (not contents) may appear; the helper never prints key material (unchanged guarantee, see helper module docs). Timeout path kills and reaps the child (`kill` + `wait`) so failed qualifications cannot leak helpers (`kill_on_drop` remains as backstop).

## 7. Local verification detail

- `cargo test --test session_family control_m004_controller_lease -- --test-threads=1` — 26/26 green (includes the 3 new regression tests).
- `cargo nextest run --workspace --locked --profile ci -E 'binary(session_family)'` — 96/96 green.
- `git diff --check` / `cargo fmt --all -- --check` — clean.
- `scripts/verify.sh quick` — passed (all guards + workspace check).
- `cargo check --workspace --all-targets --locked` — green.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` under local Rust 1.89 reports pre-existing findings unrelated to C002's scope (unchanged by this pass; they fail identically on unmodified main). Hosted stable-toolchain Clippy passed in all three qualification runs, which is the CI gate. No C002 file introduced a new Clippy finding (verified: the implementation commit is Clippy-clean relative to its own baseline to the extent separable; remaining findings are pre-existing).
- Linux-only `eggwork_remote_execution_live` harness changes and the helper crate cannot compile on the local macOS host (Linux-only `landlock` dependency); they were reviewed line-by-line and are proven by the three green hosted Linux runs, which compile both.

## 8. Invariant review

- One `verify` job; `contents: read`; no release authority; `CARGO_BUILD_JOBS=4`; Nextest 4-slot + heavy-binary exclusivity untouched.
- Live relevance policy untouched; the detector correctly reported `live_required=true` for this change (helper/harness paths).
- Controller-lease semantics preserved: terminal releases only the matching turn lease; transfer/takeover/terminal-wins/disconnect behavior unchanged (existing 26 lease tests green).
- Real `NodeServer` + `LocalProcessRunner` + mTLS path unchanged; no mock, no retry loop, no global timeout change, no second CI lane.
- No sccache configuration touched.

## 9. Unresolved findings

| Severity | Finding | Disposition |
|---|---|---|
| None (C002 scope) | — | No residual C002 findings. C001's recorded local-verification items (Rust 1.89 Clippy, scheduler-cancellation) were explicitly out of C002 scope and are unchanged; they travel with C001. |

## 10. Roadmap and dependency disposition

C002 closes. **C001 is unblocked back to `ready`** and resumes per its plan: planning/docs reconciliation review, unrelated-PR fast-path measurement, and the bounded sccache A/B — all against the now-stable baseline qualified here. No other registered plan lists C002 as a dependency; the blocked-work audit finds nothing else newly unblocked (Eggwork M003, Architecture M009, Runtime C002, and advisor lines are independent).

## 11. Registry updates

`plans/registry.md`: subsystem row → C002 closed, C001 ready; dependency-ready C002 row → closed pointing at this record; gate paragraph → C001 ready to resume; recently-closed table gains the C002 row. The addendum marks C002 closed and C001 ready; the implementation plan carries closed status.
