# Harness Lab Self-Improvement Resumption Milestone S035 — Prompt-Boundary Progress and Overlap Escalation

Status: **conditionally closed at diagnostic scope; attempt 43 completed unqualified; S036 owns terminal-attribution correction**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#29-s035--prompt-boundary-progress-and-overlap-escalation`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–29
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: infrastructure

## 1. Objective

Correct the agent-facing continuation path exposed by attempt 42. Preserve the existing 60-minute hard stop and five-minute verifier reserve, make public stage progress visible at every prompt boundary, and prioritize the next required action when progress stalls. At the 30-minute reviewer threshold, direct the agent to spawn the single reviewer before doing other work. During required background work, direct it to perform productive work while an exact-ID job remains running. Requalify the existing v93 task with its frozen inputs and hidden verifier.

## 2. Why this milestone is ready

Attempt 42 used `opencode_go/muse-spark-1.3-contributor`, ran for 3,299.840 seconds with 3,252.830 seconds of active work, and passed the duration and maximum-gap gates. It used 162/1,200 provider requests and 234/1,600 native calls. The first five ACP prompts ended normally; the sixth timed out after 180.447 seconds with 180.4 seconds allowed to preserve the verifier reserve. The run ended 160 milliseconds before the 3,300-second prompt-work ceiling. Provider accounting showed 161 successful attempts, zero failed or timed-out attempts, and one unresolved attempt at the controller stop.

Event pairing was complete (234/234, no gaps or drops). All 12 submitted test jobs reached terminal observation (seven completed, five failed), and both required named repair pairs passed (four total correlated repair cycles). Four exact job identities were observed running and then completed, but none had a productive write, edit, patch, shell, or test call between those observations. Therefore genuine productive overlap did not qualify.

The final public checkpoint still listed all seven implementation stages as pending, ten gates as incomplete, and all three required public artifacts as absent. Workspace tests and Clippy were not submitted. The reviewer was spawned at about 41 minutes; its durable Run later reached completed, but the agent made no status, wait, or get call before the turn timed out. The hidden verifier's bounded diagnostic classified setup as `workspace_build_failure` and recorded zero tests run. This is not a verifier pass or a demonstrated assertion failure.

No daemon, scheduler, ACP event-loss, job-terminal-observation, disk-floor, or provider-retry failure was observed. The evidence points to task-progress, reviewer-priority, and asynchronous-overlap behavior inside the bounded agent turn. Prompt-boundary state snapshots are needed to tell whether stage progress was absent throughout or simply not recorded in the final checkpoint.

## 3. Current implementation evidence

- `dev/harness_lab/autonomy_run.py` already computes stage and gate obligations for each follow-up and subtracts the verifier reserve from remaining wall time. It records per-prompt timing and tool counts, but not the public stage/gate state at each prompt boundary.
- The reviewer-due marker reached the first follow-up after 30 active minutes. The follow-up included reviewer instructions, but the reviewer was spawned only after about ten more minutes of work. The instruction was not placed first as the next-call requirement.
- `dev/harness_lab/diagnostics.py` now produces redacted verifier pattern signatures and pseudonymous job sequences. Attempt 42's verifier setup error meant there was no regex mismatch to classify; the sequence still established that no productive call occurred during any observed running-to-completed job interval.
- Attempt 42's private receipt has a mode-0600 safe projection with a body-free tool activity sequence, reviewer lifecycle, prompt boundaries, and public gate statuses. It contains no hidden test identities, verifier text, prompt text, tool arguments, outputs, or job IDs.

## 4. Invariants that must not regress

- Use only `opencode_go/muse-spark-1.3-contributor` for live runs; never print or persist credentials.
- Keep case manifest v93, source inputs, public tests, tools, hidden verifier, verifier digest, and H007 qualification thresholds unchanged.
- Preserve the 1,200 provider-request, 1,600 native-call, 3,300-second prompt-work, 3,600-second wall, 300-second inactivity, 100-followup, 30-GiB preflight reserve, 10-GiB runtime floor, and five-minute verifier-reserve bounds.
- Keep exact job IDs only inside the existing in-memory correlation; receipts contain per-run aliases such as `J01` plus allowlisted action/state/timing fields.
- A completed reviewer in durable storage is not agent-observed completion. The required native status-then-get lifecycle remains unchanged.
- A task timeout or setup error is not a task pass. Do not infer a product defect from model task incompletion.
- Never read, print, or modify the hidden verifier source or raw verifier output.

