# Harness Lab Self-Improvement Resumption S057 — Post-Run Tool-Call Projection Order

Status: **closed at post-run projection correction scope; attempt 76 unqualified**

Repository baseline: `e3fce7b99f13380d9e36fafc49c844ccc646be69`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#51-s057--post-run-tool-call-projection-order`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`

Primary class: invariant

## 1. Objective

Ensure the autonomy runner collects post-run ACP tool calls before using them to compute H007's verifier-pause-adjusted qualification activity, so task/verifier finalization cannot fail with an unbound local.

## 2. Why this milestone is ready

Attempt 75's exact-head safe context reported `NameError` at `dev.harness_lab.autonomy_run:run_case` line 3729 and the outer `run_suite` call at line 4236. At the pinned source head, `qualification_tool_calls` reads local `tool_calls` before its later assignment at line 3746. This is a reproducible CodeGG ordering defect; the bounded exception-origin projection added by S056 supplied enough evidence to distinguish it from agent behavior.

## 3. Implementation and live evidence

At baseline `e3fce7b99`, `run_case` drained the ACP stream and cleaned up its watchers, then computed `qualification_tool_calls` before assigning `tool_calls = _observed_tool_calls(client)`. Attempt 75 exposed the resulting `NameError`, interrupting receipt/verifier processing before safe task/verifier projections existed. The pause-adjustment helper itself was present; the defect was the collection/use ordering.

Commit `a8c6e93a25` collects the raw projection exactly once through `_postrun_tool_call_projection` before H007 pause adjustment. The 85-test autonomy-run module passes. Exact-head attempt 76 produced a safe completion/verifier projection without a runner exception, confirming the corrected post-run path. The task itself remained unqualified: the agent timed out after 54.84 minutes active, only one of two repair pairs qualified, and the public continuation document was invalid. Closure: `plans/closure/harness-lab-self-improvement-resumption/057-status.md`.

## 4. Invariants that must not regress

- Preserve the v97 task, independent verifier, manifest, model route, per-run caps, and H007 qualification predicates.
- Collect one raw tool-call projection after stream drain/cleanup and use that same projection for raw receipt evidence and the pause-adjusted qualification clock.
- Subtract host verifier pauses only from qualification elapsed/tool timing; preserve raw transcript call timestamps.
- Keep exception text and private run material out of campaign evidence.
- A regression remains unqualified unless every existing task, duration, overlap, repair, continuation, review, and verifier gate passes.

## 5. Scope

### In scope

- Fix the post-run ordering in `dev/harness_lab/autonomy_run.py` so raw calls are collected before the qualification projection.
- Add a focused regression for the combined collection and H007 pause-adjustment path.
- Record attempt 75, close S056 at the telemetry scope it achieved, and perform attempt 76 under the unchanged authorization and controls.

### Explicitly out of scope

- Workload, prompt, verifier, route, budget, daemon, scheduler, ACP protocol, provider, or tool-surface changes.
- Interpreting attempt 75 as a completed task or failed verifier result; both projections are unavailable.

## 6. Required production changes

### Runtime

Introduce one post-run projection step that first calls `_observed_tool_calls(client)`, then derives H007's pause-adjusted calls from that returned list. Use the collected raw list for later transcript, repair, continuation, event, and receipt calculations. Remove the later duplicate/unsequenced collection.

### Documentation and evidence

Record attempt 75's safe `NameError` type and source locations, 209-request aggregate, timing/disk data, and unavailable task/verifier status. Update the authorization ledger, S019 status, roadmap, and registry. Close S056 and register S057 active.

## 7. Ordered work packages

### Work package A — Collect before projecting

Intent: Remove the unbound local error while preserving the existing separation between raw event timing and qualification timing.

Required changes: Collect `tool_calls` first and compute `qualification_tool_calls` from that value; ensure the same raw list continues into receipt and verifier code.

