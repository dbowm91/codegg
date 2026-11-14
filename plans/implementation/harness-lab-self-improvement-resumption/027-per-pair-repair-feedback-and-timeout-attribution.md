# Harness Lab Self-Improvement Resumption Milestone S027 — Per-Pair Repair Feedback and Timeout Attribution

Status: **implemented; attempt 34 completed unqualified; S028 active**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` with the authorized S019–S026 work in the shared worktree.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#21-s027--per-pair-repair-feedback-and-timeout-attribution`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None; this is a bounded agent-facing feedback and evidence correction.

Primary class: **capability**

## 1. Objective

Use attempt 33's safe public projection to correct the specific incomplete repair-pair sequence, make the same-session follow-up identify which named pair and ordering requirement remain incomplete, and make the final receipt report a harness prompt timeout consistently. Then retry the unchanged H007 task and independent verifier under the existing authorization. The corrective is implemented; attempt 34 confirmed the pair feedback and timeout receipt, but did not qualify H007. S028 owns the remaining task-pacing and reviewer-receipt issue.

## 2. Why this milestone is ready

Attempt 33 demonstrates sustained operation rather than a short-run or infrastructure stop: it reached 3,268,583 ms of active work, with 279/279 native tool calls paired, complete event capture, exact-ID productive overlap, reviewer completion, and all 11 submitted jobs terminal. The independent verifier passed 3/5, so H007 remains unqualified.

The safe repair projection is specific. `catalog_format` qualified. `catalog_store` did not: its failed seed and completed repair were both observed, but the repair was submitted before the failed seed's terminal observation and there was no productive source change between that observation and the rerun. The follow-up has access to these per-pair booleans but currently gives only aggregate repair counts and a generic requirement. The continuation document therefore remained invalid with only one of two required pairs.

The run also ended at the 3,300-second prompt-work boundary (`continuation_stop_reason: turn_timeout`) while the 3,600-second wall cap retained a verifier reserve. The final receipt kept the prior `end_turn` value as `stop_reason`, making the actual timeout harder to identify. Historical S019 safe receipts show a maximum verifier duration of 240,330 ms, so retain the existing 300-second reserve.

No event-loss, daemon-cleanup, scheduler, disk-floor, activity-stall, or provider-timeout failure was observed. The correction should stay in the harness's prompt feedback and safe receipt projection; it must not widen the workload or weaken qualification.

## 3. Current implementation evidence

- `dev/harness_lab/autonomy_run.py` computes safe per-pair repair summaries and uses them in `_h007_followup_prompt`.
- The current incomplete-pair follow-up does not name the incomplete pair or the failed ordering predicates, despite those values being available in the bounded projection.
- A caught `HarnessTimeout` sets `terminal` and `continuation_stop_reason` but leaves `stop_reason` unchanged from the last completed prompt.
- Existing H007 follow-ups preserve exact job identity, the two named repair-pair requirement, the 3,300-second prompt-work boundary, and a 300-second verifier reserve.
- v87 added a lengthy public-sequence audit on top of the existing task. Attempt 33 ended with the second repair pair still invalid. v88 should retain the original task and verifier, disclose only safe attempt 33 outcomes, and prioritize the two pairs serially before optional diagnostic work.

## 4. Invariants that must not regress

- Use only `opencode_go/muse-spark-1.3-contributor` and the paired ARM64 binary/helper.
- Keep the task implementation, hidden verifier, tools, request/native-call limits, wall limit, 10-GiB disk floor, inactivity rule, cancellation policy, and qualification criteria unchanged.
- Keep the 300-second verifier reserve; it covers the observed 240.33-second maximum verifier duration with bounded margin.
- Preserve separate task, verifier, and horizon outcomes. A sustained session with a failed task does not qualify.
- Do not expose hidden verifier source, tests, assertions, raw output, task result bodies, transcript contents, credentials, raw job IDs, or tool arguments.
- Keep diagnostic feedback derived only from public task requirements and safe native job observations; emit pair labels and failed predicates, never IDs or arguments.
- Preserve user-requested live behavior: native tool use, one read-only subagent, a small helper when useful, and bounded unusual/failure probes on disposable state.

## 5. Scope

### In scope