## 5. Scope

### In scope

- Record a bounded public continuation snapshot at every H007 prompt boundary: stage and gate statuses, next pointers, required-job completion, repair-pair counts, overlap state, reviewer state, and public artifact presence. Keep note contents and raw identities out of the receipt.
- Record explicit prompt-work allowance, consumed work, remaining work, and verifier reserve alongside request timing, so a run ending near a cap can be attributed precisely.
- Use the pseudonymous job aliases to produce a bounded ordered tool-activity sequence and count productive calls between exact running and completed observations.
- When two consecutive prompt boundaries show no advancement of the first missing required stage or gate, put a concise stage-specific recovery instruction at the beginning of the next follow-up. Tell the agent to advance that stage, update/read back the public checkpoint, and defer optional probes.
- When the reviewer threshold is reached, make the first native action in the next follow-up the single reviewer spawn. Keep the exact status/wait and one ordered get rules intact.
- When an exact required job is observed running without intervening productive work, put a direct instruction before the next job poll to complete one productive implementation action while it remains active.
- Run attempt 43 using v93 unchanged and the original Muse/OpenCode Go route and caps.

### Explicitly out of scope

- Changing the task contract, stage order, work complexity, public tests, verifier, run caps, or qualification thresholds.
- Reopening daemon, scheduler, job-store, ACP transport, provider adapter, multi-provider routing, or product runtime architecture.
- Inspecting verifier source/output, raw provider payloads, transcripts, task result bodies, raw job IDs, or tool arguments.
- Increasing the prompt-work ceiling or removing the verifier reserve.

## 6. Required production changes

### Harness telemetry

- Add a versioned `prompt_progress` snapshot to each H007 prompt-boundary record. Derive it from the existing bounded continuation reader and public projections only. Record a SHA-256 of the checkpoint when present, never its text.
- Include safe counts for completed/remaining stages and gates, plus bounded enum values for next stage, next gate, repair pairs, success jobs, overlap, reviewer, and artifact state.
- Add `prompt_work_limit_ms`, `prompt_work_consumed_ms`, `prompt_work_remaining_ms`, and `verifier_reserve_ms` to prompt-boundary evidence. Use the same `guard` accounting that controls the request timeout.
- Add a compact activity projection with call index, allowlisted tool class/action/status, pseudonymous job alias, and start/finish elapsed times. Include only enough summary to prove whether productive work occurred between the exact job's running and completed observations.
- Preserve compatibility: older receipts may omit these fields; no database migration is required.

### Agent-facing follow-up

- Track consecutive prompt boundaries with no forward progress on the first missing required stage or gate. On the second such boundary, prepend one concise recovery instruction that names the public next stage, the bounded acceptance evidence, and the need to update/read back the checkpoint before optional work.
- If the 30-minute reviewer marker is due, prepend a next-call instruction to spawn exactly one read-only verifier before other calls. Do not spawn automatically on the agent's behalf.
- If the required overlap job is running and has no productive intervening calls, prepend a next-call instruction to work on an independent required task before polling that exact job again.
- Do not auto-mark phases, gates, jobs, or reviewer state complete. Continue to require native evidence and independent verification.

### Storage and migrations

None.

### Product runtime and concurrency

No production daemon, scheduler, provider, or ACP runtime changes.

## 7. Ordered work packages

### Work package A — Public progress snapshots

Intent: determine whether an incomplete final checkpoint reflects no progress, lost progress, or a checkpoint regression.

Required changes: project safe stage/gate/job/reviewer/artifact state at prompt start and terminal boundary. Retain only schema, enums, counts, booleans, timings, and a checkpoint digest.

