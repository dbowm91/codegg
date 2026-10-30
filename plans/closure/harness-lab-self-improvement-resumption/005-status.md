# Harness Lab Self-Improvement S005 — Closure

Status: **conditionally closed — deterministic offline campaign sequencing and component fault paths requalified; `stable_offline` not established**.

S005's offline-only evaluation reused the current-main optimizer, checkpoint, ledger, scoring, provider-boundary and native-supervisor tests. It did not add a second optimizer or alter CodeGG daemon/scheduler policy. Added `OptimizerCampaignSequenceTests.test_two_candidates_reject_noop_then_accept_scored_correction_repeatably`: the real optimizer writes hash-linked checkpoint events and private Git worktrees for two sequential synthetic candidates; the production score contract rejects a no-op and accepts only the second correctness result; a second run yields identical patch digests and normalized decisions. Both synthetic actors used zero provider requests. The exact-main quick run including this test passed: 430 Harness Lab tests passed, one optional test skipped (431 total), every quick guard passed, and `cargo check --workspace --all-targets --locked` passed. The S005 matrix is [`evidence/s005-offline-fault-matrix.json`](evidence/s005-offline-fault-matrix.json); exact source and binary identity are in [`evidence/s002-offline-qualification.json`](evidence/s002-offline-qualification.json).

## Fault coverage inventory

| Fault or invariant | Existing executable evidence | Classification |
|---|---|---|
| Pre-admission request exhaustion and N+1 transport refusal | `test_budget.py`; provider boundary request-cap test in S002 evidence | Proved for CodeGG subject requests. |
| External proposer request accounting | `test_improve.py` fake CLI/invocation budget tests | Invocation budget is tested; internal HTTP N+1 is not pretransport-enforced. Hard live blocker retained. |
| Checkpoint interruption and pointer reconciliation | `test_improve.py::test_sigkill_between_checkpoint_append_and_pointer_is_recorded`; resume validation tests | Proved at this injected local boundary. |
| Ledger interruption and exactly-once rows | `test_improve.py::test_sigkill_during_ledger_write_leaves_no_duplicate_trial_rows`; `test_resumed_run_does_not_append_a_second_row_for_a_recorded_pair` | Proved for covered ledger faults. |
| Stale source/binary/suite/model identity | `test_improve.py`, `test_ledger.py` | Mismatch refusal covered. |
| Candidate outcomes and score falsifiers | `test_improve.py`, `test_scoring.py`, `test_autonomy_run.py` | Accepted/rejected/incomplete/protected-control paths covered with deterministic fixtures. |
| Two-candidate controller sequence and deterministic replay | `dev.harness_lab.tests.test_improve.OptimizerCampaignSequenceTests.test_two_candidates_reject_noop_then_accept_scored_correction_repeatably` | Actual optimizer, checkpoint, Git worktree and production scorer run twice; synthetic no-op rejected, correctness fix accepted, patch digests and normalized decisions match. No build/ACP or model claim. |
| ACP timeout, cancellation, missing job fields and denial | `test_supervisor.py`, `test_sustained.py`, `test_cli.py`; S002 native Deny trial | Bounded timeout/cancellation and unknown metadata paths tested; explicit Deny trial passed. |
| Isolated subject shutdown | S002 two-subject smoke receipt | One subject stopped while the other created a session; zero provider calls. |

The synthetic optimizer sequence does not execute candidate build gates or native ACP qualification; those have separate component/native evidence. The matrix still does not cover daemon-crash orphan cleanup, sustained Cargo/CPU-target contention, or a SIGKILL at every post-admission/proposer/build/verifier/publication/cleanup transition. No candidate was launched because S004 found no eligible current defect. `stable_offline` is **not claimed**.

## Readiness and remaining blocker

Subject-side request admission is pretransport-enforced. External OpenCode proposer HTTP requests are not; the fake CLI test cannot prove otherwise. No provider calls occurred, USD is `unknown`, and the live proposer remains blocked. S006 may document its conditional/not-established operator outcome, but may not label this result stable offline or live evidenced. A successor should add only the uncovered resource/daemon-crash fault cases and prove proposer admission if that external route is retained; broader daemon or ACP redesign is unsupported by these results.
