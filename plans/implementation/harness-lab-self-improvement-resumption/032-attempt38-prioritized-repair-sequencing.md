# Harness Lab Self-Improvement Resumption Milestone S032 — Attempt-38 Prioritized Repair Sequencing

Status: **implemented; attempt 39 completed unqualified; corrective pass required**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve all other edits)

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#26-s032--attempt-38-prioritized-repair-sequencing`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Primary class: capability

## 1. Objective

Use attempt 38's safe diagnosis to make the two required failure-to-repair sequences the highest-priority work in the next same-session continuation. Preserve the existing workload, independent verifier, route, and run caps. Reattempt H007 only after focused offline verification.

## 2. Evidence and diagnosis

Attempt 38 used v92 and completed 54.53 minutes of active work before the final prompt timed out while preserving the 300-second verifier reserve. It remained unqualified: the independent verifier passed 3/5 with safe domains `continuation_boundary` and `transcript_job_lifecycle`.

The reviewer correction from S031 worked. One reviewer Run and Task matched; the Run reached `completed` after 79,098 ms, a matching Task result was fetched once in the required order, and no continuation action was needed. No replacement reviewer was spawned.

All 19 submitted test jobs had terminal observations, the native causal trace was complete with 283/283 tool starts paired and no gaps or dropped events, provider use was 253/1,200 with no provider timeout, and disk remained above the 10-GiB floor (38.3 GiB minimum). The external hard stop did not fire.

The task's broad horizon counter saw four repair cycles, but the two required named repair pairs both qualified 0/2. Their exact failed job states and completed rerun states were observed and documented; in both cases, the tool trace had zero calls between the terminal failed observation and the repair submission. The required correctness-bearing edit, failure-to-edit ordering, and correlation were absent. The public continuation document was invalid despite all listed stage/gate statuses being complete. This points to the agent not following the already-stated repair instruction; it does not indicate ACP event loss, job-state loss, or a reviewer lifecycle failure.

## 3. Scope

### In scope

- Mark S031 conditionally closed for its reviewer-recovery objective, while keeping H007 qualification explicitly unqualified.
- Create v93 as a prompt-only derivative of v92 except for `group_id`, `title`, and `prompt`.
- Put exact named repair-pair feedback at the start of same-session follow-up prompts whenever the pairs remain unqualified. Require a correctness-bearing source edit after each observed failure and before its repair submission, and ask the agent to stop unrelated work until the named pairs qualify.
- Include only attempt 38's safe counters and failure classes in v93's prompt.
- Add focused regressions for the v93 manifest and repair-feedback priority.
- Run one live attempt after offline checks under the still-active Muse/OpenCode Go authorization.

### Explicitly out of scope

- Daemon, scheduler, ACP event projection, job storage, provider adapter, or multi-model changes. Attempt 38 showed complete event and job-terminal observations.
- Hidden verifier changes or inspection, task-input changes, qualification-threshold changes, broader fixture redesign, or adding artificial duration.
- Any credential value in a prompt, test, artifact, transcript, or command output.

## 4. Required invariants

- Use only `opencode_go/muse-spark-1.3-contributor` for the live attempt.
- Preserve the v92 task inputs, public tests, hidden verifier identity, tools, and all manifest fields other than `group_id`, `title`, and `prompt`.
- Preserve the 30-minute minimum substantive active-work horizon, exact-ID overlapping background job, two named failure/repair pairs, continuation evidence, completed independent reviewer, and unchanged hidden verifier.
- Retain request, native-call, prompt-work, wall-clock, activity-gap, disk, cancellation, and USD-unavailable policies unchanged.
- Do not treat the broad `repair_cycles` count as a substitute for qualified named pairs.
- Keep safe evidence free of raw prompts, provider output, task-result bodies, transcripts, hidden-verifier output, job IDs, Task/Run IDs, and credential material.

## 5. Required changes

### v93 prompt

The bounded attempt history must state that attempt 38 reached the long active-work horizon and that its reviewer lifecycle completed correctly. It must make the repair failure unmistakable: both named seed jobs reached observed `failed` status, but each repair job was submitted immediately afterward with no intervening tool call/source edit; neither pair qualified. Tell the next agent to inspect the failed job's public result, make and verify a correctness-bearing source edit after that failure, then submit one repair job and immediately observe that exact ID. Do not repeat an unchanged test, claim a repair from documentation alone, or proceed to unrelated optional work while a named pair remains unqualified.

### Follow-up prompt priority

When the public trace still reports an unqualified named repair pair, prepend the repair instruction before stage, overlap, reviewer, or time-budget feedback. Preserve the rest of the existing continuation logic. Do not claim the fix is successful until exact-ID diagnostic predicates and the unchanged independent verifier agree.

## 6. Ordered work packages

### Work package A — Close S031 accurately

Record attempt 38 as completed unqualified. Close S031 conditionally for its implemented reviewer recovery and safe attribution; explicitly state that H007 did not qualify and transfer the task/repair failure to S032.

