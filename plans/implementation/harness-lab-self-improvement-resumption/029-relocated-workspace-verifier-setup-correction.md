# Harness Lab Self-Improvement Resumption Milestone S029 — Relocated Workspace Verifier Setup Correction

Status: **implemented; attempt 36 completed unqualified; sequencing correction carried by S030**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` with attempt 35 safe evidence and S028's v89 prompt correction in the shared worktree.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#23-s029--relocated-workspace-verifier-setup-correction`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None; this is a bounded qualification correction.

Primary class: **capability**

## 1. Objective

Use attempt 35's safe evidence to determine whether the final Rust workspace builds and tests correctly from a clean relocated copy, then make one bounded Muse attempt with that check explicit. Keep the task, hidden verifier, route, active-duration requirement, job lifecycle, repair pairs, reviewer, and resource caps unchanged. Do not change the daemon, scheduler, ACP, provider integration, or hidden verifier.

## 2. Evidence and diagnosis

Attempt 35 used v89 on `opencode_go/muse-spark-1.3-contributor`. The agent ended normally after 1,894,613 ms active work (31.58 minutes). The public projection reported all stages and gates complete, both repair pairs qualified, five correlated repair cycles, 47/47 submitted test jobs terminal, exact-ID productive overlap, and the exact five-line reviewer receipt. Native event pairing was complete (784 starts and 784 terminals), with zero explicit gaps or drops. Provider requests were 694/1,200, there was no provider timeout or inactivity stop, and the disk monitor's low-water mark was 104.45 GiB.

Task outcome is **inconclusive**, not passed. The unchanged independent verifier ran zero tests and the safe diagnostic classified setup as `workspace_build_failure`. The exact compiler output, test identity, hidden assertion text, and verifier source remain uninspected. Provider, event, job-lifecycle, disk, and activity-stall evidence does not point to a daemon or ACP failure.

