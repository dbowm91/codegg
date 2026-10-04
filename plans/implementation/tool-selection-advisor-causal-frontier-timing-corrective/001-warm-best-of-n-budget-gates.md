# Tool-Selection Advisor Causal Frontier Timing Corrective Milestone 001 — Warm Best-of-N Latency Gates

Status: implemented

Repository baseline: `cdfd6257` (fix head; baseline reproduced at `main` `614e983e`)

Source corrective addendum:

- `plans/subsystems/tool-selection-advisor-causal-frontier-timing-corrective-addendum.md`

Source plans this corrective addresses:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/005-active-disclosure-trajectory-qualification.md`

Source closure records this corrective addresses:

- `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`
  §10 — "Timing tests on this workstream are load-sensitive … classified as the
  documented load flake … no code or gate change."
- `plans/closure/desktop-frontend-ide-foundation/005-status.md` §10 — medium
  finding: the flake keeps every branch red and truncates the workspace sweep;
  remediation options were listed but deliberately not taken, because the area
  is a closed experiment and changing its gate is a corrective-pass decision.

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md`
  (closed; M005 closed with disposition B)

Long-term requirements:

- `architecture/agent-tool-surface.md` — causal advisor latency budget and
  observability
- `architecture/document.md` — untouched by this corrective

Applicable ADRs:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure

## 1. Objective

Make the three single-shot wall-clock latency assertions in the causal advisor
gates measure the causal advisor rather than the CI runner, without changing
the frozen 5.0 ms budget or deleting any assertion.

## 2. Why this milestone is ready

- The defect is fully characterized and reproduced on baseline `main`
  (`614e983e`): **4 failures in 8 runs**, with no M006-A code present.
- The fix is test-harness-only. There is no production behavior, contract,
  schema, storage, or authority change, so there is no hard dependency to
  close.
- No ADR is required: the correction restores agreement between a measurement
  and the invariant it is meant to check. It does not change ownership,
  boundaries, or what the budget protects.
- The one decision that was open — whether the constant or the measurement is
  wrong — is settled by the M005 freeze record, which pins the constant.
  The constant is therefore not available for change, which leaves measurement
  as the only remaining axis.

## 3. Current implementation evidence

Three sites assert a single cold sample against a frozen budget:

| Site | Assertion at baseline | Samples |
|---|---|---|
| `src/tool_advisor/causal_observe.rs` | `outcome.evaluation_millis < CAUSAL_OBSERVE_P95_BUDGET_MS` | 1 |
| `tests/causal_active_m005.rs` | `outcome.active_evaluation_millis < CAUSAL_ACTIVE_P95_BUDGET_MS` | 1 |
| `tests/causal_observe_replay.rs` | `outcome.evaluation_millis < CAUSAL_OBSERVE_P95_BUDGET_MS` | 1 |

Sound distributional gates already exist alongside two of them:

- `replay_p95_within_budget` (`tests/causal_observe_replay.rs`) — nearest-rank
  p95 over 201 warm samples against `CAUSAL_OBSERVE_P95_BUDGET_MS`.
- the holdout's own p95 (`tests/causal_active_m005.rs`) — nearest-rank p95
  across scenarios against `CAUSAL_ACTIVE_P95_BUDGET_MS`.

The constant is frozen and independently pinned:
`m005_freeze_record_matches_live_contracts` asserts
`freeze["structural_gates"]["pure_active_eval_p95_ms_max"] ==
CAUSAL_ACTIVE_P95_BUDGET_MS`.

Known gap: the three single-shot assertions are not additional coverage of
anything the p95 gates do not already check distributionally. They are
duplicates measured with a method that cannot separate algorithm cost from
runner load, which is why they are the ones that flake.

## 4. Invariants that must not regress

- `CAUSAL_OBSERVE_P95_BUDGET_MS` and `CAUSAL_ACTIVE_P95_BUDGET_MS` remain
  `5.0`.
- The M005 freeze record and its benchmark fingerprint remain byte-identical,
  and `m005_freeze_record_matches_live_contracts` remains green.
- `replay_p95_within_budget` and the holdout p95 remain the distributional
  enforcement of the same constant.
- An assertion exists at all three sites; none is deleted, skipped, or
  `#[ignore]`d.
- Each gate can still fail: an impossible budget must produce a failure with
  an explicit message.
- No production code path changes. The diff is confined to test modules and
  integration test files.

## 5. Scope

### In scope

- The measurement method of the three single-shot latency assertions.
- Test-module-local helpers providing warm best-of-N sampling.
- Diagnostic messages that name the measured value and the budget.
- Removal of a test import that becomes unused as a result.

