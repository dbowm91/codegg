# Harness Lab Self-Improvement Resumption Milestone S031 — Reviewer Deadline Recovery and Attribution

Status: **conditionally closed for reviewer recovery; H007 qualification remains unqualified**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (dirty worktree; preserve all existing edits)

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#25-s031--reviewer-deadline-recovery-and-attribution`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Primary class: capability

## 1. Objective

Use attempt 37's durable reviewer failure evidence to make the next H007 run diagnose and recover a transient reviewer deadline on the same child run. Keep the workload, hidden verifier, exact-ID lifecycle, two repair cycles, active-duration requirement, and independent qualification criteria intact.

## 2. Why this milestone is ready

- S030's v91 fixture and prompt-boundary telemetry are implemented and focused checks passed.
- Attempt 37 completed 54.76 minutes of active work, both repair cycles, exact-ID overlap, the relocated workspace checks, and the required Clippy and workspace-test jobs. It remained unqualified because the reviewer child failed with `deadline_exceeded` after 1,205,086 ms and the agent did not invoke `task continue`.
- The durable reviewer projection matched exactly one Run and one Task. The 313 native tool starts all had terminal events; there were no event gaps or dropped events. This does not support an ACP watcher, scheduler, or job-ID mismatch diagnosis.
- The existing task tool permits one same-run continuation for transient deadline failures. No new runtime capability is required for this correction.
- A fresh user authorization grants up to 40 additional Muse starts on the existing `opencode_go` route. The OpenCode Go credential store has been switched; its value is not recorded in this plan or evidence.

## 3. Current implementation evidence

