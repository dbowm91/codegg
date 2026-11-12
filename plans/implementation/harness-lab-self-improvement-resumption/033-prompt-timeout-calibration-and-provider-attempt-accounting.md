# Harness Lab Self-Improvement Resumption Milestone S033 — Prompt Timeout Calibration and Provider Attempt Accounting

Status: **conditionally closed; timeout/attempt accounting passed; H007 unqualified**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#27-s033--prompt-timeout-calibration-and-provider-attempt-accounting`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–29
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: infrastructure

## 1. Objective

Use attempt 39's safe timing and event evidence to distinguish an outer ACP prompt timeout from a completed or failed provider attempt, and give the next same-session H007 prompt a longer but still bounded execution window. Keep the Muse/OpenCode Go route, v93 task, hidden verifier, original qualification contract, and all run-wide caps unchanged.

## 2. Why this milestone is ready

S032's prompt-priority change is present in v93, and attempt 39 had complete event capture and observed terminal job states. Its exact failure is now bounded: three ACP prompt requests ended normally in about 10.4–10.6 minutes; the fourth reached the 900-second prompt timeout while producing 39 native calls. The run recorded 97 provider attempts started, 96 succeeded, zero failed, and no provider timeout event. The remaining started/succeeded difference indicates one attempt lacked a terminal marker when collection stopped; it does not establish why. Disk and the event stream stayed healthy.

S032 delivered the requested prompt correction but did not meet its live repair acceptance criterion. Its disposition is recorded in `plans/closure/harness-lab-self-improvement-resumption/032-status.md`.

## 3. Current implementation evidence

- `dev/harness_lab/diagnostics.py` safely projects provider-attempt start, success, and failure counts from the private provider log but does not report the unclosed-at-stop difference.
- `dev/harness_lab/autonomy_run.py` records per-prompt duration and call-count boundaries, reserves 300 seconds for the independent verifier, and terminates the case after a prompt request timeout.
- Attempt 39's safe receipt has exact model identity, per-prompt elapsed times/call counts, task outcome, job observations, event pairing, disk floor, and verifier aggregate. Its evidence file is mode `0600`.

## 4. Invariants that must not regress

- Use only `opencode_go/muse-spark-1.3-contributor`; do not emit or persist credential values.
- Preserve v93's task, tools, hidden verifier, verifier digest, and every original qualification threshold.
- Preserve the exact-ID job lifecycle requirement, productive overlap, both correctness-bearing repair cycles, continuation evidence, and completed independent verifier.
- Keep request, native-call, prompt-work, wall-clock, inactivity, disk, cancellation, concurrency, and unreported-USD rules unchanged.
- Treat incomplete provider-attempt accounting as unresolved evidence, never as success or a classified provider timeout.
- Keep protected raw logs, output, prompts, arguments, IDs, and verifier content out of safe evidence.

## 5. Scope

### In scope

- Add a safe unresolved-at-stop provider-attempt count and accounting-consistency field to the existing provider diagnostic, with regression tests.
- For attempt 41, raise the harness CLI prompt request timeout from 900 to 1,500 seconds. The existing run budget still recomputes the effective bound from remaining wall time and preserves the 300-second verifier reserve.
- Record attempt 39 and attempt 40 using allowlisted evidence only; attempt 41 remains the qualification run, not a new verifier or altered task.

### Explicitly out of scope

- Reopening daemon, scheduler, ACP projection, job store, provider adapter, task fixture design, hidden verifier, or model integration.
- Weakening qualification thresholds, adding artificial delay, retrying a failed test without a source change, or classifying an unresolved attempt as a provider failure.
- Changing authorization or spending beyond the remaining 38 starts in grant `s019-supplemental-20261110-03`.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

No runtime ownership change. Preserve the prompt timeout as a bound, not a new background retry. A timed-out session is not assumed to have completed; current teardown and cancellation semantics remain unchanged.

### Frontend or operator surface

None.

### Security and authorization

Provider diagnostics remain allowlisted and body-free. The attempt-40 preflight must recheck the approved model route, protected credential availability without reading or printing its value, idle lane, paired ARM64 binaries, disk reserve, and authorization counts.

