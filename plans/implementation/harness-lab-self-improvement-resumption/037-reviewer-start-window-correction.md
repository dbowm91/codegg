# Harness Lab Self-Improvement Resumption Milestone S037 — Reviewer Start-Window Correction

Status: **conditionally closed for reviewer start-window correction; H007 remains unqualified**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (dirty worktree; preserve all existing edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#31-s037--reviewer-start-window-correction`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs: None required.

Primary class: capability

## 1. Objective

Correct H007's reviewer admission timing so the required independent review has enough of the existing work window to finish and recover from a transient child deadline. Keep the task, verifier, review scope, route, model, qualifications, and per-run limits unchanged.

## 2. Why this milestone is ready

- Attempt 47 completed 3,299.888 seconds of active work. All seven public phases and all three required artifacts were present; exact-ID job overlap, four repair cycles, event capture, and the activity-gap limit passed.
- The sole incomplete public gate was independent review. The reviewer was due at 1,800,000 ms but first observed at 1,956,558 ms, failed with `deadline_exceeded` after 1,205,060 ms, and left only about 138 seconds before the parent prompt-work deadline.
- The parent made one reviewer spawn, 22 status calls, and 12 waits, but no result fetch or same-run continuation. The independent verifier ran five tests and passed three; the run therefore remains unqualified.
- Provider and native-event accounting were complete enough to attribute the failure. The 292 provider requests had no classified timeout, and all 339 tool starts had terminal events with zero gaps or drops. No additional telemetry or daemon, scheduler, ACP, or provider change is indicated.
- `autonomy_run.py` already selects a 15-minute reviewer threshold unless a prompt marker opts into the delayed 30-minute path. The reviewer child deadline is 1,200 seconds. This milestone removes that late opt-in for the next fixture.

## 3. Current implementation evidence

- The unchanged workload and verifier are in `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v93/cases.jsonl`.
- Attempt 47's privacy-safe receipt is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt47-safe-diagnostic-20261111.json`.
- Its schedule, reviewer, durable-state, stop, and verifier projections distinguish the late start from provider transport, ACP event loss, and task-job lifecycle failures.
- `dev/harness_lab/autonomy_run.py` provides both the existing 15-minute and 30-minute thresholds. S037 must use the 15-minute path without changing production runner behavior.

## 4. Invariants that must not regress

- Use only `opencode_go/muse-spark-1.3-contributor` for live qualification.
- Preserve the v93 task inputs, public tests, hidden verifier identity, native tool set, exact-ID requirements, repair requirements, stage/gate contract, and acceptance criteria.
- Keep exactly one read-only reviewer. Fetch its Task exactly once only after the matching Run is observed `completed`; use the same Run for one allowed transient-deadline continuation and never spawn a replacement.
- Keep the existing 1,200-request and 1,600-native-call caps, 3,300-second prompt-work limit, 3,600-second hard stop, 300-second inactivity cancellation, 100 follow-ups, 10-GiB runtime disk floor, and 30-GiB preflight reserve.
- Never retain prompt text, provider text, transcripts, hidden-verifier source/output, job IDs, Task/Run IDs, credential material, or failure messages in safe evidence.
- Do not infer a daemon, scheduler, ACP, or provider defect without a reproduced failure in that component.

## 5. Scope

### In scope

- Create v94 as a prompt-only derivative of v93, selecting the existing 15-minute reviewer admission path and making that timing unambiguous.
- Add a regression proving that v94 changes no task, verifier, tool, route, budget, or fixture-input field and no longer opts into the 30-minute threshold.
- Run focused offline checks, then one sequential live attempt under the remaining S019 grant.
- Reconcile the attempt's safe evidence and qualification result.

### Out of scope

- Changes to Rust production code, task-tool deadlines, daemon/scheduler/ACP/provider architecture, verifier tests, workload implementation, or qualification thresholds.
- Expanding reviewer scope, adding a second reviewer, or extending the live limits.
- Changing the model/provider route or using the previous credential.

## 6. Required production changes

None. The correction is limited to the harness fixture prompt and a regression test. Existing runner thresholds and child limits remain unchanged.

## 7. Ordered work packages

### Work package A — Reconcile attempt 47

Record attempt 47 as a consumed, unqualified start in the S019 ledger and closure status. Preserve its 55-minute active-work result, seven completed phases, 11/12 public gates, four repairs, 339/339 tool-event pairs, zero event gaps/drops, reviewer deadline, verifier 3/5 outcome, disk minimum, request count, and unknown externally billed USD.

### Work package B — Prepare v94

Derive v94 from v93. Change only `group_id`, `title`, and the reviewer-timing prompt. Remove the delayed-review marker, select the existing 15-minute threshold, and state that this timing takes precedence over older timing instructions. Keep one reviewer, the existing concise review scope, exact-ID lifecycle, and same-run transient-deadline recovery instructions.

### Work package C — Offline qualification

Add a fixture regression that compares every non-presentation field between v93 and v94, confirms the delay marker is absent, confirms the 15-minute path is selected, and pins the unchanged provider/model and resource caps. Run the focused H007 tests, test assertion-quality and duplication guards, Python compilation for edited files, and `git diff --check`.

### Work package D — Attempt 48

Run v94 once using the existing paired ARM64 CodeGG and sandbox-helper binaries and only the authorized Muse route. Require the normal exact-manifest, verifier, credential-availability, disk, and counter preflight. Keep the disk watcher active and stop under the existing cancellation rules. Do not run concurrently.

### Work package E — Diagnose and reconcile

Inspect only safe aggregates. If unqualified, classify the task, reviewer, verifier, job, event, provider, or resource domain from existing projections; do not broaden architecture or add telemetry unless the receipt demonstrates a specific missing measurement. Update S019 and S037 records and continue only within the remaining explicit grant.

## 8. Failure, cancellation, restart, and contention semantics

- A refusal before the first provider request consumes no live start. Once admitted, every timeout, cancellation, provider failure, and unqualified result consumes one start.
- A reviewer deadline is recoverable once on the same Run only for an allowed transient failure and while sufficient parent work budget remains. Do not replace the reviewer.
- Stop on user cancellation, sensitive-data exposure, unreadable or below-floor disk, loss of request-admission telemetry, 300 seconds without native-tool activity, 1,200 provider requests, 1,600 native calls, 3,300 seconds of prompt work, 3,600 seconds wall time, 100 follow-ups, or model/credential identity mismatch.
- Keep the existing 300-second independent-verifier reserve and record unreported billed USD as `unknown`.

## 9. Compatibility and migration

No product schema or protocol change. v94 is additive; v93 and all earlier fixture manifests and receipts remain historical. The protected OpenCode Go credential stays in the existing store and is never copied into plans or evidence.

## 10. Required tests

- v94 preserves every v93 field except the allowed presentation fields and prompt.
- v94 selects the 15-minute reviewer threshold and contains no delayed 30-minute opt-in.
- Existing reviewer lifecycle and deadline-recovery tests remain unchanged and pass.
- Safe receipts continue to omit raw prompt, reviewer, provider, transcript, verifier, and identifier data.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_h007
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/tests/test_h007.py dev/harness_lab/autonomy_run.py
git diff --check
```

## 12. Documentation updates

Update the S019 ledger and status, this plan's closure record, the subsystem roadmap, and registry. Preserve attempt 47 as unqualified. State explicitly that attempt 47's review started too late for the configured child deadline and parent prompt-work budget; do not describe it as a provider or event-stream failure.

## 13. Acceptance criteria

Attempt 48 qualifies only if the unchanged independent verifier passes and every existing H007 condition passes: at least 30 minutes of active dependent Rust development, all required phases/gates/artifacts, exact-ID running-to-terminal background work with productive overlap, both repairs, continuation, completed independent review with one correctly ordered Task fetch, healthy event and activity-gap measurements, and resource/cancellation compliance.

## 14. Stop conditions

Do not launch if v94 changes any protected fixture/verifier identity or non-prompt contract field, the route/model or paired binaries differ, credential availability cannot be safely confirmed, authorization counters are inconsistent, or free disk is below the 30-GiB preflight reserve. During a run, use only the existing cancellation bounds. Report and stop rather than weaken qualification if reviewer evidence remains incomplete.

## 15. Closure evidence required

- Focused offline verification with exact results.
- v94 manifest and prompt-only parity evidence.
- Attempt 48 preflight, safe receipt, exact model attribution, event/provider/disk measurements, reviewer schedule and lifecycle aggregates, and reconciled authorization counters.
- A requirement matrix separating H007 task completion from the active-work horizon and the exact reviewer/transport/resource outcome.
- If H007 remains unqualified, a successor only when the new evidence identifies a distinct corrective.

## 16. Handoff notes

Attempt 47 consumed 4/40 starts in grant `s019-supplemental-20261111-04`. Attempt 48 used combined start 49 and consumed grant start 5/40; 35 grant starts remain. Combined accounting is 49/200 with 151 remaining, preserving one historical unrepresented start offset. Attempt 49 is next. The model remains `opencode_go/muse-spark-1.3-contributor`; externally billed USD remains unknown when unavailable.


## 17. Offline verification

Passed before attempt 48 on the v94 corrective: python3 -m unittest dev.harness_lab.tests.test_h007 (83 tests), both test-quality guards, Python compilation of the changed H007 test and autonomy runner, and git diff --check. Attempt 48 confirmed the early reviewer lifecycle, but H007 did not qualify; see `plans/closure/harness-lab-self-improvement-resumption/037-status.md` for the exact gate matrix. No Rust production source or binary changed.
