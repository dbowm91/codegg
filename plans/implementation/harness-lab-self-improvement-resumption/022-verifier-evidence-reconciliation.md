# Harness Lab Self-Improvement Resumption Milestone S022 — Verifier Evidence Reconciliation

Status: **implemented**

Repository baseline: branch `corrective/s016-raised-request-horizon-20261102`, working tree at `ac584c886f03847ff6b829f8509667637e42a052` with previously recorded local implementation work.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md` §§14–16

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None required; this is a bounded harness diagnostic and live requalification.

Primary class: **capability**

## 1. Objective

Make a failed independent verifier result distinguishable from a harness or event-observation failure when the public task projection reports all corresponding evidence complete, then use that safe diagnosis in one fresh Muse Spark H007 attempt.

## 2. Why this milestone is ready

S021’s offline corrective passed its focused checks and attempt 28 used the authorized Muse route. The agent completed 35.9 minutes of active work, both named exact-ID repair pairs, the release-scale overlap, all continuation stages and gates, and the exact reviewer lifecycle. The complete causal trace recorded 653 events with no gaps, drops, or unpaired native calls; resource controls remained healthy. The agent ended normally. The independent verifier ran five checks and failed two categories concerning the durable continuation boundary and transcript job lifecycle, despite the public projections reporting those criteria complete. There is no evidence of provider, daemon, scheduler, ACP, stall, or disk failure.

The current receipt contains the safe categories and public projections but does not provide one explicit comparison result per failed category. S022 adds that bounded reconciliation and feeds only the public diagnosis into v84.

## 3. Current implementation evidence

- `dev/harness_lab/diagnostics.py` already excludes assertion text, tracebacks, raw verifier output, provider payloads, task arguments, and transcripts; it retains bounded test identifiers and safe assertion categories.
- `dev/harness_lab/h007_results.py` records task and horizon outcomes independently and projects continuation, repair, job, overlap, and artifact evidence.
- Attempt 28’s public summary reported all stages/gates complete, two correlated repair cycles, all 15 submitted test jobs terminal, required workspace tests and Clippy completed, reviewer completed, and 292/292 native call events paired.
- The independent verifier outcome was two assertion failures out of five; public assertion text and verifier source remain outside the evidence boundary.

## 4. Invariants that must not regress

- Never read, invoke, expose, or prompt the agent with hidden verifier source, assertion text, traceback, or raw output.
- Keep raw job IDs, arguments, provider content, and transcript out of persisted diagnostic summaries.
- Preserve the distinction between task completion and long-horizon qualification.
- Preserve the frozen task, hidden verifier, single `opencode_go/muse-spark-1.3-contributor` route, paired ARM64 binaries, and all existing per-run limits.
- Do not attribute a task-verifier failure to the harness when event, job, provider, disk, and lifecycle evidence is complete.

## 5. Scope

### In scope

1. Add a finite safe failure-domain projection for verifier test identifiers, including continuation-boundary and transcript/job-lifecycle domains.
2. Compare those domains with the already available public continuation and horizon evidence. Report whether the public projection was incomplete, complete, or unavailable when the independent verifier failed.
3. Add deterministic tests proving both the attempt-28-shaped disagreement case and a case where public evidence is also incomplete. Assert that diagnostic output contains no assertion text or raw IDs.
4. Create v84 as a prompt-only derivative of v83. It may include attempt 28’s safe outcome categories and ask the agent to re-audit public artifacts and native lifecycle evidence before marking gates complete. It must not expose hidden verifier internals or change the task contract.
5. Run focused offline verification, then attempt 29 under the recorded authorization and monitor all existing controls.

### Explicitly out of scope

- Changing the hidden verifier or its acceptance criteria.
- Reading hidden verifier source, assertion text, traceback, or raw output.
- Changing the Rust workload, required repair sequence, model route, provider architecture, daemon, scheduler, ACP protocol, request cap, wall-clock cap, or resource floors.
- Adding a new testing corpus or weakening any H007 gate.

## 6. Required production changes

### Diagnostic projection

Derive only allowlisted failure domains from bounded test identifiers. For each failed domain, record the corresponding public projection state as `complete`, `incomplete`, or `unavailable`. If the verifier fails while the projection reports complete, identify the result as a verifier/public-projection disagreement; do not claim either side is wrong without more evidence. Keep assertion-message content out of the new field.

### Prompt fixture

Create v84 from v83 with only group/title metadata and a short diagnostic context sourced from attempt 28’s safe receipt. Keep all runtime limits, case behavior, task requirements, and frozen input identities unchanged.

## 7. Ordered work packages

### Work package A — Safe verifier reconciliation

Intent: make the relationship between independent failures and public evidence explicit.

Required changes: add the bounded failure-domain and evidence-reconciliation projection plus regression tests.

Acceptance evidence: a synthetic attempt-28-shaped result projects the continuation and job-lifecycle domains as verifier/public-projection disagreements; incomplete public evidence is classified separately; serialized output contains no message text, raw IDs, arguments, or transcript.

### Work package B — v84 and offline checks

Intent: give the next live model only the public failure information needed to re-audit its work.

Required changes: derive v84 from v83; assert all non-prompt case fields match and that the added diagnostic context contains no hidden data.

Acceptance evidence: focused H007/diagnostic tests, assertion-quality and duplication guards, Python compilation, manifest digest, and `git diff --check` pass.

### Work package C — Attempt 29

Intent: test the corrected diagnosis and public re-audit prompt through the real CodeGG `opencode_go` path.

Required changes: perform a no-request preflight, then one sequential run with existing caps and continuous resource/lifecycle monitoring.

Acceptance evidence: a safe receipt records the exact model route, resource controls, continuation/reviewer/job/repair events, verifier counts and safe failure domains, outcome, and actual provider-reported cost or `unknown`.

## 8. Failure, cancellation, restart, and contention semantics

Run one attempt at a time. Treat the start as consumed on invocation. Stop on qualification, user cancellation, any existing run-wide hard cap, or exhaustion of the recorded combined authorization. A failure before the model request due to preflight consumes no start. Preserve every attempt receipt and do not retry a cancelled or terminated run under the same attempt number.

## 9. Compatibility and migration

No persisted product schema, provider contract, or Rust binary changes. Diagnostic JSON additions remain bounded and additive. v84 must preserve all v83 non-prompt case fields and the frozen hidden-verifier identity.

## 10. Required tests

### Focused unit tests

- Verifier test identifiers map only to the finite supported diagnostic domains.
- A failed verifier with complete public continuation/job projections yields `verifier_public_projection_disagreement`.
- A failed verifier with incomplete public evidence yields `public_evidence_incomplete`.
- No assertion message, traceback, job identity, task argument, or transcript is serialized.
- v84 has the same non-prompt fields and hidden-verifier hash as v83.

### Static checks

- `python3 scripts/check_test_assertion_quality.py`
- `python3 scripts/check_test_duplication.py`
- Python compilation and `git diff --check`.

## 11. Required verification commands

```bash
rtk python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_causal_trace dev.harness_lab.tests.test_h007_attribution dev.harness_lab.tests.test_verifier_diagnostics
rtk python3 scripts/check_test_assertion_quality.py
rtk python3 scripts/check_test_duplication.py
rtk python3 -m compileall -q dev/harness_lab
rtk git diff --check
```

Run the focused checks before the live attempt. Do not rebuild Rust binaries unless an input affecting either binary changes.

## 12. Documentation updates

Update the S019 authorization ledger, the S022 closure/status, this plan’s closure record, the subsystem roadmap, and registry with attempt 28’s result, attempt 29’s authorization consumption, and the safe reconciliation outcome.

## 13. Acceptance criteria

- The diagnostic projection safely distinguishes an independent-verifier/public-projection disagreement from incomplete public evidence.
- v84 changes no frozen task, verifier, or runtime configuration.
- Attempt 29 is qualified only if the unchanged independent verifier passes and every existing H007 criterion passes. Duration or complete-looking public artifacts alone do not qualify.

## 14. Stop conditions

- Stop if the opencode_go credential entry, model route, ARM64 binaries, verifier identity, budget admission, or 30-GiB free-disk preflight check fails.
- Stop on the existing 10-GiB free-disk floor, 300-second inactivity bound, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, or 100 follow-ups.
- Do not claim qualification for an unavailable, setup-failed, timed-out, incomplete, or assertion-failing verifier.
- Do not inspect hidden verifier source or assertion details to diagnose a disagreement.

## 15. Closure evidence required

- v84 manifest and prompt hashes, v83 parent hash, focused test/guard results, binary and verifier identities.
- Attempt 29 preflight and final safe receipt, receipt digest, model route, request count, provider-reported cost or `unknown`, active span, disk minimum, event completeness, exact job/reviewer lifecycle, repair-pair results, verifier test counts and safe failure domains.
- Authorization reconciliation showing combined starts and remaining supplemental starts.
- Whether attempt 29 qualified; if not, the exact public conditions still unmet and the next narrow corrective supported by evidence.

## 16. Handoff notes

Use only `opencode_go/muse-spark-1.3-contributor`. After attempt 29, the supplemental grant has 33 starts remaining and the combined authorization has 90 starts remaining. Do not change the cap or route. The temporary OpenCode Go auth entry is selected locally; never print or persist its credential value.

## Offline implementation result

Added a bounded verifier failure-domain projection and a task-level reconciliation that compares continuation-boundary and transcript/job-lifecycle failures with their public evidence. It reports `verifier_public_projection_disagreement`, `public_evidence_incomplete`, or `public_evidence_unavailable` without retaining assertion text, raw output, job IDs, or transcripts. The task completion summary also records `independent_verifier_failures` explicitly while preserving `verification_failed` as the state.

Created v84 from v83 with only group/title/prompt changes. v83 parent manifest SHA-256 is `8e36f5c03299e4089d950b0564a019deb99890c2cdccd3d6f352f4861b5c9d6a`; v84 SHA-256 is `b95124e46c1ecf325d38d54e6d91723e8043ef3e790b3e2345fd30a2288554ce` (10,967 prompt characters). All non-prompt case fields and the hidden-verifier identity are unchanged. Attempt 28’s safe receipt classifies both failed domains as verifier/public-projection disagreements.

Offline verification passed: 186 tests across the five focused harness modules; assertion-quality and duplication guards; Python compilation; and `git diff --check`. S022 changes only Python diagnostics/tests and the prompt fixture; no paired Rust binary rebuild is required. Attempt 29 completed on the recorded Muse route but did not qualify: it timed out before task completion and the unchanged verifier passed 2/5. S023 owns the narrow public-task behavior and final-checkpoint correction.