### Explicitly out of scope

- The 5.0 ms constants, the M005 freeze record, and every frozen gate threshold.
- Production causal-advisor code, scoring, admissibility, promotion,
  disclosure, effect-path planning, and any production timing instrumentation.
- The M005 holdout scenario set, gold structured signal, and fingerprints.
- Moving the gates to a dedicated non-default CI job, adding a CI-command
  policy change, or a toolchain pin.
- `src/tool_advisor/mod.rs`'s loose `elapsed_millis < 1000` (1 s) assertion —
  a different defect class; widening this corrective to cover it was
  considered and rejected as unrelated scope.
- Any edit to a historical closure record, including the causal frontier
  `001`–`005` records and the M006-A `005-status.md`.

## 6. Required production changes

None. This milestone changes no production code.

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend or operator surface

None.

### Security and authorization

None.

### Documentation and static guards

- No new static guard is required: the defect is in test methodology, and the
  regression evidence is the repeated-run proof plus the deliberate
  breach-fires proof, both recorded in the closure record.
- Planning registration in `plans/registry.md` and this addendum.

## 7. Ordered work packages

### Work package A — Warm best-of-N helper in the observe unit test

Intent: remove the scheduler confound from the observe single-shot assertion
while keeping the assertion and the constant.

Required changes: in the `causal_observe` test module, add a `BUDGET_SAMPLES`
constant and a `warm_min_millis` helper that performs one discarded warmup and
returns the minimum of N warm samples, documented with why a single cold
sample is the wrong estimator. Route the assertion through it, keeping the
`black_box` so the measured call is not optimized away.

Acceptance evidence: the assertion still compares against
`CAUSAL_OBSERVE_P95_BUDGET_MS` and its failure message names both the measured
value and the budget.

### Work package B — Warm best-of-N helper in the M005 holdout

Intent: the same correction for the per-scenario single-shot check, which is
the assertion that failed in the M005 close run and again in the M006-A run.

Required changes: add the same helper next to the file's existing
`nearest_rank_percentile`, and route the per-scenario assertion through it
inside `qualify_scenario`, keeping the scenario id in the message.

Acceptance evidence: the file's own p95 assertion below is untouched and
remains the distributional check; arm 1's freeze pin is untouched.

### Work package C — Sanity check on the live-loop sample

Intent: the third site records a duration produced inside a live agent-loop
request preparation. It cannot be re-run with identical inputs, so best-of-N
is not available there.

Required changes: convert the budget assertion into a finite/non-negative
sanity check that the live path recorded a real duration, and point the
comment at `replay_p95_within_budget` as the enforcement point for the same
constant. Remove the now-unused constant import from that inner module.

Acceptance evidence: the live path is still required to produce a real
measurement, and the constant remains enforced over 201 warm samples in the
same file.

### Work package D — Prove the flake is gone and the gate still fires

Intent: a single green run proves nothing about a flake, and a passing
assertion proves nothing if it can no longer fail.

Required changes: none to source; this is the evidence-producing step.

Acceptance evidence: 20 consecutive clean runs of the two affected integration
targets on the fix head, against a baseline of 4 failures in 8; and a
deliberate-breach run with both constants temporarily set to an impossible
value in which all four affected gates fail with explicit messages, followed
by restoration and a passing re-run.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable in the production sense: no production path changes, no state,
no process, no authority.

For the measurement itself:

- A preemption that lands on every one of the 15 samples is still detected —
  that is the behavior the fix accepts, because 15 consecutive preemptions is
  a different signal from a single cold sample. This is recorded as a
  deliberate trade-off, not an oversight.
- Test failures must name the measured value and the budget so a future
  failure is diagnosable without re-running.
- The helpers are pure functions with no shared state, so they are safe under
  the workspace's one-process-per-test `nextest` profile and under repeated
  invocation.

## 9. Compatibility and migration

None. No schema, storage, protocol, configuration, or public API change. The
constants remain `pub` and unchanged, so downstream consumers see no
difference. MSRV 1.89 is preserved; `std::hint::black_box` is stable well
below it.

## 10. Required tests

### Focused unit tests

- The corrected `observe_never_mutates_or_suppresses` in the `causal_observe`
  unit test module.
- The full `cargo test --lib tool_advisor` suite, to confirm no other advisor
  test depended on the changed helper's absence or on the removed import.

### Integration tests

- `causal_active_m005` — all four tests, including the freeze-record pin
  (`m005_freeze_record_matches_live_contracts`) and the holdout structural
  gates.
- `causal_observe_replay` — all thirteen tests, including
  `replay_p95_within_budget`.

