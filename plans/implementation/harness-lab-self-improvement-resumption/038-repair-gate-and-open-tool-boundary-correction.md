# Harness Lab Self-Improvement Resumption S038 — Repair Gate and Open-Tool Boundary Correction

Status: **conditionally closed at gate-scheduling and open-tool measurement scopes; H007 unqualified; S039 owns repair-pair and continuation closeout**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#32-s038--repair-gate-and-open-tool-boundary-correction`

Primary class: harness measurement and task calibration

## 1. Objective

Use attempt 48's body-free evidence to correct the next reproducible task failures without changing the Rust workload, hidden verifier, provider route, model, qualification gates, or per-run bounds. Make the second seeded repair and required workspace-test job explicit priorities, and make unmatched ACP starts at run termination directly observable without inferring process state.

## 2. Evidence and scope

Attempt 48 reached 3,288.055 seconds of active work, all seven public phases, all three artifacts, exact-ID productive overlap, and a completed early reviewer lifecycle. It remained unqualified: only one of two seeded repair pairs qualified; the `catalog_store` repair had no observed productive source correction or terminal result; Clippy completed but `workspace_tests` was not submitted; the public continuation omitted the reviewer receipt; and the unchanged verifier passed 3/5. Its drained ACP stream had zero reported gaps or drops but one `bash` start without a terminal event at the 3,300-second boundary. The safe projection identifies only tool class and timing; it does not prove whether a process was still active or whether a terminal notification was absent.

The reviewer start correction is confirmed and is closed at its own scope by S037. S038 is limited to the task's repair/gate sequencing and explicit, privacy-safe reporting of open ACP tool calls at the final run boundary. No daemon, scheduler, ACP protocol, provider, process, or test-runner architecture change is justified by attempt 48.

## 3. Invariants

- Live route remains only `opencode_go/muse-spark-1.3-contributor`.
- Keep the frozen fixture inputs, Rust task, public tests, hidden verifier and digest, tool set, exact-ID requirements, active-work threshold, independent reviewer, and all current acceptance criteria unchanged.
- Preserve the per-run limits: 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, 300 seconds inactivity cancellation, 100 follow-ups, 10-GiB runtime disk floor, 30-GiB preflight reserve, and the existing verifier reserve.
- Keep the reviewer on the existing 15-minute start path and retain exact Run-completed then one matching Task fetch ordering.
- Safe evidence may include allowlisted tool class/action, bounded elapsed times, counts, typed status and run terminal only. Never retain tool IDs, arguments, outputs, paths, provider content, prompts, transcripts, hidden-verifier content, or credential material.
- An unmatched ACP start is an observation about stream pairing only. Do not label the underlying process active, failed, cancelled, or lost without separate evidence.
- Preserve bounded run-terminal classes including `timeout`; do not substitute `unknown` when a known terminal class is missing from the allowlist.

## 4. Corrective changes

### Task sequencing

Create v95 as a prompt-only derivative of v94, changing only `group_id`, `title`, and `prompt`. The appended priority block directs the agent to finish the `catalog_store` seeded repair pair before optional acceptance work, preserve the required release-overlap work, start the reviewer at the stable checkpoint, and submit both required Clippy and workspace-test jobs through native managed background execution after source and lockfile edits are frozen. It requires the exact `workspace_tests` ID to reach a terminal state by 40 minutes of active work, and directs the agent to avoid long foreground Bash build/test operations and to close every started tool call before ending.

### Stop-boundary telemetry

Add an H007-only receipt projection of tool calls that have an ACP start but no terminal update when the run ends. Include a schema version, final run terminal, boundary elapsed time, tool class, allowlisted action, start time, and age when known. Explicitly report `underlying_process_state_inferred: false`. This supplements the existing gap/drop and ordered activity evidence without retaining raw identifiers or payloads.

## 5. Required tests and offline verification

- v95 differs from v94 only in the three permitted presentation fields and carries the reviewer early-start behavior forward.
- Prompt regression pins the store-pair priority, native background `clippy` and `workspace_tests`, submission deadline, and final tool-terminal audit.
- Production-shaped diagnostic test projects an unfinished Bash call at a run boundary, excludes a completed call, checks its bounded age/class, and proves command/output/path material is absent and process state is not inferred.
- Run `python3 -m unittest dev.harness_lab.tests.test_h007` and `python3 -m unittest dev.harness_lab.tests.test_verifier_diagnostics` plus the affected autonomy-run tests, assertion-quality and duplication guards, Python compilation, and `git diff --check`.

## 6. Live qualification

Use attempt 49 next, sequentially, with v95, the paired ARM64 CodeGG binaries, and the already authorized Muse/OpenCode Go route. The current authorization has 5/40 starts used and 35 remaining; combined accounting is 49/200 with 151 remaining, including one explicitly recorded historical start offset. Require exact manifest/binary/model/auth checks, 30-GiB preflight free disk, the live disk watcher, and the existing cancellation rules. Do not inspect raw private logs or verifier output.

If attempt 49 remains unqualified, consume only its safe receipt and create the next narrow corrective from observed task, lifecycle, measurement, provider, or resource evidence. Continue sequentially within the remaining 35 starts; stop only on qualification, exhaustion, or an existing hard stop. USD usage reported only by the provider is allowance metadata; separately billed cost stays `unknown` when unavailable.

## 7. Acceptance and closure

S038's offline work is complete when the prompt-only parity test, open-tool-boundary diagnostic regression, focused suites, quality guards, compilation, and diff check pass. Attempt 49 qualifies only if the unchanged independent verifier passes and every existing H007 gate passes, including at least 30 minutes of active dependent Rust work, all required phases/artifacts, exact-ID productive overlap, both seeded repairs, reviewer receipt, Clippy and workspace-test terminal success, complete tool-event measurement, activity-gap bound, and resource controls.

An unpaired tool at the final boundary remains a measurement failure, not an inferred process failure. If the new projection cannot distinguish the next observed terminal condition using safe data, add only the specifically missing bounded telemetry before another run.

## 8. Attempt 49 disposition

Attempt 49 confirmed the S038 task-priority improvements at their observed scopes: the `catalog_store` repair pair qualified; both required success jobs (`clippy` and `workspace_tests`) were submitted and observed completed; the reviewer lifecycle and public receipt passed; all seven stages, twelve gates, and three artifacts were present; and the final event stream was 309/309 paired with zero gaps or drops. The new boundary projection reported zero unpaired calls. The attempt lasted 3,287.013 seconds of active work and ended at the prompt-work timeout without an external hard stop.

H007 remained unqualified. `catalog_format` had a failed seed and terminal repair job but no productive source change between them and no correlated pair. The continuation document was invalid at timeout with the next phase/stage/contract boundaries still present. The unchanged verifier passed 4/5, with one safe `transcript_job_lifecycle` failure. S039 owns the narrowly evidenced per-pair correction and earlier continuation closeout.

The attempt's first safe projection mapped the bounded run terminal `timeout` to `unknown`. An offline correction added `timeout` to the allowlist and a regression. Its disk summary also used the configured threshold rather than `lowest_observed_free_bytes`; the corrected minimum was 22,995,116,032 bytes and matched the external watcher. The 10-GiB runtime floor did not trigger. No raw prompt, command, transcript, verifier output, or runner log was inspected.

Attempt 49 used start number 50: 6/40 starts are consumed in grant `s019-supplemental-20261111-04` and 50/200 combined starts are consumed, with 34 grant starts and 150 combined starts remaining. The provider allowance receipt reported `$0.287164`; externally billed USD remains `unknown`. Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt49-safe-diagnostic-20261111.json` (mode 0600).