### Work package B — Prepare v93 and regression

Create v93 from v92, change only the allowed metadata and prompt, and assert stable manifest fields and prompt requirements. Add a test that incomplete repair feedback precedes unrelated implementation/reviewer instructions while preserving redaction.

### Work package C — Focused verification

Run the H007 and autonomy-run tests that cover the modified prompt and fixture, test-quality guards, Python compilation, and `git diff --check`.

### Work package D — Attempt 39

Recheck the exact OpenCode Go route and available credential without reading or printing the value, ARM64 binary/helper identity, request headroom, idle lane, and at least 30 GiB preflight free disk. Count the start before the first provider request. Monitor the 10-GiB runtime floor and stop immediately on cancellation or any authorized cap.

### Work package E — Safe diagnosis

Project only approved aggregates. If attempt 39 fails, identify whether the named edit-to-rerun sequence occurred, preserve the exact safe failure-domain counts, and create a narrower corrective only if new evidence requires it. Keep the campaign active while grant starts remain; do not claim qualification before every original criterion and the unchanged verifier pass.

## 7. Acceptance criteria

- v93 changes no stable task, verifier, tool, route, or budget field.
- Regression coverage proves failed repair-pair feedback is the first corrective instruction and includes the required post-failure edit ordering without identifiers.
- Focused checks pass.
- Attempt 39 records two independently qualified named repair pairs, all original task gates, exact-ID overlap, required horizon, completed reviewer lifecycle, and a passing unchanged hidden verifier, under the current caps.
- If it fails, its safe receipt, start consumption, and exact unqualified cause are reconciled before any next attempt.

## 8. Authorization and limits

Attempt 39 consumed the second start from the latest 40-start user grant. At closure, 38 starts remain in that grant and 120 total starts remain in the combined ledger. Use only Muse Spark 1.3 Contributor via `opencode_go`. Per-run caps remain 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, 300 seconds activity inactivity, 100 follow-ups, and a 10-GiB runtime disk floor with at least 30 GiB required before start. No USD ceiling is available; record billed USD as unknown if the route does not report it. Do not run concurrently.

## 9. Closure

Close S032 only when its implementation, focused verification, attempt-39 evidence, authorization ledger, S019 status, roadmap, and registry are reconciled. The S019 campaign remains unqualified until the unchanged hidden verifier and every original live criterion pass.


## 10. Offline verification completed

- v93 manifest: `dev/harness_lab/fixtures/agent_autonomy_s018_muse_continuation_v93/cases.jsonl`, SHA-256 `1d7914eec515c161a683b6ef8bc2464760638ee91c2871d6b953db6e1f2a086e`; prompt length 13,081 characters. Stable task, verifier, tool, route, and budget fields match v92.
- `python3 -m unittest dev.harness_lab.tests.test_h007`: 82 tests passed.
- `python3 -m unittest dev.harness_lab.tests.test_autonomy_run`: 64 tests passed.
- `python3 scripts/check_test_assertion_quality.py` and `python3 scripts/check_test_duplication.py`: passed.
- `python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/tests/test_h007.py`: passed.
- `git diff --check`: passed.

No Rust binary-affecting source changed. Attempt 39 still requires fresh route/credential availability, paired ARM64 binary/helper, disk, cap, and unused-lane preflight.


## 11. Attempt 39 result and corrective disposition

Attempt 39 used v93 and the authorized `opencode_go/muse-spark-1.3-contributor` route. It accumulated 2,734,803 ms of active work and passed the 30-minute duration and 300-second activity-gap gates. It did not qualify H007: the exact priority job lifecycle with productive overlap was absent, neither named repair pair qualified, required stages/gates and final workspace/Clippy jobs remained incomplete, and the unchanged verifier reported a setup error before recording tests run.

The ACP event trace was complete (150/150 tool start/terminal pairs, no event gaps or drops). Disk remained above the runtime floor. The agent made 150 native calls: 100 read-class, 14 shell, 13 tool-search, 7 job, 4 test, 4 write-class, 3 task, and 5 other calls. It submitted four test jobs; all reached terminal states (two failed, two completed), but no correctness-bearing edit separated either failed seed from its rerun. The public repair diagnostic remained 0/2.

The four prompt requests ended after about 637, 637, 625, and 900 seconds. The fourth hit the 900-second ACP `session/prompt` request deadline after 39 more native calls. The safe provider projection counted 97 attempts started, 96 succeeded, and zero failed/provider-timeout events; one attempt had no terminal marker when the outer prompt timed out. This is evidence of an unfinished attempt at cancellation, not proof of an upstream provider timeout. Disk, event transport, and job-terminal observation do not explain the failure.

S032 delivered and verified its prompt-priority change, but its live acceptance criterion failed. S033 owns the narrow follow-up: safe unfinished-provider-attempt accounting and a longer bounded prompt window for attempt 40. The H007 contract, task fixture, route, hidden verifier, and qualification thresholds remain unchanged.
