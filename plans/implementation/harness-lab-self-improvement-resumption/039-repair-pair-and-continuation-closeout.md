# Harness Lab Self-Improvement Resumption S039 — Repair-Pair and Continuation Closeout

Status: **conditionally closed at repair-pair and continuation-sequencing scope; H007 remains unqualified**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#33-s039--repair-pair-and-continuation-closeout`

Primary class: task calibration

## 1. Objective

Correct the remaining public task-sequencing defects reproduced by attempt 49: a seeded repair pair can reach terminal seed and repair jobs without a productive source change or correlated repair evidence, and the agent can reach all listed stages and gates but leave the continuation boundaries invalid until the prompt timeout. Keep the workload implementation, verifier, route, model, architecture, qualification criteria, and caps fixed.

## 2. Evidence

Attempt 49 lasted 3,287.013 seconds of active work and completed all seven public phases, twelve gates, three artifacts, exact-ID productive overlap, early reviewer lifecycle, Clippy, and workspace tests. The `catalog_store` pair qualified. The `catalog_format` seed failed and its repair job reached terminal, but the safe pair projection reported no intervening productive source change and no correlation. The continuation projection was invalid at the `turn_timeout`; its `next_phase`, `next_stage`, and `next_contract_phase` values were still present. The unchanged verifier passed 4/5 with one safe failure domain, `transcript_job_lifecycle`.

Attempt 49 had 309/309 ACP tool pairs, zero event gaps/drops, and zero unpaired calls at the run boundary. No process state is inferred. The reviewer and both required success jobs completed. This evidence isolates the next correction to the second repair's ordering and earlier continuation finalization. No provider, daemon, scheduler, ACP, job, disk, or request-limit failure was reproduced.

## 3. Invariants

- Use only `opencode_go/muse-spark-1.3-contributor` and the already authorized protected connection; never retain or reveal its credential.
- Keep the Rust workload, frozen inputs, public and hidden verifier bytes, required phases/gates/artifacts, exact-ID job contract, reviewer contract, active-work and activity-gap criteria unchanged.
- Keep the same request, native-call, prompt-work, wall-clock, inactivity, follow-up, disk-floor, reserve, and verifier limits.
- Preserve the 15-minute reviewer path, exact completed Run observation, and one matching Task fetch.
- Keep evidence body-free; no prompts, transcripts, provider text, command arguments, output bodies, paths, raw job/task IDs, hidden-verifier text, or credentials.
- A seeded failure only counts as repaired when an observed failure is followed by a productive source correction, a distinct named repair job, exact-ID running/terminal observations, and correlated continuation evidence.

## 4. Corrective change

Create v96 as a prompt-only derivative of v95, changing only `group_id`, `title`, and `prompt`. The new block uses attempt 49's safe public diagnostic to prioritize `catalog_format`: observe its seed terminal failure, make a substantive affected-source change after that failure and before submitting its repair validation job, observe the same repair ID running and terminal, and document the pair. It explicitly preserves the already-qualified `catalog_store` pair and its distinct evidence.

The prompt stages the closeout: maintain continuation evidence as work completes; submit Clippy and workspace tests after source/lockfile freeze, with workspace tests by 40 active minutes; by 45 minutes validate all phases, gates, artifacts, repair evidence, jobs, and reviewer fields and set all three next-boundary fields to `none`; then finish normally by 50 active minutes after all tools are terminal. This leaves time before the existing prompt-work timeout and verifier reserve without adding idle work or extending the horizon.

## 5. Offline verification

- Assert v96 differs from v95 only in `group_id`, `title`, and `prompt`; preserve the reviewer early-start behavior and all protected contract fields.
- Pin post-failure productive change, separate named repair pairs, explicit continuation boundaries, and the 50-minute normal closeout in the prompt regression.
- Run focused H007, autonomy-run, and verifier-diagnostic tests; assertion-quality and duplication guards; Python compilation; and `git diff --check`.
- Verify the S038 open-tool boundary projection preserves the `timeout` terminal, distinguishes configured disk floor from lowest observed free space, omits private content, and never infers underlying process state.

## 6. Live attempt 50

After all offline checks pass, use v96 for attempt 50, combined start 51, with the existing paired ARM64 binaries and the authorized Muse route. Preflight the exact manifest/verifier/binary/helper identities, connection availability, counters, and 30-GiB free-disk reserve. Keep the external 15-second disk monitor and the runner's 10-GiB floor, 1,200-request cap, 1,600-call cap, 3,300-second prompt-work limit, 3,600-second wall stop, 300-second inactivity cancellation, and existing follow-up limit unchanged. Run sequentially.

Authorization remaining before attempt 50: 34/40 starts in the latest supplemental grant and 150/200 combined starts, preserving one historical offset. Refused preflight does not start a provider run. Once admitted, charge a start on the first provider request and update the ledger without recomputing the offset from row count. External billed USD remains `unknown` if unavailable.

## 7. Acceptance and disposition

Attempt 50 qualifies only when the unchanged independent verifier passes and all existing H007 requirements pass: at least 30 minutes of active dependent development, all phases/gates/artifacts, exact-ID overlap, both productive correlated repair pairs, Clippy and workspace-test terminal success, public reviewer receipt, valid completed continuation with all next boundaries `none`, complete event pairing, activity-gap limit, and resource controls.

If it remains unqualified, use only the safe receipt and plan the next single observed defect. Do not inspect raw logs or hidden verifier material. Continue sequentially within the remaining authorization, stopping on qualification, exhaustion, or a hard stop.