- v91 is `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v91/cases.jsonl`, SHA-256 `9e2052da9c1e45dbc5eebd136c5a290e852c1248c0f77ec87399624071c4524d`.
- Attempt 37 safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt37-safe-diagnostic-20261110.json`.
- The private run already recorded `reviewer_diagnostic.final_failure_class=deadline_exceeded` and a matching durable-state observation. S030's public projection had omitted that allowlisted class; the safe evidence now includes it without identifiers or failure text.
- The existing `task` tool contract allows one continuation of a failed durable child for transient failures, including `deadline_exceeded`, while preserving the same run authority. Authentication failures and cancellation are not continuable.

## 4. Invariants that must not regress

- Use only `opencode_go/muse-spark-1.3-contributor` for live qualification.
- Do not expose, print, hash, or record credential material. Do not retain raw prompts, model output, transcripts, job IDs, Task/Run IDs, hidden-verifier output, or failure messages in safe evidence.
- Preserve the same H007 task inputs, public tests, hidden verifier identity, native tool surface, request/time/disk limits, repair pairs, exact-ID jobs, and qualification thresholds.
- Spawn exactly one independent reviewer. A transient failure may be continued once using the same `run_id`; do not spawn a replacement or broaden authority.
- Fetch the reviewer Task exactly once only after observing its exact Run as `completed`.
- Do not claim qualification from duration, successful public gates, or reviewer attempt count alone; the unchanged independent verifier and every H007 gate must pass.
- Do not change daemon, scheduler, ACP projection, or multi-provider architecture without new evidence that identifies one of those surfaces.

## 5. Scope

### In scope

- Create v92 as a prompt-only derivative of v91 except for `group_id` and `title`.
- Replace stale run history with attempt 37's bounded public diagnosis.
- Narrow the reviewer audit to three highest-risk claims and explicitly teach the existing same-run continuation path for transient terminal failures.
- Extend safe post-run evidence with the reviewer failure class, elapsed duration, and same-run continuation count.
- Add focused fixture and diagnostic regressions, then attempt live qualification under the new authorization.

### Explicitly out of scope

- Production daemon, scheduler, ACP, job lifecycle, provider adapter, or authorization changes absent a new reproduced defect.
- Changing the hidden verifier, task contract, fixture source inputs, public tests, model/provider route, task duration threshold, repair requirements, or independent review requirement.
- Starting any run below the 30-GiB preflight reserve or exceeding any per-run cap.

## 6. Required changes

### Fixture and reviewer recovery behavior

Create v92 from v91. Preserve every manifest field except `group_id`, `title`, and `prompt`; retain the hidden verifier digest and every budget and route field. The prompt must:

1. Summarize attempt 37 using safe facts only: successful horizon/overlap/repair/workspace checks; reviewer `deadline_exceeded` after approximately 1,205 seconds; zero `task continue` actions; 313/313 paired native events, zero gaps/drops; 250/1,200 provider requests; and an unqualified verifier result. State that the next run uses the rotated OpenCode Go key without storing its value.
2. Keep one reviewer and limit the independent audit to three checks: the dependent implementation/public contract, the two repair-pair and exact-ID evidence, and consistency of final continuation/job/reviewer records. Ask for concise findings and one high-value edge case; prohibit exhaustive review or edits.
3. On terminal reviewer failure, inspect the exact public failure class. For one transient class (deadline, budget exhaustion, rate limit/transient upstream failure, or tool permission refusal), call `task continue` once on the same `run_id`, then observe the new attempt to terminal. Never continue authentication failure or cancellation, never spawn a replacement, and never fetch a Task result unless the exact Run is `completed`.
4. Keep the existing independent-review gate pending until one completed Run status and its one matching Task result have been observed and recorded.

### Safe evidence

Include only the allowlisted reviewer failure class, numeric child elapsed milliseconds, bounded continuation-action count, bounded durable-match counts, terminal status, and whether a completed Task result was observed. Keep raw failure text and identifiers absent. Preserve `billed_usd: unknown` when the provider does not report a USD amount.

## 7. Ordered work packages

### Work package A — Preserve attempt 37 evidence

Keep the safe diagnostic and authorization ledger reconciled: attempt 37 is completed unqualified, 250 provider requests, `deadline_exceeded` reviewer class, disk and event results, 38/160 combined starts consumed, and the new 40-start grant active. Do not rewrite prior run outcomes.

### Work package B — Prepare v92 and focused regressions

Create the v92 manifest and add a test proving all non-prompt fields match v91 and that the prompt contains the same-run recovery rules without stale attempt 35/36 narratives or any credential value. Update the reviewer lifecycle regression to cover `deadline_exceeded` and a `continue` action count. Run the focused Harness Lab suites, test-quality guards, Python compilation, and `git diff --check`.

### Work package C — Fresh live preflight and attempt 38

Before provider invocation, verify the exact route/model, credential-store entry availability without reading or printing its value, v92/verifier/binary/helper identities, unused private lane, request headroom, and at least 30 GiB free disk. Count the start before the first provider request. Keep the disk watcher active and stop at the 10-GiB runtime floor.

### Work package D — Attribute and continue the loop

Inspect only safe aggregates. If attempt 38 does not qualify, classify the exact failure domain. Continue the same-run child only for an allowed transient failure. Add a new narrow S032 corrective only if the evidence identifies an unclosed defect; do not restart the daemon/scheduler/ACP redesign. Stop when one attempt qualifies or the fresh 40-start grant is consumed.

## 8. Failure, cancellation, restart, and contention semantics

- A preflight refusal before provider invocation does not consume a live start.
- Once counted, cancellation, timeouts, provider errors, and incomplete qualification remain consumed historical attempts.
- On reviewer `deadline_exceeded`, preserve the original Run/Task relationship and use the existing single continuation bound. Do not substitute another reviewer.
- If the durable RunStore and tool projection disagree on terminal state or identity, record a harness lifecycle defect and stop qualification for that attempt.
- Stop the process group on user cancellation, 10-GiB free-disk floor, 300 seconds inactivity, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, 100 follow-ups, or identity/auth mismatch.

## 9. Compatibility and migration

No production schema, protocol, provider, or credential format changes. v92 is an additive fixture version; previous fixtures, receipts, and grants remain historical. The OpenCode Go credential stays in the existing protected store and is not copied into run receipts.

## 10. Required tests

### Focused unit and fixture tests

- `dev.harness_lab.tests.test_h007`: v92 inherits all non-prompt fields and pins the reviewer recovery instructions.
- `dev.harness_lab.tests.test_verifier_diagnostics`: `deadline_exceeded` stays an allowlisted reviewer class; a `continue` action is counted without retaining IDs or result text.
- `dev.harness_lab.tests.test_autonomy_run`: existing prompt-boundary and receipt-redaction coverage remains green.

### Guards and static checks

- `python3 scripts/check_test_assertion_quality.py`
- `python3 scripts/check_test_duplication.py`
- Python compilation and `git diff --check`.

### Live qualification

Attempt 38 must pass the unchanged hidden verifier, every original H007 task/gate, at least 30 minutes of active dependent Rust work, exact-ID overlapping background jobs, both repair cycles, independent completed review, continuation evidence, and resource/cancellation limits. One failed, timed-out, or incomplete run does not qualify.

## 11. Authorization and bounded resource controls

The new user grant authorizes at most 40 additional counted starts for this corrective loop, only via `opencode_go/muse-spark-1.3-contributor`. Per-run limits remain 1,200 requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall, 300 seconds inactivity, 100 follow-ups, 10-GiB runtime disk floor, and a 30-GiB preflight reserve. There is no USD ceiling; record USD as `unknown` if unavailable. Respect cancellation immediately. Do not run concurrently.

## 12. Closure criteria

Close S031 conditionally when its reviewer recovery and safe attribution objectives are verified, while keeping S019 unqualified if any independent task or verifier criterion fails. Attempt 38 confirms the reviewer correction; S032 owns the unqualified task/repair result. Reconcile its safe evidence, append-only ledger, S019 status, subsystem roadmap, and registry without claiming sustained qualification.

## 13. Offline verification result

The v92 fixture and reviewer recovery regression passed with the focused H007 and verifier-diagnostic suites (106 tests). The autonomy-run suite passed (64 tests). The test assertion-quality and duplication guards, Python compilation, and `git diff --check` passed. No Rust binary-affecting source changed; attempt 38 must still verify the paired ARM64 binary/helper hashes and all live preflight conditions.


## 14. Attempt-38 disposition

Attempt 38 completed the reviewer lifecycle successfully: one exact Run reached `completed`, the matching Task was fetched once after the terminal status, and the durable state matched. It required no `task continue`, so the intended transient-failure path was not exercised. S031's reviewer correction and safe reviewer-state attribution are therefore closed at their stated scope.

The overall H007 task did not qualify. The safe receipt records 54.53 minutes of active work, 253/1,200 provider requests, 19/19 terminal test jobs, 283/283 native event pairs, no event gaps/drops, and a 38.3-GiB minimum disk value. The independent verifier passed 3/5. Both named repair pairs had zero tool calls between the observed failed status and the repair-job submission; consequently 0/2 named pairs qualified. The final agent prompt timed out while preserving the 300-second verifier reserve. No reviewer, ACP-event, or job-terminal-loss defect was observed.

S032 owns the remaining task-execution failure. Closure evidence: `plans/closure/harness-lab-self-improvement-resumption/031-status.md` and `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt38-safe-diagnostic-20261110.json`.