### Documentation and static guards

Update this plan, S019 closure, roadmap, registry, and safe attempt evidence with the observed timeout policy and diagnostic interpretation.

## 7. Ordered work packages

### Work package A — Reconcile attempt 39

Intent: close stale status and preserve the exact model attribution and failure evidence.

Required changes: mark run 39 `completed_unqualified`; record 97 provider requests, 40/160 combined starts consumed, 2/40 starts in the latest grant consumed, and the correct November 11 safe evidence path/digest. Retain no raw output.

Acceptance evidence: ledger and S019 status agree with the mode-0600 safe receipt; no stale `running` status or earlier date path remains.

### Work package B — Add unresolved provider-attempt accounting

Intent: distinguish a provider attempt with a terminal failure from one whose terminal event was not observed before the run stopped.

Required changes: derive an unresolved count only from a complete, non-truncated provider log. Expose a consistency boolean and use null when the log is absent, unreadable, oversized, or truncated. Never expose attempt IDs or raw log text.

Acceptance evidence: tests cover successful retry, terminal failure, one started-but-unclosed attempt, privacy, and incomplete/truncated input.

### Work package C — Recover the launcher and prepare attempt 41

Intent: give a productive long ACP prompt enough bounded time to finish without changing the overall hour, prompt-work cap, or verifier reserve.

Required changes: use v93 unchanged with CLI `--timeout 1500`; preserve the provider request limit of 1,200, native-call limit of 1,600, prompt-work limit of 3,300 seconds, 3,600-second wall stop, 300-second activity stop, 100 followups, 30-GiB start reserve, and 10-GiB runtime floor.

Acceptance evidence: preflight records the exact route/model, v93 digest, verifier digest, binary/helper pair, timeout option, budget counts, and free disk without recording secret material.

### Work package D — Execute and diagnose attempt 41

Intent: test the exact candidate while preserving all qualification criteria.

Required changes: run sequentially; monitor disk; stop on user cancellation or any hard cap; project the complete receipt through a bounded allowlist. If it does not qualify, name whether it was task incomplete, outer ACP timeout, provider timeout, activity gap, event loss, disk stop, or missing job observation.

Acceptance evidence: task and horizon projections, independent verifier aggregate, model route, prompt-boundary counters, unresolved-at-stop provider count, resource counters, and ledger all reconcile.

## 8. Failure, cancellation, restart, and contention semantics

A prompt timeout is distinct from a provider timeout and from a whole-run wall-clock stop. The existing 300-second verifier reserve remains protected. A timed-out provider attempt with no terminal marker is unresolved. Do not infer success, failure, or a provider defect from the count difference alone. Do not restart a timed-out prompt concurrently or claim that job state was lost when the event trace records its terminal status. No parallel run is permitted.

## 9. Compatibility and migration

The diagnostic schema change is additive. It does not affect persisted product data, the CodeGG runtime protocol, provider selection, or task manifests. The attempt-specific CLI timeout is limited to the private harness invocation for H007.

## 10. Required tests

### Focused unit tests

- Provider log with a terminal success has zero unresolved attempts.
- Provider log with a terminal failure has zero unresolved attempts.
- Complete log with a started attempt and no terminal event has one unresolved attempt and no retained ID.
- Truncated or oversized log reports unresolved count as null.

### Integration tests

No new live provider test besides authorized attempt 41.

### Restart and recovery tests

Not applicable; this plan does not alter runtime recovery.

### Contention and cancellation tests

No concurrency change; verify the existing attempt cancellation and resource caps remain unchanged in the launch preflight.

### Security and negative tests

Assert provider attempt IDs, provider body, and unrelated log content do not appear in the safe diagnostic.

### Migration and compatibility tests

