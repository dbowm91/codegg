# Harness Lab Self-Improvement Resumption Milestone S028 — Public Gate Pacing and Review Receipt

Status: **implemented; attempt 35 completed unqualified; S029 active**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` with S027's offline corrective and attempt 34 evidence in the shared worktree.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#22-s028--public-gate-pacing-and-review-receipt`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None; this is a bounded task-sequencing correction.

Primary class: **capability**

## 1. Objective

Use attempt 34's safe public evidence to complete the remaining H007 gates in one bounded Muse run. Keep the Rust workload, independent verifier, horizon, request and resource limits, and evidence contract unchanged. Create v89 as a prompt-only derivative of v88 that makes the mandatory gate order and reviewer closeout sequence explicit.

## 2. Why this milestone is ready

Attempt 34 shows that S027 corrected the repair-order feedback: both `catalog_format` and `catalog_store` qualified, four correlated repair cycles matched the continuation record, the exact-ID overlap completed, and all 11 submitted test jobs reached terminal status. The event trace was complete with 245/245 native calls paired, no event gaps or dropped events, and a 148,982 ms maximum native-event gap. Active work lasted 3,286,586 ms (54.78 minutes); free disk never fell below 114.19 GiB. The run used 199/1,200 provider requests, had no provider timeout or stall-stop event, and stopped at the planned 3,300-second prompt-work boundary with a 300-second verifier reserve.

