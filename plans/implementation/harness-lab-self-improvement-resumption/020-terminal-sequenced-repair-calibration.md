# S020 — Terminal-sequenced seeded repair calibration

Status: active; attempt 26 unqualified; v82 marker-preserving corrective active; attempt 27 pending
Repository baseline: ac584c886f03847ff6b829f8509667637e42a052 (worktree already contains the S019 diagnostics and v80 corrective)

Source roadmap: plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#14-s020
Long-term requirements: plans/000-long-term-specification.md; plans/002-long-term-roadmap.md
Applicable ADRs: None. This is a task-prompt calibration; it changes no protocol, authority, provider, scheduler, or daemon contract.
Primary class: capability

## 1. Objective

Correct the exact named repair sequence in the H007 live task so an agent observes a seed test job’s terminal failure before editing the relevant source and submitting its repair job. Requalify the unchanged workload and independent verifier with Muse Spark 1.3 Contributor under the existing S019 authorization and caps.

## 2. Why this milestone is ready

S019 attempt 25 provides a bounded, production-shaped failure diagnosis. The run lasted 55.0 minutes, exceeded the 30-minute active-work minimum, and completed all 404 native call pairs, all 25 job lifecycles, seven public stages, twelve gates, reviewer observation, release overlap, and public artifacts. Its causal trace was complete with zero event gaps; the inactivity guard did not fire; the run used 359 of 1,200 provider requests; and disk remained above the hard floor.

The independent verifier failed and the task timed out at the 3,300-second prompt-work limit. Both required named repair groups were unqualified: the repair submission preceded observation of the corresponding seed job’s terminal failed status, and no direct source mutation occurred between that observed failure and the repair submission. This is an agent/task sequencing failure, not evidence of a provider, daemon, scheduler, ACP, event-observation, or disk failure. The typed receipt has enough information to select a narrow corrective without reading transcript or verifier output.

## 3. Current implementation evidence

- S019 attempt 25 used manifest v80, hash fce6c3396c29e647f3904381c84fc4bbaaa728dc6c644f1fcc8f0df164ef5be9, and the already-paired ARM64 CodeGG/helper binaries.
- Its safe summary records six generic repair observations, but zero of the two named format/store pairs qualified.
- For each named pair, the failed job was submitted once, received an immediate exact-ID status call, and later had a terminal failure observation. The repair job was submitted once, received an immediate exact-ID status call, and later completed. However, the repair job was submitted before the failed terminal observation, so the required direct edit-after-failure ordering was absent.
- The continuation projection records all public stages, gates, artifacts, required jobs, review, and overlap complete, but document_valid is false because neither named repair pair qualified. The task-completion projection records agent_incomplete and continuation_not_confirmed. No raw IDs, arguments, note text, transcript, or verifier output are needed for this conclusion.

## 4. Invariants that must not regress

- Keep the S019 hidden verifier and public Rust workload unchanged.
- Keep named repair cycles one-to-one and bound to distinct exact job IDs.
- Count a seed failure only after an exact-ID status observation returns terminal failed. A submission response or running status is not failure evidence.
- Require a direct edit or patch after the terminal failure and before the one repair submission.
- Require the repair job’s exact-ID terminal completed status; never infer success from submission or an intermediate running state.
- Keep private diagnostics outside the agent workspace and exclude raw IDs, arguments, continuation note text, transcript, provider content, and verifier output.
- Use only opencode_go/muse-spark-1.3-contributor. Preserve request, native-call, wall-clock, prompt-work, disk, inactivity, and follow-up caps.

## 5. Scope

In scope:

- Add v82 as a prompt-only derivative of the failed v81 attempt, preserving all four literal named-job markers that activate the strict pair projection and retaining explicit wait-for-terminal instructions.
- Add a focused manifest regression proving v82 changes only versioned group/title metadata and prompt, retains every route/tool/workload/verifier/budget field, contains all four exact repair markers, and enables the strict repair-pair projection.
- Run focused offline checks, update S019 safe evidence, and execute sequential live attempts under the already-recorded authorization until qualification or exhaustion.

Out of scope:

- Changing the hidden verifier, Rust fixture, production daemon, scheduler, ACP transport, provider adapter, event protocol, request limits, or qualification criteria.
- Treating the six generic repair observations as substitutes for the two named exact-ID cycles.
- Reading or storing raw transcript, hidden verifier output/source, continuation note contents, job IDs, task arguments, or credentials.
- Adding idle delays or artificial work to force duration.

## 6. Required production and fixture changes

This plan changes only the versioned public task prompt and its manifest regression. Keep all S019 runtime and verifier behavior fixed.

The v82 prompt must state for both catalog-format and catalog-store, separately, and retain the literal field names used by the runner to enable strict validation:

1. Submit exactly one named seed test job and immediately issue job status for its returned ID.
2. If the exact job is running, continue observing that same ID with status or wait until its terminal state. Do not edit the target source, submit a repair/rerun, or claim a failure before a terminal failed status is observed.
3. After terminal failed is observed, make one direct source edit that addresses the observed failure, then submit exactly one corresponding repair test job.
4. Immediately observe the repair job by its exact returned ID; if running, continue observing that same ID until terminal completed.
5. Record each distinct ID and its observed terminal state in the public continuation artifact. Never manufacture a failed state or reuse a different successful job as the repair.
6. Once all required phases, gates, job evidence, review, and the minimum active-work span are complete, write the final audit and end the session without optional rework.

## 7. Ordered work packages

### A. Preserve and record attempt 25

Write a safe receipt projection containing only bounded counts, booleans, enums, hashes, and resource summaries. Reconcile the S019 global and supplemental authorization counts without charging a start twice. Do not copy private run output into the repository.

### B. Build v82

Copy v81 to a new versioned fixture. Change only group/title metadata and prompt text: retain the clarified terminal sequence and add the four literal job_id/job_state labels for the format/store seed and repair jobs. Keep the same fixture, verifier, tools, route, model, budgets, and abort policy. Stay below the existing prompt-size ceiling.

### C. Verify the prompt derivative

Add a regression that compares v82 with v81 after excluding only group/title/prompt. Assert all four repair markers are present and _h007_case_requires_repair_job_pairs returns true; also assert the exact per-pair terminal-failure, direct-edit, repair-submit, terminal-success, and total repair-count instructions. Run focused H007 tests, both test-quality guards, Python compilation, and git diff --check. No Rust rebuild is required.

### D. Requalify sequentially

Before each start, verify the route/auth metadata without displaying credential bytes, paired binary hashes, manifest and verifier identity, zero preexisting lane/receipt, admission policy, and at least 30 GiB free disk. Run only the existing opencode_go adapter and Muse Spark 1.3 Contributor route.

Each attempt keeps the S019 limits: 1,200 provider requests, 1,600 native calls, 3,600 seconds wall time, 3,300 seconds prompt work, 10 GiB free-disk hard floor, 300-second inactivity cancellation, and at most 100 same-session follow-ups. No USD ceiling is added; report provider-reported USD when available and otherwise record it as unknown. Preserve every failed receipt and safe diagnostic.

## 8. Failure, cancellation, and restart semantics

An attempt is not qualified if either named seed/repair pair lacks terminal-failure-before-edit-before-rerun order, if the agent ends incomplete, or if the independent verifier does not pass. Distinguish these task/agent outcomes from provider, daemon, scheduler, ACP/event, disk, and cancellation failures using the existing bounded diagnostics. Stop the same run on the recorded hard caps or explicit cancellation; never restart a terminated attempt in place. Do not begin another attempt until its no-request preflight and 30-GiB reserve pass.

If v82 still fails, use its now-enabled per-pair diagnostic to distinguish model sequencing from the remaining specific gap; refine only the task prompt or public task shape as the evidence warrants. If event or process diagnostics identify a reproducible CodeGG failure, write a separate narrow corrective before changing that subsystem.

## 9. Compatibility

v82 is a new benchmark manifest version. Historical manifests, receipts, authorizations, and verifier inputs remain immutable. The S019 ledger remains the source of truth for consumed starts and caps. No Rust binary-affecting input changes.

## 10. Required tests

- Manifest equivalence and repair-marker activation regression for v81 to v82.
- Focused H007 continuation, exact repair, and attribution tests.
- Python test assertion-quality and duplication guards.
- Python syntax/compilation checks for changed harness files.

