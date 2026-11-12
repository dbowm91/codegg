# Harness Lab Self-Improvement Resumption Milestone S026 — Public Sequence Audit

Status: **implemented; attempt 33 completed unqualified; S027 active**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` with the authorized S019–S025 work in the shared worktree.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md` §20

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None; this is a bounded task-facing sequence-audit prompt correction.

Primary class: **capability**

## 1. Objective

Use the safe attempt 32 verifier aggregates to guide a focused public-contract audit of the remaining continuation-boundary and transcript/job-lifecycle failures. Re-run the unchanged four-crate Rust task and independent verifier through the existing Muse/OpenCode Go route. The model should identify and repair any public task defect from the workspace contract and its own observable job/continuation evidence without access to hidden verifier details.

## 2. Evidence and bounded inference

Attempt 32 used v86, ended normally after 1,904,940 ms total and 1,894,823 ms active work, and crossed the 30-minute active-work gate. The agent completed all public phases and gates, documented three observed repair cycles, qualified both required named repair pairs, observed every one of 18 test jobs terminally, demonstrated exact-ID productive overlap, and completed reviewer lifecycle. The task still failed; the unchanged verifier passed 3/5.

The only remaining safe failure domains were `continuation_boundary: 1` and `transcript_job_lifecycle: 1`. Each aggregate reported one pattern assertion, one `AssertionError`, and the safe message category `assertion_message`. Both carried the same allowlisted term set, including `before`, `index`, `observed`, `repair`, `job`, `running`, `workspace`, and `test`. Public continuation, stage/gate, repair-count, exact-job, overlap, and terminal-job projections all reported complete. These signals suggest a chronological or textual public-evidence mismatch, but they do not disclose the exact failed condition. This is an inference from coarse categories; it does not establish a verifier defect or a harness root cause.

