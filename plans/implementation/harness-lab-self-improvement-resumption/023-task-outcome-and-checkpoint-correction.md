# Harness Lab Self-Improvement Resumption Milestone S023 — Task Outcome and Checkpoint Correction

Status: **implemented; attempt 30 unqualified; S024/S025 implemented; S026 active**

Repository baseline: branch `corrective/s016-raised-request-horizon-20261102`, working tree at `ac584c886f03847ff6b829f8509667637e42a052` with the recorded local S019–S022 work.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md` §§14–17

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None required; this is a bounded fixture-prompt and live qualification corrective.

Primary class: **capability**

## 1. Objective

Use attempt 29's safe evidence to target the unqualified task outcome and stale continuation checkpoint, then run attempt 30 through the unchanged native CodeGG and OpenCode Go path. S023 is implemented, but attempt 30 remained unqualified; S024 owns the numeric live repair-count feedback required by the next run.

## 2. Why this milestone is ready

Attempt 29 used `opencode_go/muse-spark-1.3-contributor` and reached the 3,300-second turn cap after 54.9 minutes of active work. The duration gate passed, but the agent terminal was `timeout`, the task failed, and the horizon remained unqualified. The provider, causal-event, job, reviewer, disk, and inactivity telemetry was sufficient to distinguish this from an infrastructure stall.

The verifier ran five checks and passed two. Safe failure domains were `continuation_boundary`, `transcript_job_lifecycle`, and `task_behavior`. The public continuation had all phases, stages, gates, artifacts, job terminals, and reviewer evidence, but `document_valid` was false: both named repair pairs qualified, while the trace recorded three qualifying cycles and the note documented two. The `task_behavior` failure was not reproduced by the 15-test offline workspace suite, the 15/15 public CLI acceptance driver, or a 10/10 combined public-contract sequence. That result does not establish that the independent verifier is wrong; it leaves one task-behavior edge case unresolved.

S022's body-free verifier/public-evidence reconciliation is implemented and its focused offline checks passed. No evidence supports expanding the daemon, scheduler, ACP, provider, or request-admission architecture.

## 3. Current implementation evidence

- Attempt 29 safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt29-safe-diagnostic-20261110.json`.
- Receipt SHA-256: `0cafae58cd67fc4705fc7e7f85d71e4f9e69b7f4631b577ba1e728b19cf262ca`.
- The 822-event trace had no drops, all 354 native calls paired, a maximum activity gap of 74,646 ms, exact-ID productive overlap, and complete reviewer/job terminal observations.
- The 318 provider requests were below the unchanged 1,200-request limit. Provider failures, retries, stream timeouts, disk-floor cancellation, and inactivity cancellation were not observed. Reported OpenCode Go allowance was USD 0.166339.
- Public diagnostics show two of two required repair pairs qualified and three correlated cycles observed; the continuation count was stale at two.
- Offline checks after attempt 29: `cargo test --offline --locked --workspace` passed 15 tests; `tools/acceptance.py` passed 15/15; the additional combined recovery/import/lineage/diff/export sequence passed 10/10. None accessed the hidden verifier.
- v85 prompt-only manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v85/cases.jsonl`; SHA-256 `fd271cfb9e008c9808bae05ba753dd8f8ffd8a2aac89e029d90d9f6880d93dc2`; parent is v84 SHA-256 `b95124e46c1ecf325d38d54e6d91723e8043ef3e790b3e2345fd30a2288554ce`.

## 4. Invariants that must not regress

- Use only `opencode_go/muse-spark-1.3-contributor` and the already-authorized runner binaries.
- Preserve the 1,200 provider-request, 1,600 native-call, 3,600-second wall, 3,300-second prompt-work, 300-second inactivity, 10-GiB disk-floor, and 100-follow-up limits. No USD ceiling is authorized; report provider allowance when available and otherwise `unknown`.
- Keep the task contract, corpus, public tests, hidden verifier, case behavior, tools, and frozen manifest fields unchanged. v85 may change only prompt/group/title metadata.
- Never inspect, invoke, quote, or infer hidden verifier source or assertion text. Persist only safe categories/counts, never raw provider output, job IDs, arguments, or transcripts.
- Keep task completion separate from duration qualification. A timeout or failed verifier is not success, even when the active-duration gate passes.
- Do not attribute a task-behavior failure to CodeGG infrastructure without evidence. Do not change daemon, scheduler, ACP, provider, or request-cap architecture in this milestone.

## 5. Scope

### In scope

1. Preserve attempt 29's safe receipt interpretation and public diagnostic results.
2. Use v85 to make the task-behavior lead and live repair-cycle count explicit while preserving all task and run contracts.
3. Perform the existing public-contract checks and one bounded live attempt under the recorded authorization.
4. Diagnose any new failure from safe receipt and causal/job evidence, and create a sibling corrective plan if evidence warrants further changes.

### Explicitly out of scope

- Changing the hidden verifier, its assertions, the public contract, corpus, or frozen public tests.
- Raising any per-run cap, changing provider route/model, or extending the 60-minute hard stop.
- Broad harness, daemon, scheduler, ACP, or multi-provider redesign.
- Adding artificial delays or another synthetic test corpus.

## 6. Required production changes

No CodeGG runtime or Rust production changes are presently justified by attempt 29. The only planned harness change is the prompt-only v85 fixture. If public evidence identifies a specific implementation defect, record it and keep the correction within the existing task fixture or a new narrow plan; do not weaken qualification criteria.

### Prompt fixture

Derive v85 from v84. Add only attempt 29's safe outcome, the unresolved public `task_behavior` category, and explicit final-checkpoint instructions. Keep all non-prompt case fields and the hidden-verifier identity unchanged. Treat `repair_cycles` as a live count: update it after each qualifying cycle, then reconcile it against the complete trace after every required job is terminal.

## 7. Ordered work packages

### Work package A — Preserve and reconcile attempt 29

Intent: keep the outcome and its limits auditable.

Required changes: record the timeout, task/verifier failure domains, public reconciliation, 30-minute duration pass, and public test results in safe evidence, the authorization ledger, S019 status, S022 closure, roadmap, and registry.

Acceptance evidence: no raw verifier output, assertions, hidden source, job IDs, or transcript is present; attempt 29 is marked completed/unqualified and its start remains consumed.

### Work package B — v85 and public checks

Intent: address the diagnosed task and checkpoint issues without changing the task contract.

Required changes: create v85 as a prompt-only derivative of v84. Ask the agent to audit cross-command behavior against the frozen public contract, avoid redundant test submissions, update the repair-cycle count as events occur, and reserve the final 300 seconds for required closeout before the 3,300-second turn cap.

Acceptance evidence: a manifest comparison proves all non-prompt fields and verifier identity unchanged; the public workspace tests, CLI acceptance, and combined diagnostic are recorded without invoking the hidden verifier.

### Work package C — Attempt 30

Intent: test the safe corrective through a real sustained model session.

Required changes: stage the exact v85 manifest, pass a no-request doctor/budget/auth/disk preflight, then start one sequential attempt and monitor disk, request count, job/causal events, activity gaps, and cancellation thresholds.

Acceptance evidence: a body-free receipt records terminal/task/verifier outcomes, active work, all required exact-ID job and repair evidence, resource limits, and reported allowance or `unknown`.

## 8. Failure, cancellation, restart, and contention semantics

Count an authorization start at live harness invocation. A failed preflight consumes no start. Preserve receipts and traces; never reuse an attempt number. Stop on qualification, user cancellation, any unchanged run cap, or authorization exhaustion. If a run ends with a timeout, provider/daemon failure, missing causal events, or insufficient public evidence, classify that exact outcome before changing scope.

## 9. Compatibility and migration

No persisted CodeGG schema, protocol, binary, or credential format changes. v85 is an additive, prompt-only fixture derivative. The active authorization ledger and prior failed receipts remain immutable in meaning; reconcile factual status only.

## 10. Required tests

### Focused public checks

- `cargo test --offline --locked --workspace` in the fixture.
- `python3 tools/acceptance.py` in the fixture.
- A bounded combined public-contract scenario covering torn-tail recovery, later import, lineage, diff, deterministic export, conflict atomicity, and interior-corruption rejection.

### Harness checks

- Focused S022 diagnostics tests and the prompt/manifest invariant check when the fixture changes.
- Assertion-quality and duplication guards if test bodies change.
- `git diff --check` for repository changes.

Do not inspect or execute the hidden verifier directly; it runs only as part of the authorized harness session.

## 11. Required verification commands

```bash
rtk cargo test --offline --locked --workspace
rtk python3 tools/acceptance.py
rtk python3 -m dev.harness_lab doctor --binary /private/tmp/s019-build/target-arm64/aarch64-apple-darwin/release/codegg
rtk python3 -m dev.harness_lab budget-check --budget /private/tmp/s019-muse-runs/budget.json
rtk git diff --check
```

The fixture tests and acceptance driver above passed for attempt 29. Re-run preflight commands for attempt 30; do not claim new offline results unless rerun.

## 12. Documentation updates

Update the S019 authorization ledger and status, S022 closure, subsystem roadmap, registry, and add S023 closure evidence after attempt 30. Keep the route, caps, and remaining-start counts synchronized.

## 13. Acceptance criteria

- v85 changes only prompt/group/title metadata and contains no hidden verifier information.
- Attempt 30 is admitted only after the documented no-request preflight passes.
- The agent terminal is `completed`, the task outcome is passed, and all five independent verifier checks pass.
- At least 30 minutes of active dependent Rust development, required exact-ID job lifecycles, productive overlap, at least two qualified repair pairs, complete continuation/reviewer evidence, and all remaining H007 gates are demonstrated.
- The final documented repair-cycle count matches the complete trace. All requests, tools, event gaps, disk, and cancellation limits remain within the unchanged authorization.
- Report unqualified if any task, verifier, or horizon condition fails, even when the others pass.

## 14. Stop conditions

- Stop before provider invocation if auth, route, binary hash, budget admission, lane isolation, or disk reserve fails.
- Stop the run at any existing cap or cancellation threshold.
- Stop and report rather than infer hidden verifier details or alter its criteria.
- If the `task_behavior` domain remains unlocalized after public contract review and the next live attempt, create a new narrowly scoped plan instead of broadening S023.

## 15. Closure evidence required

Record exact manifest and binary hashes, no-request preflight, authorization counts, model route, live receipt hash, task and horizon outcomes separately, verifier count/domain summary, public evidence reconciliation, provider allowance or `unknown`, resource and event telemetry, test commands/results, and the disposition of every remaining finding. Do not record hidden assertion text, test source, raw output, credentials, raw IDs, arguments, or transcript.

## 16. Handoff notes

The current worktree is intentionally dirty from prior user-authorized S019–S023 work. Do not revert unrelated edits. Reuse the existing paired ARM64 binaries; do not rebuild CodeGG unless one of their inputs changes. Attempt 30 consumed one start: 8/40 starts in the latest grant are consumed and 32 remain, while 31/120 combined starts are consumed and 89 remain.

## Attempt 30 result

Attempt 30 used v85 on the authorized Muse route. It reached the 3,300-second prompt-work cap after 3,293,205 ms active work and 3,299,886 ms total. The agent timed out before task completion; the task failed and the verifier passed 3/5, so H007 did not qualify. The trace was complete with 542/542 native calls paired, zero dropped events, 19/19 test jobs terminal, exact-ID productive overlap, and a completed reviewer. Both named repair pairs qualified; seven correlated and persisted cycles were observed, while the continuation note documented two. The provider, event, job, reviewer, disk, and inactivity controls were healthy. Safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt30-safe-diagnostic-20261110.json`.

The S023 prompt asked the model to reconcile against private trace data and the runner's incomplete-repair follow-up did not include the live observed/documented numbers. S024 now owns the bounded numeric follow-up feedback and the next live attempt.