No migration. Existing safe diagnostics remain readable with missing new fields.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_verifier_diagnostics
python3 -m py_compile dev/harness_lab/diagnostics.py dev/harness_lab/tests/test_verifier_diagnostics.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
```

## 12. Documentation updates

Record that the attempt 39 failure was an outer ACP prompt timeout, not a classified provider timeout; note the one unresolved provider attempt only as incomplete terminal evidence. Update S019's run ledger and current start counts before attempt 41.

## 13. Acceptance criteria

- Focused provider-attempt diagnostic tests pass and the privacy projection retains no raw ID or content.
- Attempt 41 uses the exact Muse/OpenCode Go route, v93, unchanged verifier, and a 1,500-second requested prompt timeout, with the effective timeout bounded by remaining wall time minus the verifier reserve.
- Attempt 41's active work is correctly measured; all original H007 horizon, exact-ID overlap, two named repair, continuation, task, and verifier criteria remain unchanged.
- Qualification is declared only if the unchanged hidden verifier passes and every original criterion is met.
- If it fails, the safe record distinguishes observed task incompletion from an outer prompt timeout, a classified provider timeout, unresolved provider attempt, transport/event loss, resource stop, or job-observation gap.

## 14. Stop conditions

Stop before launch if route/model, credential availability, binary pair, fixture/verifier identity, idle lane, authorization ledger, or 30-GiB preflight disk reserve is not verified. Stop a run immediately on cancellation, request/call/wall/inactivity cap, 10-GiB disk-floor breach, or untrustworthy event telemetry. Do not consume more than the remaining authorized starts.

## 15. Closure evidence required

- Focused command outputs and diff check.
- Attempt-40 preflight and mode-0600 safe evidence, with correct route/model and no secret, prompt, verifier text, IDs, or raw tool output.
- Updated authorization ledger, S019 status, roadmap, and registry counts.
- A requirement-to-evidence disposition stating whether H007 qualified; if not, a narrowly scoped successor only when attempt evidence identifies one.

## 16. Handoff notes

The current grant is `s019-supplemental-20261110-03`: attempt 40 consumed 3 of 40 starts, so 37 remain. The combined ledger is 41/160 starts consumed, so 119 remain. Attempt 40 consumed start number 41 before reaching the provider because the newly created private run directory lacked `budget.json`; no provider request was made. Attempt 41 will consume start number 42 if admitted. OpenCode Go billed USD is unavailable; report it as `unknown` when the route does not report it.


## 17. Attempt 40 launch failure

The launcher created a new private directory but did not create its required `budget.json`. The autonomy CLI exited with code 2 in 5.022 seconds; no receipt was written and provider request count remained zero. The allowlisted startup sanitizer identified `missing_budget_file`. The external disk monitor stayed above the floor and did not stop the process. The start is retained as consumed. Attempt 41 must create the policy `{ "max_provider_requests": 1200, "max_wallclock_seconds": 3600 }` in mode `0600`, assert it matches the preflight, and then run the unchanged v93 case with `--timeout 1500`.


## 18. Attempt 41 result and successor

Attempt 41 used v93 unchanged with `opencode_go/muse-spark-1.3-contributor`. All five ACP prompt requests ended normally within the requested 1,500-second bound; the final request received a dynamically reduced 1,207.8-second bound to preserve the verifier reserve and completed in 252.3 seconds. The run recorded 247/247 provider attempts successful, zero failed, and zero unresolved.

The task reached 38.59 active minutes and passed the exact-ID running-to-terminal overlap, productive work during that job, both named failure/edit/repair pairs, five correlated repair cycles, continuation, reviewer completion, all seven stages/twelve gates, and required workspace/Clippy jobs. All 21 test jobs were terminal; 274/274 tool calls had terminal ACP events; zero gaps/drops; maximum activity gap 80.833 seconds; disk minimum 101,345,566,720 bytes.

The unchanged verifier passed 3/5 and reported two pattern assertion failures in `continuation_boundary` and `transcript_job_lifecycle`. It ran all five tests with no setup error. S034 adds safe expected/observed pattern marker classes and an exact-ID-preserving pseudonymous job sequence before another task run. H007 remains unqualified. Safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt41-safe-diagnostic-20261111.json`; receipt digest `5f681a96d855ce1e3626dbc5c64faa32aa2ac2c8753334c5c1fada21bddc3fd9`.