Inference to test: a candidate workspace may pass its in-place public checks but fail when the independent verifier prepares or builds it from a fresh path. A clean relocation check can test that hypothesis without access to the verifier. If it passes, preserve the setup failure as unresolved and do not claim qualification.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt35-safe-diagnostic-20261110.json`.

## 3. Invariants

- Use only `opencode_go/muse-spark-1.3-contributor` and the existing paired ARM64 binary/helper.
- Preserve the exact frozen task and verifier identities, task acceptance contract, and all existing H007 gates.
- Require at least 30 minutes of substantive active Rust development, both distinct repair pairs, exact-ID productive job overlap, all job terminal observations, one completed read-only reviewer with a valid five-line receipt, and all unchanged verifier checks passing.
- The relocation check copies only the completed public workspace sources and lockfile to a fresh temporary path. Exclude build outputs and remove the copy and its target directory after recording results.
- Do not invoke, read, copy, or infer hidden verifier source, test identities, assertions, or output.
- Keep the 1,200 provider-request cap, 1,600 native-call cap, 3,600-second wall cap, 3,300-second prompt-work cap, 300-second verifier reserve, 300-second inactivity cancellation, 100 same-session follow-up cap, 10-GiB disk floor, and 30-GiB preflight reserve.
- Billed USD remains `unknown` when the route does not report it.

## 4. Scope

### In scope

- Create v90 as a prompt/group/title-only derivative of v89.
- State attempt 35's safe outcome without implying qualification: the public task completed after 31.58 active minutes, but the unchanged verifier ran zero tests after a setup error classified as `workspace_build_failure`.
- Require a fresh relocated copy of the final workspace; run locked Cargo metadata, workspace tests across all targets, and all-target Clippy against that copy. Record the terminal exit status for each exact-ID job before marking its qualification gate complete.
- Remove the temporary copy and target after the checks; verify the source workspace and lockfile remain intact.
- Preserve native tools, one read-only task reviewer, exact-ID background overlap, two repair cycles, existing bounded failure probes, and the 300-second final closeout reserve.
- Add a prompt-only fixture regression; run the focused S019 tests, quality guards, Python compilation, and `git diff --check`.
- Run attempt 36 under the existing authorization and record safe evidence.

### Out of scope

- Changing the task fixture's acceptance criteria, frozen inputs, verifier, qualification gates, provider route, or time/request/resource limits.
- Reading hidden verifier files, test IDs, assertion text, traceback, task transcript, or raw tool/job output.
- Reopening daemon, scheduler, ACP projection architecture, multi-provider integration, or general harness telemetry absent new direct evidence.

## 5. v90 task instructions

Build v90 from v89. Tell the agent to complete the Rust task, then validate the final workspace from a fresh relocated copy using these exact command classes:

```text
cargo metadata --locked --no-deps --format-version 1
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
```

The temporary copy must exclude `target/`, preserve every public manifest and `Cargo.lock`, use the repository's approved offline dependencies, and be removed after the checks. The final checkpoint must record successful exact-ID terminal results, not only terminal job states. If any check fails, repair the public source, recreate the clean copy, and rerun the failed check. Do not claim that this predicts or reproduces a hidden verifier assertion; it only validates the public workspace across relocation.

## 6. Acceptance criteria

- v90 differs from v89 only in `group_id`, `title`, and `prompt`; verifier hash, task, tool list, scope, and caps are unchanged.
- Prompt names the safe attempt 35 result and states that task correctness remains inconclusive.
- The relocated copy is built/tested with the locked commands above and deleted after results are recorded.
- Focused H007/autonomy/diagnostic tests, test assertion-quality and duplication guards, Python compilation, and `git diff --check` pass.
- Attempt 36 is qualified only if the task passes, the unchanged verifier runs and passes every check, and every sustained-horizon criterion passes. A repeated verifier setup error remains unqualified and becomes evidence for a narrow next owner.

## 7. Attempt 36 authorization and preflight

Attempt 35 consumed 13/40 supplemental starts and 36/120 combined starts. Attempt 36 consumes one start before provider invocation, leaving 26 supplemental and 83 combined starts. Before the start, verify the OpenCode Go auth entry exists without printing its value, v90 and verifier identities, matching ARM64 binaries, unused lane/receipt, admitted request budget, and at least 30 GiB free disk. Use a mode-700 private lane and inspect only a safe aggregate receipt and disk monitor.

Stop on user cancellation, 10-GiB disk floor, 300-second inactivity, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, 100 follow-ups, missing authorization, or identity mismatch. Do not claim success from public checks alone.

## 8. Verification commands

```bash
rtk python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics
rtk python3 -m unittest dev.harness_lab.tests.test_autonomy
rtk python3 scripts/check_test_assertion_quality.py
rtk python3 scripts/check_test_duplication.py
rtk python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/diagnostics.py dev/harness_lab/tests/test_h007.py
rtk git diff --check
```

## 9. Offline verification result

The v90 prompt-only regression and focused H007/autonomy/diagnostic suite passed (166 tests); the autonomy suite passed (52 tests). Test assertion-quality and duplication guards, Python compilation, and `git diff --check` passed. v90 SHA-256 is `71d2fa63acf3492358509975c6aaa80aa5a15a3929cdf6b5d0105afff82d9fa5`; parent v89 SHA-256 is `48ede91e68a33949f5e4f50874cb4451d89da11ccad90240206431a1885b274d`.

## 10. Evidence

Record v90/v89 manifest hashes, verifier and paired binary identities, preflight and authorization counters, safe receipt digest, route, task/verifier/horizon results separately, relocated check outcomes, gate and reviewer aggregates, request/attempt/event/job counts, disk minimum, timeout classification, and billed USD as `unknown` if unreported. Never record credential values, raw logs, transcripts, task result bodies, hidden test IDs/assertions, raw job IDs, tool arguments, or raw verifier output.
