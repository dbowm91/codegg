# Harness Lab Self-Improvement Resumption Milestone S030 — Prompt Window Sequencing and Diagnostic Telemetry

Status: **implemented; attempt 37 completed unqualified; S031 owns the remaining reviewer correction**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052`, with S029 and attempt 36 in the shared worktree.

Source subsystem roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#24-s030--prompt-window-sequencing-and-diagnostic-telemetry`

Primary class: **capability**

## 1. Objective

Use attempt 36's safe receipt to improve the sustained task's completion probability and make the stop boundary attributable. Produce v91 as a prompt-only derivative that folds clean relocated-workspace checks into the existing `workspace_qualification` stage, prioritizes required work before optional probes, and removes stale attempt history that competes with the current instructions. Add bounded, content-free telemetry for each H007 ACP prompt boundary so future receipts show when the 30-minute reviewer marker was sent relative to active work and whether a prompt ended, timed out, or failed.

The task, hidden verifier, model route, tool surface, active-duration rule, repair sequences, exact-ID overlap, reviewer requirement, and resource ceilings remain unchanged. Do not modify production daemon, scheduler, ACP, provider, or job semantics.

## 2. Attempt 36 evidence and diagnosis

Attempt 36 used v90 on `opencode_go/muse-spark-1.3-contributor`. Its safe receipt reports:

- 54.91 minutes of active work, meeting the 30-minute horizon; exact-ID productive overlap and both repair pairs qualified.
- 246/1,200 provider requests, no failed provider attempts, no inactivity cancellation, and no provider-timeout event.
- 305/305 native tool starts had terminal events; the trace had zero gaps and zero drops. Maximum native-event gap was 192,342 ms.
- The task ended `timeout` with `turn_timeout`, not a normal successful end. The final request was allowed 247.827 seconds because 547.827 seconds remained and 300 seconds were reserved for verification; it ran 247,832 ms and timed out at that bound.
- The public continuation remained invalid. `workspace_qualification` and `independent_review` were incomplete; the required Clippy job was observed failed, the required workspace-test job was never submitted, and the reviewer remained running. The verifier ran five checks and passed three; safe failure domains were `continuation_boundary` and `transcript_job_lifecycle`.
- The reviewer spawned once, but no reviewer-due follow-up marker was recorded. The current receipt has only aggregate prompt-timeout data, so it cannot show the active-work span at each successful prompt boundary or whether the due marker was sent before the reviewer spawn.
- The external disk monitor's low-water mark was 96.92 GiB, above the 10-GiB stop floor.