Acceptance evidence: synthetic tests prove snapshots advance monotonically when public state advances, retain no sentinel note text/IDs/paths, and report malformed or missing notes as unavailable rather than complete.

### Work package B — Prompt-work attribution and safe tool activity

Intent: explain the actual timeout boundary and verify asynchronous overlap from ordered calls.

Required changes: make prompt work and verifier reserve explicit in each boundary. Correlate exact job IDs internally, then publish only `Jnn` aliases and a count of productive calls between running and completed observations.

Acceptance evidence: tests prove a prompt that reaches the bound is classified as controller `turn_timeout`, not provider failure; the request-plus-reserve cannot exceed the wall cap; exact IDs alias consistently; and zero productive calls remains zero in the summary.

### Work package C — Targeted continuation priorities

Intent: direct the next call toward the highest-value unmet acceptance condition.

Required changes: after two no-progress boundaries, put the next-stage recovery at the beginning of the follow-up. At the reviewer threshold, require reviewer spawn as the next native call. While the required overlap job remains running without productive calls, prioritize one independent productive action before polling.

Acceptance evidence: table-driven tests cover progress, no-progress, malformed checkpoint, reviewer due, and running-job cases. The prompt still preserves all original stage/gate names, IDs, verifier boundary, and budgets.

### Work package D — Focused verification

Intent: validate privacy and follow-up control before another provider run.

Required changes: run `test_autonomy_run`, `test_verifier_diagnostics`, relevant H007 projection tests, assertion-quality and duplication guards, Python compilation, and `git diff --check`.

Acceptance evidence: exact commands and results are recorded in the closure evidence.

### Work package E — Attempt 43

Intent: test whether better stage-progress and boundary guidance lets the same model complete v93.

Required changes: full private preflight; same binary/helper pair; Muse Spark 1.3 Contributor through OpenCode Go; unchanged v93 manifest and hidden verifier; 1,500-second requested prompt timeout; all existing caps; sequential run; continuous disk monitoring.

Acceptance evidence: mode-0600 preflight, safe receipt, exact-ID tool/job sequence, per-prompt progress timeline, provider attempt accounting, disk/watch evidence, and full ledger reconciliation. A pass still requires all original task, horizon, job, repair, overlap, continuation, reviewer, and hidden verifier conditions.

## 8. Failure, cancellation, restart, and contention semantics

- Existing cancellation, disk-floor, inactivity, request, native-call, prompt-work, wall-clock, follow-up, and verifier-reserve behavior stays unchanged.
- A missing or malformed continuation snapshot is recorded as unavailable. It cannot satisfy a stage or gate.
- No-progress recovery prompts do not reset any cap, grant authority, or create a new provider request outside the existing budget.
- Reviewer state observed only in durable storage remains unobserved to the agent until the exact native status and get sequence occurs.
- An unresolved provider attempt at a controller timeout remains unresolved; it is not relabeled as provider failure.

## 9. Compatibility and migration

Receipt fields are additive and versioned. Older receipts remain readable with missing prompt snapshots treated as unavailable. No task manifest, product persistence, protocol, or database migration changes are required.

## 10. Required tests

### Focused unit tests

- Prompt snapshots cover complete, pending, missing, and malformed public checkpoints with counts and safe enums only.
- Two consecutive no-progress snapshots produce a recovery prefix; any forward stage/gate progress resets the counter.
- The reviewer due follow-up starts with the exact single-spawn requirement; non-due follow-ups preserve existing behavior.
- A running exact-ID job without productive overlap produces a next-call reminder; productive work resets it.
- Prompt-work consumed/remaining values reconcile with wall time minus the reserved verifier window and never become negative.
- The body-free activity sequence preserves order and aliases while excluding IDs, arguments, outputs, paths, prompt text, and task result bodies.

### Integration tests

- Production-shaped ACP tool events create a prompt-boundary snapshot and an alias-correlated activity sequence without changing existing task or horizon outcomes.
- A final turn timeout retains the controller reason, request timeout, unresolved attempt count, prompt-work remainder, and verifier reserve separately.