- Add ID-free pair-specific follow-up text from the existing repair summary, including the exact ordering predicate that remains false for each named pair.
- Set the top-level stop reason to `turn_timeout` when `HarnessTimeout` is caught and record a bounded H007 timeout projection with prompt count, follow-up count, last request timeout, elapsed request time, and remaining wall-clock time.
- Add v88 as a prompt-only derivative of v86. Include the safe attempt 33 result and direct the agent to complete each named failure/repair pair serially before optional audit work.
- Add focused regressions for incomplete-pair feedback, redaction, timeout classification, and v88 manifest identity.
- Requalify through the unchanged independent verifier and existing live caps.

### Explicitly out of scope

- Editing the Rust task, public tests, hidden verifier, qualification criteria, provider adapter, request budget, 300-second reserve, daemon, scheduler, ACP protocol, or job lifecycle architecture.
- Broad harness redesign, another provider route, artificial delay, no-op work, or replacing the realistic workload with a deterministic fixture.

## 6. Required changes

### Harness feedback and timeout telemetry

- Derive incomplete pair labels from the existing bounded `repair_job_diagnostic.groups` projection.
- For each incomplete pair, state only the false predicates needed to correct it: observe the exact seed terminal before the repair submission, make a productive correction between that observation and rerun, observe the distinct rerun terminal, and document the evidence once.
- Keep the latest observed/documented cycle counts and do not repeat a completed pair.
- On `HarnessTimeout`, set `stop_reason` to `turn_timeout` and preserve the independent `terminal` and `continuation_stop_reason` fields.
- Add `turn_timeout_diagnostic` to H007 receipts. Include only schema, observed/not-observed state, prompt request count, follow-up count, last request timeout, elapsed request duration, and remaining wall-clock budget. Do not retain exception text or provider output.

### v88 fixture

- Derive from v86 and change only prompt/group/title metadata.
- State that attempt 33 was unqualified because only one of the two named pairs qualified; identify the catalog-store ordering issue in public-contract terms.
- Require the failed seed's terminal observation before editing and before submitting its repair; do the two pairs serially rather than queueing both seeds.
- Preserve the dependent Rust stages, native exact-ID overlap, reviewer, helper, bounded probes, 30-minute active-work minimum, and unchanged verifier. Remove the extra S026 sequence-audit experiments so core task completion stays first.

## 7. Ordered work packages

### A — Preserve attempt 33

Write a safe evidence projection and update the S026 plan/closure, S019 status, registry, and roadmap. Record the exact incomplete pair predicates, timeout classification, verifier domains, resource health, and consumed authorization. Do not inspect protected logs, raw task artifacts, or hidden verifier data.

### B — Correct prompt feedback and timeout projection

Implement the ID-free per-pair follow-up and timeout receipt fields. Keep the configured work and verifier budgets unchanged. Add a regression proving a failed `catalog_store` ordering is named without job IDs or arguments and a timeout cannot retain a stale `end_turn` reason.

### C — Prepare and verify v88

Create v88 from v86 with only prompt/group/title changed. Its prompt must identify the safe attempt 33 outcome, prioritize serial repair ordering, and retain the user's request for real native tool use, a read-only subagent, a small helper, and bounded adverse probes. Verify prompt length, all unchanged case fields, and verifier identity.

### D — Offline qualification and preflight

Run focused autonomy/H007/diagnostic tests, Python compilation, assertion-quality and duplication guards, manifest identity checks, and `git diff --check`. Do not rebuild the ARM64 binaries. Before the live start, confirm auth presence without reading its value, route, v88 manifest, hidden verifier hash, paired binary/helper hashes, budget admission, fresh lane/receipt, and at least 30 GiB free. Count the start before invocation.

### E — Attempt 34

Run the unchanged H007 task and independent verifier using the same Muse route and all existing caps. Monitor only safe event, request, lifecycle, repair, timeout, and disk projections. On completion, write a safe attempt receipt projection and stop immediately if it qualifies. Otherwise preserve the failure, classify task versus harness/provider/resource behavior, and register the next narrow correction within the remaining authorization.

## 8. Failure, cancellation, and resource semantics

- Preserve the existing 3,300-second prompt-work limit inside the 3,600-second hard wall cap and the 300-second verifier reserve. Do not count an expected prompt deadline as a provider stream failure.
- A `HarnessTimeout` is recorded as `turn_timeout`; a provider timeout remains a distinct provider failure; `verifier_reserve` remains a distinct pre-prompt stop.
- Keep 1,200 provider requests, 1,600 native calls, 100 follow-ups, 300-second activity inactivity cancellation, 10-GiB disk floor, and user cancellation behavior unchanged.
- Any incomplete tool/job observation remains unqualified. Preserve exact job lifecycle evidence only in protected/private runtime storage and expose aggregate counts in the receipt.

