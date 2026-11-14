# S021 — Repair-pair correlation and v83 admission sequencing

Status: **active**

## 1. Objective

Correct the measured mismatch from S019 attempt 27 and make the named seed-job observation rule unambiguous in the next frozen public prompt. Requalify the existing H007 workload only after focused offline verification. Preserve the independent verifier and its separation from the agent.

## 2. Evidence and attribution

Attempt 27 used v82 on `opencode_go/muse-spark-1.3-contributor`. It ran 54.9 active minutes and ended unqualified at the 3,300-second prompt-work cap. The safe receipt records 310/1,200 provider requests, 361/1,600 native calls, complete causal observation (815 events, no gaps or drops, all 361 calls terminal), a 124,734-ms maximum activity gap, no stall or disk-floor cancellation, and a completed exact reviewer lifecycle. The independent verifier ran five tests, passed two, and failed three; assertion text and hidden verifier source/output remain excluded.

Both named seed jobs reached terminal `failed`, both repair jobs reached terminal `completed`, and the public note documented two cycles. The format pair failed `failed_job_status_called_immediately`; the store pair passed all named-pair predicates. The safe projection nevertheless reported one qualified named pair but zero observed/correlated repair cycles. Thus the task did not qualify. There is no evidence of a provider, daemon, scheduler, ACP, event-capture, disk, or activity-stall failure.

## 3. Scope

1. Fix and regression-test `_observed_repair_cycle_pairs` so a repeated terminal status observation cannot move the failure point past an already observed productive edit. Keep exact job-ID matching, unique submissions, terminal states, event ordering, and one-to-one pairing intact.
2. Add a safe per-pair correlation result that reconciles the named-pair predicates with persisted repair-cycle events. Persist booleans/counts only; never retain IDs, arguments, transcript, or note text.
3. Create v83 as a prompt-only derivative of v82 apart from group/title metadata. For both named seed jobs, say explicitly that the immediately following native call must be `job status` with the exact returned ID and that no other tool call may intervene. Preserve the terminal-failed → direct-edit → distinct repair submission → terminal-completed requirements and all task/verifier/budget identity.
4. Keep the strict hidden-verifier boundary. Use public task requirements, public tests, safe receipt diagnostics, and the independent verifier’s test IDs/counts; do not inspect its source, assertion messages, traceback, or raw output.

Do not widen the provider cap, change the task architecture, alter Rust production code, weaken acceptance criteria, or redesign the scheduler/daemon/ACP path. No live provider attempt is part of offline implementation work.

## 4. Required offline checks

- Add a deterministic regression with a seed submission, exact terminal-failed observation, direct edit, a repeated failed-status observation after the edit, and a distinct completed repair. Require both the pair validator and event correlator to count the same qualifying pair.
- Add v83 manifest equivalence and next-native-call wording assertions for format and store; require the named-pair projection markers to remain enabled.
- Run focused `test_h007`, `test_autonomy_run`, `test_causal_trace`, `test_h007_attribution`, and `test_verifier_diagnostics` modules.
- Run `scripts/check_test_assertion_quality.py`, `scripts/check_test_duplication.py`, Python compilation, and `git diff --check`.
- Reuse the existing paired ARM64 binaries; this plan must not modify any Rust source or binary-affecting input.

## 5. Attempt 28 preflight and live controls

The existing S019 authorization covers the same sequential Muse Spark route and caps. Attempt 27 consumed one start: 28/120 combined starts are consumed, 92 remain; the current 40-start supplemental grant has 5/40 consumed, 35 remain. Do not consume another start until the offline checks pass.

Before attempt 28, verify the `opencode_go` auth entry without exposing its value, paired binary and helper hashes/architecture, v83 and verifier hashes, harness doctor, budget admission, absent lane/receipt, and at least 30 GiB free disk. Make no request during preflight. Count the start on invocation.

Per-run limits stay at 1,200 provider requests, 1,600 native calls, 3,600 seconds wall time, 3,300 seconds prompt work, 300 seconds inactivity, 10 GiB hard free-disk floor, and 100 same-session follow-ups. Keep monitoring provider requests, causal event gaps/drops, job lifecycle, disk, reviewer lifecycle, continuation, and verifier outcome. There is no USD ceiling; report provider-reported usage or `unknown` when unreported.

## 6. Qualification and stop conditions

One run must independently pass the hidden verifier and every current H007 criterion, including at least 30 minutes of substantive active work, exact-ID running/terminal overlap with productive work, both named repair pairs in the required order, valid continuation, reviewer lifecycle, complete event observation, and resource caps. `end_turn`, complete-looking public notes, terminal test jobs, or duration alone do not qualify a run.

Stop on qualification, explicit cancellation, a hard cap, or exhaustion of the recorded 120-start combined authorization. For another failure, preserve the safe receipt and test identifiers, distinguish agent/task failure from harness or observation failure, and choose only the next corrective supported by evidence. Never change the hidden verifier or report a verifier pass from an incomplete or timed-out task.

## 7. Closure evidence

- v83 manifest SHA-256 and prompt length, with preserved v82 hash.
- Focused test and guard results plus paired binary/verifier identities.
- Attempt 28 no-request preflight, live checkpoints, final safe diagnostic, receipt digest, current authorization reconciliation, provider request/cost, disk/activity data, per-pair sequence results, reviewer lifecycle, task terminal state, and verifier counts/test IDs.
- If attempt 28 does not qualify, record the exact unmet acceptance conditions and keep S019/S021 active.


## Offline implementation result

Implemented in `dev/harness_lab/h007_results.py`: preserve the earliest terminal job observation per exact ID and include a safe per-pair `repair_cycle_correlated` predicate. Added a regression for a repeated terminal-failed observation after the direct edit and before the repair submission. Created v83 with the exact seed status as the next-native-call instruction.

The v83 case manifest hash is `8e36f5c03299e4089d950b0564a019deb99890c2cdccd3d6f352f4861b5c9d6a` (9,988 prompt characters); all fields except group/title/prompt match v82. Verification passed across 182 focused tests, both Python quality guards, compilation, and diff check. Rust sources and binary-affecting inputs are unchanged; reuse the verified paired ARM64 binaries. Attempt 28 passed the planned fresh no-request preflight before it started.

Attempt 28 used v83 after the no-request preflight. It completed normally after 2,152,015 ms active work with both named repair pairs, exact-ID productive overlap, all public continuation gates, a completed reviewer, complete event/job observation, and healthy resource bounds. The independent verifier ran five tests and passed three; two evidence-related checks failed although the public continuation and transcript/job-lifecycle projections reported complete. S021 remains unqualified pending the narrow reconciliation owned by S022. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt28-safe-diagnostic-20261110.json`; receipt SHA-256 `588dc5204e6264fb79bf10bf06cff8c125971d7fc41192af46774ece16bb2295`. The ledger records 29/120 combined starts and 6/40 starts in the current supplemental grant.
