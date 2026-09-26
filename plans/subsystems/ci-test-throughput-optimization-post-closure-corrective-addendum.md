# CI and Test Throughput Optimization — Post-Closure Evidence Corrective Addendum

Status: active; C002 ready; C001 blocked on C002

Repository baseline reviewed: `4f7e508976e70fbec0e645bd530b63fe3c8393c1`

Predecessor work:

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` — historical M001-M005 workstream;
- `plans/closure/ci-test-throughput-optimization/001-status.md` through `005-status.md` — accepted historical closure evidence;
- final predecessor hosted run `36196068239` attempt 3 — green main push, 17m17s total, 7m51s test build, 362.537s execution, 11,726 passed / 1 skipped, live Eggwork qualification included.

Canonical references:

- `plans/003-planning-process.md` §7, corrective passes;
- `architecture/testing.md` — active verification/CI contract;
- `.github/workflows/ci.yml`;
- `.config/nextest.toml`;
- `scripts/verify.sh`.

Primary class: polish / development infrastructure.

## 1. Why this corrective exists

M001-M005 materially improved routine hosted CI and remain valid historical implementation work. Post-closure review found four evidence/documentation defects that prevent the workstream from being treated as cleanly closed.

### Finding A — planning state drift

The predecessor roadmap still reports `Status: active` even though M001-M005 are closed. It also contains stale duplicate M004/M005 milestone blocks that still say `blocked/conditional` and `blocked` after later closed copies of those milestones.

The registry currently reports the original workstream as `closing`, which is more accurate than the roadmap but still leaves two sources of planning truth in conflict.

### Finding B — active testing documentation contains stale pre-final values

The primary `architecture/testing.md` CI section still describes the post-tuning state as approximately 19 minutes with `JOBS=8` and 11,781 tests even though the final M001-M005 state is:

- `CARGO_BUILD_JOBS=4`;
- 17m17s on the final main/live hosted run;
- 11,726 passed / 1 skipped;
- explicit live-Eggwork relevance detection and conditional fixture prebuild before workspace tests.

A later final-state subsection is correct, but the earlier operational section remains contradictory.

### Finding C — M005 cache disposition was closed without the planned compiler-cache experiment

M005 rejected `sccache` before a hosted probe on the premise that `Swatinem/rust-cache` already provided an equivalent compiler-result cache and that `sccache` would require an external secret.

Current upstream documentation reviewed for this corrective does not support those premises:

- `Swatinem/rust-cache` documents workspace-crate caching as opt-in through `cache-workspace-crates` and states that workspace crates are not cached by default;
- `sccache` documents a GitHub Actions cache backend enabled with `SCCACHE_GHA_ENABLED=on` using the GitHub Actions runtime/cache credentials.

This does not establish that `sccache` will improve CodeGG. It establishes that the M005 negative disposition lacks the bounded hosted A/B evidence required by its own implementation plan.

The M005 closure record remains immutable historical evidence. C001 owns the corrected cache disposition.

### Finding D — the unrelated-PR fast path lacks a direct hosted baseline

M002 intentionally omits the expensive live Eggwork target for unrelated pull requests while preserving relevant-PR and main/Linux qualification. The final 17m17s predecessor baseline is a main push where live qualification was included.

Therefore the ordinary unrelated-PR feedback time may already be materially lower, potentially near the original <15-minute aspirational target, but the workstream never captured a clean authoritative measurement.


### Finding E — C001 implementation exposed hosted timing/correctness instability

C001 began with a live-relevance selector correction, merged as `9c88b9b8` (PR #82). The fix changes PR comparison from a shallow-history-sensitive three-dot diff to direct fetched-base-tree vs synthetic merge-tree comparison. The PR run `36213918571` was green, but because the detector script itself is live-relevant it correctly exercised the live path and is not the required unrelated-PR baseline.

The subsequent main run `36215541015` failed twice:

- attempt 1: `session_family::control_m004_controller_lease::reaper_releases_on_turn_completed_event`;
- attempt 2: `eggwork_remote_execution_live::live_blob_upload_and_workspace_materialization` timed out waiting for helper readiness.

Inspection of `CoreDaemon::spawn_turn_reaper` found a concrete lost-event race: the event-log receiver is created inside the spawned task, so an immediately following terminal publication can occur before subscription and be lost permanently. This is production correctness scope and must be fixed before performance measurements resume.

The Eggwork timeout is a previously observed hosted flake, but its current timeout path drops the captured helper diagnostics. C002 owns deterministic reaper readiness, Eggwork startup diagnostics/characterization, and hosted stability qualification. C001 is blocked until C002 closes.

## 2. Corrective milestone

### C001 — Closure evidence, compiler-cache qualification, and documentation reconciliation

Status: blocked on C002.

Implementation plan:

- `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/001-closure-evidence-cache-qualification-and-doc-reconciliation.md`


### C002 — Hosted CI timing-flake stabilization

Status: ready.

Implementation plan:

- `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/002-hosted-ci-timing-flake-stabilization.md`

Objective:

Repair the turn-reaper lost-event race, make live Eggwork helper-startup failures phase-diagnostic, characterize the readiness stall without blind timeout inflation, exonerate or correct the consolidated session-family boundary, and restore a stable green full hosted baseline before C001 resumes cache/timing work.

Dependency:

- C002 has no new hard dependency beyond the already-closed M001-M005 work.
- C001 is hard-blocked on C002 because cache/timing A/B evidence is not trustworthy while main/live CI is unstable.

Hard dependencies:

- CI/Test Throughput M001-M005 closed — satisfied.
- No product/runtime dependency.

Objective:

Repair the planning and active-document inconsistencies, execute the previously omitted bounded compiler-cache experiment with comparable hosted evidence, measure the unrelated-PR fast path, and produce one authoritative final disposition without reopening the already-closed test-topology and live-qualification design.

## 3. Historical closure treatment

C001 MUST NOT rewrite `plans/closure/ci-test-throughput-optimization/001-status.md` through `005-status.md` to make their historical reasoning appear different.

Where C001 supersedes a predecessor conclusion, the new closure record must say so explicitly. In particular:

- M001-M004 implementation conclusions remain accepted;
- M005's same-job-overlap rejection may remain accepted unless new evidence contradicts it;
- M005's unmeasured `sccache` rejection is historical evidence and is superseded by the C001 measured disposition.

## 4. Invariants

C001 must preserve:

- one ordinary `CI / verify` job;
- `contents: read` and no release authority;
- `CARGO_BUILD_JOBS=4` unless new evidence explicitly demonstrates that the cache experiment requires a different controlled setting; changing build jobs is otherwise out of scope;
- Nextest `ci` at 4 slots and the M002 heavy-binary exclusivity policy;
- live Eggwork change detection, relevant-PR qualification, and unconditional main qualification;
- the M003/M004 consolidated `projection_replay` and `session_family` targets;
- all tests, assertions, feature gates, and platform/resource boundaries;
- local `scripts/verify.sh quick/full` semantics;
- no new permanent performance gate, benchmark lane, matrix, or external cache service.

## 5. Non-goals

C001 does not authorize:

- another integration-test consolidation campaign;
- changing the heavy-test run-alone policy;
- removing live Eggwork qualification from main;
- splitting Clippy/tests into separate permanent jobs;
- remote/self-hosted runners;
- broad dependency/profile refactoring;
- changing production Rust behavior;
- editing historical closure records to conceal the original M005 reasoning.

## 6. Completion definition

C001 closes when:

- predecessor roadmap/registry state is internally consistent and historical M001-M005 closure is preserved;
- stale operational values in `architecture/testing.md` are reconciled with the actual workflow;
- one comparable unrelated-PR hosted run establishes the ordinary fast-path wall time;
- a bounded hosted `sccache` A/B is actually executed, including cache statistics and cold/seeded/warm interpretation;
- the final tree either retains `sccache` because it clears the preregistered materiality threshold without correctness/complexity regression, or fully reverts it with a measured negative disposition;
- the final main/live hosted path is green;
- C001 closure explicitly supersedes only the unsupported M005 cache conclusion, not M001-M004.

## 7. Status table

| Corrective | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C002 Hosted CI timing-flake stabilization | ready | `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/002-hosted-ci-timing-flake-stabilization.md` | pending | — |
| C001 Closure evidence, cache qualification, and doc reconciliation | blocked | `plans/implementation/ci-test-throughput-optimization-post-closure-corrective/001-closure-evidence-cache-qualification-and-doc-reconciliation.md` | pending | C002 hosted CI stability closure |