The task did not qualify. The public continuation had two incomplete gates: `bounded_probes` and `independent_review`. The one read-only reviewer actually completed and its result was fetched once, but the agent did not write the five required review fields to `CONTINUATION.md`. Safe public reconciliation classifies the sole verifier failure as incomplete public lifecycle evidence; no hidden test identity, assertion, or output was used. The task was still incomplete at timeout, so this is a task-pacing and checkpoint-recording failure, not evidence of a provider, daemon, scheduler, ACP, disk, or event-loss defect.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt34-safe-diagnostic-20261110.json`.

## 3. S027 result and boundaries

- The S027 ID-free pair feedback worked: both required pairs qualified in attempt 34.
- The timeout receipt now reports `turn_timeout` consistently and records bounded timing; the previous stale `end_turn` mismatch is corrected.
- The native reviewer lifecycle completed, but its required durable continuation receipt was absent. Preserve this distinction in all reporting.
- No new provider, daemon, scheduler, ACP, multi-provider, or telemetry architecture is indicated.
- Do not lower the 300-second verifier reserve or relax any qualification criterion.

## 4. Invariants

- Use only `opencode_go/muse-spark-1.3-contributor` and the existing paired ARM64 binary/helper.
- Keep the unchanged frozen Rust task and independent verifier identities.
- Retain the explicit 1,200-request ceiling, 1,600 native-call limit, 3,600-second wall cap, 3,300-second prompt-work cap, 300-second verifier reserve, 100-follow-up limit, 300-second inactivity cancellation, and 10-GiB disk floor.
- Require at least 30 minutes of substantive active work, both distinct repair pairs, exact-ID productive overlap, all required gates, terminal job observations, one terminal read-only reviewer receipt, and all five unchanged verifier checks.
- Do not reuse job or reviewer IDs from attempt 34; every new lane must produce its own evidence.
- Keep hidden verifier source, test identities, assertion text, raw output, task result bodies, transcript contents, credential values, raw IDs, and tool arguments out of plans and safe evidence.
- Use native CodeGG tools, one read-only task subagent, a small helper when useful, and only the documented reversible failure probes.

## 5. Scope

### In scope

- Create v89 as a prompt/group/title-only derivative of v88.
- State attempt 34's safe result: both repair pairs qualified; `bounded_probes` and the durable `independent_review` checkpoint remained incomplete; the reviewer child completed, but its result was not written into the continuation receipt.
- Direct the agent to finish mandatory stages, bounded probes, workspace qualification, and Clippy before the 2,700-second active-work cutoff; keep optional work out of closeout.
- After the one reviewer result is retrieved, immediately write the five required reviewer fields, read the checkpoint back, and do not invoke the reviewer again.
- Add a regression for v89's prompt-only identity and gate/reviewer closeout requirements.
- Run focused S019 H007/autonomy/diagnostic tests, quality guards, compilation, and `git diff --check`.
- Run attempt 35 under the existing authorization and record its safe outcome.

### Out of scope

- Changing the task fixture, hidden verifier, qualification criteria, request ceiling, timeout policy, binary, daemon, scheduler, ACP transport, provider integration, or tool authority.
- Repeating already-qualified repair pairs beyond their required evidence, adding a second broad sequence audit, or adding artificial delays.
- Interpreting safe diagnostics as hidden assertion details.

## 6. Acceptance criteria

- v89 differs from v88 only in `group_id`, `title`, and `prompt`, remains below the prompt size cap, and retains the exact task/verifier and non-prompt manifest fields.
- The prompt makes the mandatory gate deadline and one-shot reviewer receipt sequence explicit without disclosing hidden verifier information or changing the task contract.
- Focused tests, assertion-quality and duplication guards, Python compilation, and `git diff --check` pass.
- Attempt 35 is reported as qualified only when the task completed, all five unchanged verifier checks pass, and every horizon gate passes. Otherwise preserve its safe evidence, identify the next narrow owner, and consume its start.

## 7. Attempt 35 preflight and run limits

- Start only after verifying the temporary OpenCode Go auth entry is available without printing its value, the v89 and unchanged verifier identities match, the paired ARM64 binaries match, the new private lane and receipt are absent, request budget is admitted, and at least 30 GiB are free.
- Count attempt 35 before provider invocation. It is start 13/40 of the supplemental grant and 36/120 combined, leaving 27 supplemental and 84 combined starts.
- Stop on user cancellation, the 10-GiB disk floor, 300-second inactivity, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, 100 follow-ups, missing authorization, or identity mismatch.
- Keep output and logs in a mode-700 private directory; inspect only the safe receipt projection and aggregate disk monitor.

## 8. Verification commands

```bash
rtk python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics
rtk python3 -m unittest dev.harness_lab.tests.test_autonomy
rtk python3 scripts/check_test_assertion_quality.py
rtk python3 scripts/check_test_duplication.py
rtk python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/diagnostics.py dev/harness_lab/tests/test_h007.py
rtk git diff --check
```

Pre-attempt-35 offline results: focused H007/autonomy/diagnostic suites passed (165 tests), the autonomy suite passed (52 tests), test assertion-quality and duplication guards passed, Python compilation passed, and `git diff --check` passed. v89 manifest SHA-256 is `48ede91e68a33949f5e4f50874cb4451d89da11ccad90240206431a1885b274d`; parent v88 SHA-256 is `57bf7b463f14e2b2c6f6aedf693b74e207bc88699143dd9df817135561d77113`.

## 9. Evidence

Record the v89/v88 manifest hashes, unchanged verifier and binary identities, preflight and authorization counters, safe receipt digest, route, task/verifier/horizon outcomes separately, gate and reviewer lifecycle aggregates, provider request/attempt counts, event counts/gaps, disk minimum, timeout classification, and billed USD as `unknown` if unreported. Never store raw logs, transcript, task result bodies, hidden test IDs/assertions, credentials, raw job IDs, or tool arguments.


## 10. Attempt 35 result

The prompt-only correction worked for its stated sequencing gap: the public projection shows all gates and stages complete, including `bounded_probes` and the exact five-line independent-review receipt. Both repair pairs, exact-ID productive overlap, all 47 job terminals, and the reviewer lifecycle completed. The run lasted 31.58 active minutes. However, the task outcome is inconclusive because the independent verifier failed during setup with safe category `workspace_build_failure` and ran zero tests. No hidden output or assertion details were inspected. S029 owns the narrow next corrective: validate the finished workspace from a clean relocated copy and complete final locked build/test/Clippy checks before checkpointing. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt35-safe-diagnostic-20261110.json`.