### Restart and recovery tests

Not applicable.

### Contention and cancellation tests

The repeated-run proof serves this role: 20 consecutive runs of both
integration targets, sequentially, to demonstrate the gate tolerates ordinary
run-to-run variation.

### Security and negative tests

The deliberate-breach proof is the negative test: with an impossible budget,
every gate must fail. This is the check that the change did not silently
weaken the gate into something unfailable.

### Migration and compatibility tests

The freeze-record pin test (`m005_freeze_record_matches_live_contracts`) is the
compatibility check: it fails if the constant is altered, which is exactly the
prohibition the milestone is working under.

## 11. Required verification commands

```bash
# the two affected integration targets, repeated to test the flake itself
cargo test --test causal_active_m005 --test causal_observe_replay
# (run 20+ times; baseline at main 614e983e fails ~4 in 8)

# the advisor unit suite
cargo test --lib tool_advisor

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
cargo nextest run --workspace --locked --profile ci
```

Hosted `CI / verify` on the fix head supplies the full-sweep evidence, because
`nextest` fail-fast previously left roughly 6,900 of 12,180 tests unrun on
every red attempt.

## 12. Documentation updates

- `plans/subsystems/tool-selection-advisor-causal-frontier-timing-corrective-addendum.md`
  (new; the corrective record).
- `plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/001-status.md`
  (new; the closure record).
- `plans/registry.md`: a roadmap row, an implementation-plan row, a closure
  row, and a reconciliation of the causal-frontier gate paragraph.
- The causal frontier M006-A closure's medium finding is discharged by this
  milestone; that record is not edited.
- No `architecture/` document changes: no production contract, ownership, or
  behavior changed, and the budget semantics they describe are unchanged.

## 13. Acceptance criteria

- The flake is gone by repetition: 20 consecutive clean runs of
  `--test causal_active_m005 --test causal_observe_replay` on the fix head,
  against a measured baseline of 4 failures in 8 on `main` `614e983e`.
- `CAUSAL_OBSERVE_P95_BUDGET_MS` and `CAUSAL_ACTIVE_P95_BUDGET_MS` are still
  `5.0` in source, and the freeze-record pin test is green.
- With both constants temporarily set to an impossible value, all four
  affected gates fail with explicit measured-vs-budget messages; after
  restoration they pass.
- `replay_p95_within_budget` and the holdout p95 are unmodified and green.
- `git diff` for the change touches only
  `src/tool_advisor/causal_observe.rs`, `tests/causal_active_m005.rs`, and
  `tests/causal_observe_replay.rs`.
- `cargo fmt --all -- --check` and workspace clippy under `-D warnings` are
  clean.
- Hosted `CI / verify` is green on the fix head with the full workspace sweep
  executed rather than truncated by fail-fast.

## 14. Stop conditions

Stop and report rather than improvise when:

- the fix appears to require changing a frozen constant or the M005 freeze
  record — that would contradict the milestone's own invariants;
- removing or skipping an assertion appears to be the only way to make a
  gate green;
- the baseline reproduction stops failing at `main` `614e983e`, which would
  mean the root cause is not what this milestone claims;
- a gate cannot be made to fail by a deliberate breach, which would mean the
  change weakened the gate rather than fixing its measurement;
- the work would expand into the M005 holdout content, causal-advisor
  production behavior, CI-command policy, or a toolchain pin.

## 15. Closure evidence required

- The exact 20-run (or better) repetition result and the 8-run baseline result,
  with the baseline worktree SHA.
- The deliberate-breach transcript: which gates failed, with their messages,
  and the confirmation that the constants were restored.
- The final values of both constants read from source.
- `cargo fmt --check`, workspace clippy, and the focused test results.
- The hosted `CI / verify` run id and per-step outcome, explicitly stating
  whether the workspace sweep ran to completion.
- The `git diff --stat` proving the change is confined to the three files.
- An explicit statement that no historical closure record was edited.

## 16. Handoff notes

- Do not raise the budget. It is frozen by the M005 freeze record, which is
  itself an assertion in the test suite; changing it breaks a passing test.
- Do not delete the three assertions. The whole point is that the fix is a
  measurement fix, not a removal.
- `tests/causal_observe_replay.rs` needs its inner-module import list updated;
  an unused import is a `-D warnings` failure.
- The 20-run proof is the expensive part of this milestone. Budget for it
  rather than substituting a single green run, which is the mistake that let
  the original classification stand unfixed across three hosted runs.
- Do not touch `src/tool_advisor/mod.rs`'s 1 s assertion; it is a separate
  defect class and out of scope by decision.