The run did not stop early and did not exhaust its request allowance. It used nearly the full prompt-work window, then the request-time bound preserved the verifier reserve. Current evidence points to incomplete task sequencing and missing prompt-boundary observability; it does not establish provider, ACP event-stream, scheduler, or daemon failure. The precise cause of the failed Clippy job is not available in the safe projection and must not be guessed from private output.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt36-safe-diagnostic-20261110.json`.

## 3. Invariants

- Use only `opencode_go/muse-spark-1.3-contributor` and the existing paired ARM64 binary/helper.
- Keep the frozen verifier SHA, task acceptance, source/reference inputs, and H007 gates unchanged.
- Count a start before the first provider request. Attempt 36 consumed 14/40 supplemental and 37/120 combined starts; 26 supplemental and 83 combined starts remain.
- Preserve the 1,200 provider-request, 1,600 native-call, 3,600-second wall, 3,300-second prompt-work, 300-second inactivity, 10-GiB free-disk floor, and 100-follow-up caps. Preflight still requires 30 GiB free.
- Billed USD remains `unknown` unless the provider reports it.
- Do not inspect or retain hidden verifier source/output, assertion text, test identities, transcripts, raw job IDs/arguments, credentials, or protected runner output.
- New prompt telemetry contains only bounded counters, elapsed times, allowlisted stop classes, and booleans. It retains no prompt text, provider text, exception text, task content, IDs, or arguments.
- Local harness probes and findings stay in git-ignored `agentfindings.md`; never commit it.

## 4. Implementation

1. Create v91 from the latest accepted v89 task prompt, preserving all non-prompt manifest fields and the exact verifier identity. Carry forward only current relevant context: attempt 35's completed public stages with verifier setup failure, and attempt 36's timeout/incomplete public gates. Remove obsolete nested attempt narratives.
2. Require `workspace_qualification` to perform locked metadata, workspace-test, and Clippy checks from a clean relocated source/lockfile copy, excluding `target/`. These are the existing qualification gate, not an extra post-task phase. Keep each required operation observable by exact native job ID; on failure, repair the source, recreate the copy, and rerun the failed operation. Clean up the copy and confirm the source lockfile is unchanged.
3. Clarify stage priority: complete the dependent implementation and seeded repair pairs first; schedule required workspace checks as part of the existing final stage, not after declaring the task complete; perform bounded probes only after required implementation/acceptance work; start exactly one reviewer at the specified 30-minute active-work follow-up and continue other required work while it runs; record the five-line review receipt only after exact completed Run status and one matching Task fetch.
4. Add a bounded H007 prompt-boundary timeline to the private execution receipt. For each request, record request ordinal, elapsed and remaining time at start, request timeout and elapsed time, active-work span and native-call count at the boundary, an allowlisted stop class, whether the current prompt contained the reviewer-due marker, and whether the next follow-up included it. Do not persist prompt text or provider responses.
5. Keep the existing safe aggregate projection compatible; add only an allowlisted projection for this new timeline and regression coverage proving arbitrary stop/error text cannot enter it.

## 5. Acceptance criteria

- v91 changes only `group_id`, `title`, and `prompt` from v89; the verifier, task inputs, tool list, route, and caps remain identical.
- The prompt makes relocated builds part of `workspace_qualification`, names every existing H007 phase/gate consistently, and contains no obsolete attempt 33/34 sequencing blocks.
- Prompt-boundary diagnostics record completed and timed-out requests with consistent nonnegative timing, bounded counts, and no raw prompt/provider/exception strings.
- A synthetic boundary sequence crossing 30 minutes proves the receipt distinguishes a reviewer-due marker sent from one never sent, without storing the marker text itself.
- Focused H007, autonomy-run, diagnostic, test-assertion-quality, and test-duplication checks pass; Python compilation and `git diff --check` pass.
- Attempt 37 is qualified only if the unchanged hidden verifier passes and all original H007 public gates, active-work, overlap, repair, reviewer, and resource criteria pass. A task timeout, incomplete gate, verifier failure, or missing receipt remains unqualified.

## 6. Attempt 37 authorization and preflight

Attempt 37 passed preflight and consumed start 15/40 of the then-current supplemental grant and start 38/120 of the then-current combined ceiling before provider invocation. The configured OpenCode Go entry was available in a mode-0600 store; v91, verifier, ARM64 binary/helper identities, route, request policy, and unused private lane matched. Preflight free disk was 91.85 GiB.

Stop on user cancellation, the 10-GiB disk floor, 300-second inactivity, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, 100 follow-ups, missing authorization, or identity mismatch. Do not claim task completion from active duration or public checks alone.

## 7. Focused verification

```bash
rtk python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_h007_attribution dev.harness_lab.tests.test_verifier_diagnostics
rtk python3 scripts/check_test_assertion_quality.py
rtk python3 scripts/check_test_duplication.py
rtk python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/tests/test_autonomy_run.py dev/harness_lab/tests/test_h007.py
rtk git diff --check
```

## 8. Offline verification result

Focused verification passed: 182 autonomy-run, H007, attribution, and verifier-diagnostic tests; both test assertion-quality and duplication guards; Python compilation; and `git diff --check`. One existing `ResourceWarning` about a SQLite connection appeared during the suite; all tests passed. v91 SHA-256 is `9e2052da9c1e45dbc5eebd136c5a290e852c1248c0f77ec87399624071c4524d`; its parent v89 SHA-256 is `48ede91e68a33949f5e4f50874cb4451d89da11ccad90240206431a1885b274d`.

## 9. Evidence

Record the v91/v89 manifest hashes, verifier and binary identities, attempt counters, safe receipt digest, route, task/verifier/horizon results separately, relocated-check status, prompt-boundary aggregate, reviewer and repair summaries, provider attempts, disk minimum, timeout class, and billed USD as `unknown` if unavailable. Never record credentials, hidden test identities/assertions, raw logs, transcripts, task result bodies, raw IDs/arguments, or verifier output.

## 10. Attempt 37 live result

Attempt 37 completed after 3,299.686 seconds of task runtime, including 3,285.576 seconds of active work. The duration and exact-ID productive-overlap gates passed; both repair pairs were qualified; all required stages completed; and the relocated metadata, workspace-test, and Clippy jobs completed. The independent reviewer did not: its single Run and matching Task ended `failed` with the allowlisted class `deadline_exceeded` after 1,205,086 ms. The agent made 13 status calls, five waits, and one get, but no `continue` action; no completed reviewer result was observed. The public continuation remained invalid and the independent verifier passed 3/5, classifying `continuation_boundary` and `transcript_job_lifecycle` as incomplete. The parent subsequently reached `turn_timeout` while preserving the 300-second verifier reserve.

The request count was 250/1,200 with no provider-timeout event. All 313 native tool starts had terminal events; there were zero explicit gaps, dropped events, or unpaired starts, and the maximum native-event gap was 97,255 ms. The disk watcher sampled 455 times, recorded a 69.56-GiB low-water mark, and did not trigger. Billed USD was unavailable and is recorded as `unknown`. Safe evidence and receipt digest are in `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt37-safe-diagnostic-20261110.json`.

Closure: `plans/closure/harness-lab-self-improvement-resumption/030-status.md` marks S030 conditionally closed for its implementation and offline diagnostic objective. H007 qualification remains outstanding; S031 owns the same-run reviewer recovery and next live attempt.