S025's new domain detail made the failure category more specific without exposing hidden test names, values, assertion text, or raw output. Attempt 32's trace was complete: 981 events, 436/436 paired tool calls, zero explicit event gaps/drops, and an independently computed maximum gap of 240,305 ms across native tool events. Post-run correlated repair markers preserve their original occurrence times, so aggregate gaps across all trace row types are not a valid native inactivity measurement. The model/provider path completed 379/1,200 requests with no failed provider attempts in the final run. Lowest free disk was 151,965,368,320 bytes. No inactivity or disk stop occurred.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt32-safe-diagnostic-20261110.json`; private receipt SHA-256 `12e670619a4916bb281f413f9c1c08586babddf51daefb3add7d270c3eb14fa6`.

## 3. Scope

### In scope

1. Derive v87 from v86 with prompt-only changes that report attempt 32's safe outcome and direct a public-contract audit of chronological job, repair, continuation, and workspace-qualification evidence.
2. Ask the agent to use native CodeGG tools, write a small read-only audit helper where useful, exercise unusual but authorized job/tool sequences, and use one read-only subagent to challenge its interpretation of the public requirements.
3. Have the agent check submit/status/terminal order for exact job identities, failure-to-repair ordering, state counts, continuation sequencing, and final workspace qualification. Any local mutation testing must use disposable copies and must not weaken the task or verifier.
4. Keep the single Muse model route, paired ARM64 binaries, task body, case tools, limits, independent verifier, hidden-verifier identity, and all qualification criteria unchanged.
5. Perform focused offline verification, then attempt 33 under the existing authorization and caps.

### Explicitly out of scope

- Reading, invoking, quoting, or reverse-engineering hidden verifier source, test identities, assertion text, or raw output.
- Editing the task implementation, public contract, verifier, success criteria, paired binaries, daemon, scheduler, ACP protocol, or request-admission policy before the live agent has a public reproduction.
- Treating S025's safe terms as the exact hidden requirement; they are category evidence only.
- Artificial delays, idle waiting, broad harness redesign, or additional provider routes.

## 4. Invariants

- Use only `opencode_go/muse-spark-1.3-contributor` with the current paired ARM64 binaries.
- Preserve the 1,200-request, 1,600-native-call, 3,600-second wall, 3,300-second prompt-work, 300-second inactivity, 10-GiB disk-floor, and 100-follow-up limits. No USD cap is authorized; report provider allowance and billed USD as `unknown` when unreported.
- Keep task and verifier identity unchanged; v87 changes only prompt/group/title metadata from v86.
- Preserve exact-ID job handling, at least two qualifying repair pairs, productive work during a running job, complete continuation, reviewer lifecycle, and at least 30 minutes of substantive active work.
- Keep task completion, verifier success, and horizon qualification separate. Only all unchanged verifier checks plus all horizon gates qualify H007.
- Preserve all prior attempt records and consume the next start before provider invocation. Do not record credential values, raw provider output, hidden test names, assertions, task results, transcripts, job IDs, or arguments.

## 5. Ordered work packages

### A — Preserve attempt 32 and S025

Record the safe per-domain aggregate, attempt receipt digest, verifier outcome, separate horizon result, native-event gap calculation, provider/disk evidence, and current authorization balance. Mark S025's telemetry implementation complete but attempt 32 unqualified.

### B — Create v87 prompt-only fixture

Derive from v86. Include only safe attempt 32 outcomes and aggregate diagnostic categories. Tell the agent the task remains unqualified despite the horizon pass. Ask it to reconstruct the public event order from native observations and its own workspace record; validate the sequence with a small local read-only checker; use one read-only subagent to find missing or contradictory public evidence; run disposable negative cases for duplicate, missing, and reordered lifecycle events; then correct the actual task artifact or implementation only where the public contract demonstrates a defect. Preserve all existing acceptance criteria and all non-prompt case fields.

### C — Verify offline

Add a regression that checks v87's prompt contains the safe attempt 32 facts and sequence-audit instructions, stays within the prompt bound, preserves all non-prompt fields and verifier identity, and contains no hidden test names or assertion text. Run focused H007/verifier/autonomy tests, relevant assertion-quality and duplication guards, Python compilation, manifest identity/equivalence checks, and `git diff --check`. Do not rebuild the Rust binaries.

### D — Attempt 33

Preflight the existing Muse entry without revealing its value, budget admission, disk reserve, private lane, v87 manifest, verifier identity, and paired binary/helper hashes. Count the authorized start before invocation. Run the unchanged workload through the normal CodeGG harness, monitoring safe request, job, event, inactivity, and disk aggregates. After terminal state, inspect only an allowlisted safe receipt projection, including the new domain summary.

## 6. Acceptance criteria

- v87 differs from v86 only in prompt/group/title; the case, task, tools, budgets, and verifier remain identical.
- Offline tests prove the sequence-audit directions are explicit and do not include hidden identities, values, or assertions.
- Attempt 33 completes the task and passes all five unchanged verifier checks in addition to every H007 horizon gate. Duration or complete public projections alone are insufficient.
- If the same pattern domains fail, preserve the safe details and make the next correction from a reproduced public contract issue or an explicitly identified missing public projection; do not infer private verifier behavior.
- All caps remain respected. Any task, verifier, or horizon failure is recorded as unqualified and consumes the start.

## 7. Stop conditions and authorization

- Do not start unless the route is Muse, the auth entry is present, the v87 manifest and verifier identity match, the paired ARM64 binaries match, the budget is admitted, the private lane is unused, and at least 30 GiB are free.
- Stop on user cancellation, the 10-GiB disk floor, 300-second inactivity limit, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, or 100 follow-ups.
- Attempt 32 consumed start 10/40 in the supplemental Muse authorization and 33/120 combined. Attempt 33 passed preflight and its start is counted as 11/40, leaving 29 supplemental starts. The combined ledger is 34/120, with 86 remaining.

## 8. Closure evidence

Record the changed fixture and tests, v87/v86 manifest hashes, verifier and binary identities, preflight and authorization counts, safe receipt digest, model route, separate task/verifier/horizon outcomes, redacted domain aggregates, request and provider attempt counts, native tool event gaps, job/repair/reviewer evidence, and disk minima. Do not store credentials, raw logs, transcript, task result body, hidden test identity/assertions, raw IDs, or arguments.

## Attempt 33 result

Attempt 33 completed unqualified. It used the authorized Muse route and unchanged task/verifier identities, ran for 3,299,945 ms with 3,268,583 ms of active work, and reached the 30-minute horizon. The independent verifier ran five checks and passed 3/5. The task outcome was `failed`; H007 remains unqualified.

The public projection identifies the concrete repair-order defect: `catalog_format` qualified, but `catalog_store` did not. Its seed failure was eventually observed, and a distinct repair job eventually completed, but the repair rerun was submitted before the failed seed had been observed terminally and without a productive source change between that terminal observation and rerun. The summary therefore counted one of the required two named pairs and one correlated repair cycle. The continuation projection was invalid even though all seven phases, all required gates, the exact overlap, reviewer evidence, and terminal job observations were present. All 11 submitted jobs reached terminal states (8 completed, 3 failed); none remained unobserved.

The verifier's safe domain counts were one `continuation_boundary` and one `transcript_job_lifecycle` failure. No hidden assertion text or output was inspected. The live trace was complete with 279/279 paired calls, zero recorded gaps/drops, and an 83,030 ms maximum native-event gap. Exact-ID productive overlap and reviewer completion were observed. Provider requests were 263/1,200; disk remained above the floor; the stall watcher did not trigger; and cleanup found no owned daemon left behind. Three provider attempts failed with retry scheduled; no provider timeout class was observed. Provider allowance was USD 0.178673; billed USD remains unknown.

The run ended at the configured 3,300-second prompt-work boundary with `continuation_stop_reason: turn_timeout`; the 3,600-second wall cap retained its verifier reserve. The receipt's top-level `stop_reason` still said `end_turn`, which was stale and misleading after the timeout. Existing safe group diagnostics already identify the incomplete `catalog_store` predicates, but the follow-up exposed only aggregate repair counts and the generic pair requirement. S027 adds pair-specific, ID-free feedback and an explicit timeout projection, then reattempts the same task with a shorter prompt that prioritizes serial completion of both required pairs. This is an agent-facing sequencing/feedback correction, not evidence of daemon, scheduler, ACP, disk, or event-loss failure.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt33-safe-diagnostic-20261110.json`; private safe-receipt SHA-256 `fd9d832c583c674ac0616535e9fb4619e61eb727d1991d240939a1a585035b7d`. The start consumed 11/40 supplemental and 34/120 combined starts; 29 supplemental and 86 combined starts remain.