### Security and negative tests

Use unique sentinels in continuation text, tool outputs, IDs, paths, and task results; assert none appear in any new receipt projection. Verify every new enum rejects unknown text.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_autonomy_run
python3 -m unittest dev.harness_lab.tests.test_verifier_diagnostics
python3 -m unittest dev.harness_lab.tests.test_h007
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/diagnostics.py
git diff --check
```

## 12. Documentation updates

Record attempt 42 as duration-qualified but task-incomplete and H007-unqualified. Name the controller timeout at the 3,300-second prompt-work budget, the `workspace_build_failure` setup class with zero tests run, incomplete stages, missing overlap, unobserved reviewer lifecycle, complete event trace, and available resource evidence. Do not quote hidden output or identify hidden tests. Record the explicit separation between task execution failure and harness infrastructure failure.

## 13. Acceptance criteria

- Every prompt boundary has a bounded public progress snapshot and explicit prompt-work accounting.
- Safe sequence evidence proves productive overlap or identifies its absence without raw IDs or tool content.
- After repeated no-progress boundaries, the next prompt names the first missing stage and directs evidence-backed advancement.
- The 30-minute reviewer follow-up makes reviewer spawn the first native call.
- Attempt 43 uses the exact Muse/OpenCode Go route, v93, unchanged hidden verifier, and original caps.
- H007 qualifies only if task completion, 30–60-minute activity, activity gap, exact-ID overlap, two named repair cycles, continuation, all gates, reviewer, and independent hidden verifier all pass.
- No daemon, scheduler, ACP, provider, or product runtime redesign occurs without new direct evidence.

## 14. Stop conditions

Stop before launch if route, credential availability, verifier/fixture identity, ARM64 binary/helper pair, budget policy, idle lane, authorization counts, or 30-GiB disk reserve fails preflight. Stop during execution on cancellation, any run cap, 10-GiB floor breach, or incomplete/untrustworthy telemetry.

## 15. Closure evidence required

- Focused test and guard results.
- Attempt-43 preflight and mode-0600 safe evidence.
- Updated S019 status, roadmap, registry, and authorization ledger.
- A requirement matrix that distinguishes model/task incompletion from runtime, transport, job-observation, or resource failures.
- If H007 remains unqualified, a successor only when the new checkpoint and activity diagnostics support a narrower corrective.

## 16. Handoff notes

Offline implementation changed only Harness Lab diagnostics, follow-up construction, receipt projection, and focused tests. Prompt-boundary snapshots include a checkpoint digest, exact required stage/gate states, required job/repair/reviewer/artifact booleans, and no checkpoint body or raw IDs. Prompt-work start/end accounting reconciles to the 3,300-second ceiling and 300-second verifier reserve. The ordered activity projection aliases job IDs and reports productive calls inside running-to-completed intervals. Repeated public-checkpoint stalls, reviewer deadline, and required-job overlap now lead the next prompt with bounded next-action guidance.

Focused verification passed: `python3 -m unittest dev.harness_lab.tests.test_autonomy_run` (68 tests), `python3 -m unittest dev.harness_lab.tests.test_verifier_diagnostics` (30 tests), `python3 -m unittest dev.harness_lab.tests.test_h007` (82 tests), both Python assertion-quality and duplication guards, Python compilation, and `git diff --check`. The current free-disk sample is about 51 GiB, above the 30-GiB preflight reserve.

Attempt 42 consumed start 43: 5/40 latest-grant starts consumed and 43/160 combined starts consumed. Attempt 43 consumed start 44, leaving 34 starts in the prior grant and 116 combined. A fresh 40-start authorization was then recorded for the next diagnostic/corrective loop. It ended after a 300-second activity-stall cancellation with 24:59 active work and an incomplete task; S036 records and corrects the stale top-level end_turn attribution. The authorized model is `opencode_go/muse-spark-1.3-contributor`; attempt 40 failed before provider startup and invoked no model. Billed USD is reported only when the provider route supplies it; otherwise it remains `unknown`.