## 9. Compatibility

The receipt addition is an additive H007 diagnostic. Preserve schema compatibility for other autonomy cases. No Rust source, binary, database, protocol, or fixture task/verifier changes are allowed.

## 10. Required tests

### Focused unit tests

- Incomplete `catalog_store` follow-up names the public pair label and missing ordering predicates.
- Generated follow-up contains no raw job ID or tool arguments.
- Timeout projection records a `turn_timeout` rather than stale `end_turn`, and keeps prompt/provider timeout categories distinct.
- Existing verifier-reserve and turn-timeout boundary tests continue to pass.

### Harness and fixture tests

- v88 is a prompt-only derivative of v86 with matching non-prompt fields and unchanged verifier identity.
- v88 includes attempt 33's safe outcome and serial repair instruction, stays within the prompt length cap, and includes no hidden assertion text.
- Existing S019 focused suites, Python compilation, quality guards, and `git diff --check` pass.

### Live test

- Attempt 34 meets all task, verifier, repair-pair, lifecycle, overlap, continuation, reviewer, resource, and 30-minute active-work criteria; or its safe result is recorded unqualified with the specific next corrective owner.

## 11. Required verification commands

```bash
rtk python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics
rtk python3 -m unittest dev.harness_lab.tests.test_autonomy
rtk python3 scripts/check_test_assertion_quality.py
rtk python3 scripts/check_test_duplication.py
rtk python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/diagnostics.py
rtk git diff --check
```

Run exactly the focused commands needed after implementation; report their actual results in closure evidence.

## 12. Documentation updates

- S019 status and safe evidence record.
- S026 closure and implementation-plan status.
- S027 closure record after attempt 34.
- `plans/registry.md` and the S019 subsystem roadmap.

## 13. Acceptance criteria

- Safe follow-up identifies the incomplete named repair pair and its public ordering defect without disclosing IDs, arguments, transcripts, or hidden verifier details.
- H007 receipt correctly reports `turn_timeout` and the bounded prompt-timeout context, without stale `end_turn` attribution.
- v88 retains the task/verifier and all non-prompt case fields from v86.
- Attempt 34 completed unqualified. It reached 54.78 minutes of active work, and both named repair pairs qualified with four observed/documented cycles. All 11 test jobs were terminal; exact-ID overlap, complete event pairing, and the read-only reviewer lifecycle were observed. The task timed out at the configured prompt-work boundary with `bounded_probes` and `independent_review` incomplete. The reviewer result was retrieved once, but the five public continuation receipt fields were not written. The unchanged verifier passed 4/5; its one safe `transcript_job_lifecycle` failure reconciled to incomplete public evidence. There was no provider timeout, stall stop, event gap, disk-floor event, or unobserved test job. This is task sequencing and evidence recording, not evidence of a daemon, scheduler, or ACP failure.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt34-safe-diagnostic-20261110.json`. Attempt 34 consumed 12/40 supplemental and 35/120 combined starts; 28 supplemental and 85 combined starts remain. S028 owns attempt 35.

## 14. Stop conditions

Stop for user cancellation, resource floor, request/tool/follow-up/wall caps, missing authorization entry, mismatched binary/verifier/manifest identity, or any need to change the task/verifier/qualification contract. Do not infer hidden verifier behavior from safe terms.

## 15. Closure evidence required

Record code and test changes, v88/v86 manifest hashes, verifier and binary identities, preflight, authorization counters, safe receipt digest, exact route, separate task/verifier/horizon outcomes, per-pair booleans, timeout classification, request/provider attempt counts, event/job/reviewer totals, native event-gap maximum, disk minimum, and billed cost as `unknown` if not reported. Never record credential values, raw logs, transcripts, task result bodies, hidden test IDs/assertions, job IDs, or tool arguments.

## 16. Handoff notes

Attempt 34 consumed 12/40 supplemental starts and 35/120 combined starts; 28 supplemental and 85 combined starts remain. Attempt 35 consumes one start before provider invocation, leaving 27/40 supplemental and 84/120 combined starts. The OpenCode Go credential remains in the mode-0600 OpenCode auth file; never print or copy its value. Repeat the 30-GiB preflight reserve check before the next run.