Acceptance evidence: A regression patches the observed-call source, invokes the production post-run projection helper, and proves it returns the raw list plus the correctly pause-adjusted H007 list. The 85-test autonomy-run module passes.

### Work package B — Reconcile attempt 75 and register the corrective

Intent: Carry the exact source diagnosis and authorization state into the plan registry.

Required changes: Add attempt 75's sanitized evidence, close S056, update S019/roadmap/registry, and preserve task/verifier outcomes as unavailable.

Acceptance evidence: The safe aggregate and campaign row agree at 209 requests; counters are 75/200 combined, 31/40 in the current grant, and 46/120 task starts, with 9 grant and 74 task starts remaining.

### Work package C — Offline verification and live reevaluation

Intent: Validate the ordering fix, then check it against the unchanged H007 workload.

Required changes: Run the focused autonomy-run tests, test-quality guards, Python compilation, and diff checks. Push the fix, verify the ARM64 pair remains exact for the unchanged Rust source, run fresh campaign preflight, and execute attempt 76 with the existing Muse grant.

Acceptance evidence: Offline checks pass; preflight matches the current head/task/verifier/route/limits; attempt 76 produces a safe terminal diagnostic. H007 qualifies only if all original gates pass.

## 8. Failure, cancellation, restart, and contention semantics

Do not change existing campaign accounting or controls. Reconcile positive requests once; unavailable counts remain fail-closed. Preserve the 300-second inactivity cancellation, 10-GiB runtime floor, 30-GiB preflight reserve, 1,200-request cap, 1,600 native-call cap, 3,300-second prompt-work cap, and 3,600-second hard stop.

## 9. Compatibility and migration

This is a local control-flow correction with no evidence or campaign schema migration.

## 10. Required tests

### Focused unit tests

- Production post-run call collection precedes H007 verifier-pause projection.
- Raw call timestamps remain unchanged; only qualification call timestamps remove verifier pauses.

### Integration and privacy tests

- The autonomy-run suite passes, and the safe exception projection continues to exclude messages, paths, and locals.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_autonomy_run
python3 -m py_compile dev/harness_lab/autonomy_run.py dev/harness_lab/tests/test_autonomy_run.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
```

## 12. Documentation updates

- S019 status, S019 roadmap, and `plans/registry.md`.
- S056 implementation and closure records; S057 closure after its gates are met.
- Attempt 75/76 safe preflight, diagnostic, and monitor evidence.

## 13. Acceptance criteria

- `run_case` never reads `tool_calls` before collection in the post-run path.
- Raw calls are collected once and reused for qualification, lifecycle, and verifier inputs.
- Pause-adjusted timing remains separate from original transcript timing.
- Focused verification and guards pass.
- Attempt 76 uses the unchanged route, task, verifier, and caps; qualify only on every original H007 gate.

## 14. Stop conditions

Stop before live admission for inconsistent authorization, unresolved request counts, mismatched source/binary/task identities, route or cap drift, or a free-disk reserve below 30 GiB. Stop during the run at the 10-GiB floor, 300-second inactivity limit, 1,200 requests, 1,600 native calls, 3,300 seconds prompt work, or 3,600-second hard stop.

## 15. Closure evidence required

Record source head, binary hashes, focused checks, attempt 75/76 safe evidence hashes, request aggregate reconciliation, safe task/verifier outcome, duration/activity gates, disk start/minimum/end, stop reason, and remaining authorization. Never infer completion or verifier pass from elapsed time.

## 16. Handoff notes

After attempt 76, the current grant has 8 starts remaining and task authorization has 73 remaining. Use only `opencode_go/muse-spark-1.3-contributor`; unreported USD remains `unknown`. Attempt 76 confirms the CodeGG post-run ordering correction but does not qualify H007. It showed no event gaps or unpaired tool calls; its task-level repair and continuation requirements were incomplete. The single safe `execution_backend_unavailable` submit class does not prove a daemon or scheduler defect. Preserve the unchanged task, verifier, route, and caps in the next authorized run.