## 11. Required verification commands

- Focused unittest discovery for the H007 and autonomy-run test modules.
- python3 scripts/check_test_assertion_quality.py
- python3 scripts/check_test_duplication.py
- python3 -m compileall -q dev/harness_lab
- git diff --check
- Harness doctor and budget-admission preflight for every live start.

## 12. Documentation updates

Update the S019 implementation and closure records with attempts 25 and 26 safe outcomes; add S020 to the subsystem roadmap and registry; record v82 identity, focused verification, each start, and each safe terminal result in the S019 evidence ledger.

## 13. Acceptance criteria

One single Muse run must have at least 30 minutes of substantive active Rust work and finish before the configured prompt-work deadline. In that same run:

- Both named seed jobs have exact-ID terminal failed observations before their corresponding direct source edits and repair submissions.
- Both repair jobs use distinct exact IDs and reach terminal completed states after the edits.
- The independent verifier passes and task completion is explicitly complete.
- Continuation, reviewer, required job lifecycle, exact-ID productive overlap, complete causal observation, activity-gap, disk, request, and native-call gates all pass.

Any run that meets duration but fails any task or verifier requirement remains unqualified.

## 14. Stop conditions

- Stop live attempts on qualification, explicit user cancellation, exhaustion of the recorded combined authorization, or any hard run cap.
- Do not exceed the current authorization ledger or start below the 30-GiB preflight reserve.
- Do not widen into daemon/scheduler/ACP architecture without a reproduced failure in the relevant telemetry.
- Do not change the verifier, weaken an acceptance criterion, or present a timed-out/incomplete agent as a task pass.

## 15. Closure evidence required

- Safe attempt 25 summary and receipt digest.
- v82 manifest hash and marker-activation/equivalence regression result.
- Focused test and guard results.
- Per-attempt preflight and safe terminal summaries, including route, binary and verifier identity; task/horizon outcome; exact event/job/repair diagnostic counts; request/time/disk/cancellation values; and authorization reconciliation.
- A passing independent verifier and all acceptance gates for closure. If authorization is exhausted first, close as unqualified with the exact remaining unmet criterion.

## 16. Handoff notes

Continue S019’s existing sequential campaign. Attempts 25 and 26 are consumed; 27 of 120 combined starts are consumed and the current supplemental grant has 36 starts remaining. The existing authorization remains controlling. Never expose the temporary OpenCode Go key in command output, evidence, or Git.

## Attempt 26 finding and v82 correction

Attempt 26 completed after 42.3 minutes and met the active-work horizon, all tool/job/event/resource gates, and the public continuation check. The safe verifier summary reports one failed transcript lifecycle/repair assertion. The prompt-only v81 edit had replaced the explicit job_id/job_state labels with a generic phrase; _h007_case_requires_repair_job_pairs therefore returned false, and the receipt’s per-pair repair diagnostic was null. The diagnostic is sufficient to show the strict named-pair contract was not active, but it cannot say whether either pair was correctly ordered. v82 must restore all four literal labels and add a regression that asserts the marker detector is active, so the next safe receipt reports the per-pair booleans. Attempt 26 evidence: plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt26-safe-diagnostic-20261110.json.


## Attempt 27 outcome and next corrective

Attempt 27 used v82, reached 54.9 active minutes, and ended unqualified at the prompt-work cap. The verifier passed 2/5 tests. All stages and gates were reported complete, but the harness rejected continuation because the named-pair contract was incomplete. `catalog_format` failed the immediate seed-status predicate; `catalog_store` qualified. The pair validator returned one qualified pair while the repair-cycle correlator persisted zero. This mismatch, not a provider/daemon/ACP/event issue, is the new harness-side owner for S021. The end-to-end import/recovery/lineage/diff/export verifier test also failed, but its assertion message is excluded and no source-level cause is asserted. Preserve the hidden verifier boundary.

S020 is superseded by S021 for the remaining corrective work. S021 targets the event-pairing inconsistency and makes the next-native-call requirement explicit in v83. Attempt 27 safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt27-safe-diagnostic-20261110.json`; receipt SHA-256 `4c4361f6a656c0efcf9ebfa6125d89f80c7617773c47bf68bc308d5701a35c4b`.
