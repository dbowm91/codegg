# Harness Lab Self-Improvement Resumption S058 — Failed Job-Status Identity and Error Typing

Status: **active**

Repository baseline: `36f081a5b08917f78879848e1bfceb0196a85e82`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#53-s058--failed-job-status-identity-and-error-typing`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`

Primary class: invariant

## 1. Objective

Keep failed `job.status` calls bound to the exact requested job ID and preserve the distinction between an ordinary job lookup/execution error and a true unavailable backend, without changing H007's task, verifier, route, or qualification gates.

## 2. Why this milestone is ready

Attempt 78's safe diagnostic records one failed `job.status` call whose request contained a string job ID but whose result had no job ID. Twenty-three of twenty-four applicable request/result ID correlations matched; the remaining call was unobserved and counted as unbound. The safe error class was `execution_backend_unavailable`, but that label does not identify the underlying cause.

At baseline `36f081a5b`, `JobTool::execute_structured` propagates `ToolError::Execution` with `?`, so an error result omits the requested ID. The broker's generic fallback maps non-specialized `ToolError` values to `InfrastructureError`, and the agent tool batch maps that status to `ToolError::BackendUnavailable`. Consequently a job-not-found or store execution error can be presented and safely classified as backend unavailable. This source path is reproducible; attempt 78's exact underlying error remains unknown.

## 3. Current implementation evidence

- `src/tool/job.rs`: status lookup maps store errors and a missing job to `ToolError::Execution`; `execute_structured` currently returns early on that error and does not include the supplied ID.
- `src/tool/broker.rs`: the terminal fallback groups generic errors into `ToolValue::infrastructure_error`.
- `src/agent/tool_batch.rs`: `ToolTerminalStatus::InfrastructureError` becomes `ToolError::BackendUnavailable`; `ToolTerminalStatus::Error` remains an execution error.
- `dev/harness_lab/autonomy_run.py`: ACP start events already retain a safe digest of the requested ID, and the public tool projection can correlate it when the result includes the same ID.
- Attempt 78: all 231 tool starts had terminals, with zero event gaps; both repair pairs and all public continuation checks passed. The agent timed out without the required review, and the unchanged verifier reported failures in two public domains. Those task/verifier outcomes are preserved separately from this corrective.

## 4. Invariants that must not regress

- Exact job ownership is still checked before job details are returned.
- A failed status lookup never fabricates a job state or terminal success.
- The requested ID may appear in the tool response only when it is a bounded, valid UUID supplied in that request; campaign evidence continues to retain no raw ID.
- `ToolError::BackendUnavailable` remains a backend failure; ordinary `ToolError::Execution` stays an execution failure.
- H007 task, verifier, task instructions, route, caps, and acceptance predicates remain frozen.
- A status failure does not trigger an automatic retry or a replacement job submission.

## 5. Scope

### In scope

- Preserve a valid requested UUID in the failed `job.status` response when `JobTool` returns `ToolError::Execution`.
- Keep the structured result unsuccessful and omit any job-state claim when lookup failed.
- Classify the stable `job not found` message correctly when the response also contains the ID line.
- Add a production-shaped ACP start/failure regression proving that the failed call is associated with its request ID without retaining the raw ID in public diagnostics.

### Explicitly out of scope

- Changing task sequencing, review behavior, continuation criteria, the independent verifier, or prompt text.
- Changing daemon, scheduler, ACP protocol, provider integration, admission controls, or live-run budgets.
- Inspecting hidden verifier source/output, raw tool arguments/results, receipts, transcripts, or credentials.
- Treating attempt 78's safe error class as proof of a daemon or scheduler outage.

## 6. Required production changes

### Runtime and tool surface

In `JobTool::execute_structured`, handle only `ToolError::Execution` from a `status` request with a valid UUID argument by returning a bounded unsuccessful `StructuredToolResult`. Its output includes the bounded error message and a `job_id: <same UUID>` line so the ACP projection can correlate the failed call. It must not include the job's owner, state, or other stored data on a failed lookup. Propagate permission, timeout, and any `BackendUnavailable` error through their existing typed paths. Leave other actions and malformed IDs on their current error path.

The failed structured result must remain `success: false`; it must not be represented as a completed, failed, or otherwise terminal **job state**. The response only establishes that the status operation failed for the requested ID.

### Diagnostics and safe projection

Update the fixed job-status error classifier to inspect the first detail line after the standard tool-error prefix, so `job not found` remains classifiable when the bounded ID line follows it. Keep only fixed error classes in safe evidence. The existing request digest and result-ID comparison should report a matched request/result ID for a failed status response that echoes its own requested UUID.

### Documentation and static guards

Record the corrective and attempt 78 evidence in the S019 closure and subsystem roadmap. Do not retain raw job IDs, error strings, tool payloads, or private transcript material in campaign evidence.

## 7. Ordered work packages

### Work package A — Preserve identity on failed status lookup

Intent: Let the model and exact-ID projection identify which requested job a failed status lookup concerned.

Required changes: Add the bounded failed structured-result path for a valid UUID `job.status` request and preserve the original unsuccessful outcome.

Acceptance evidence: A Rust regression proves the output includes the exact requested ID, remains unsuccessful, does not claim a job state, and retains the original error category needed by the caller.

### Work package B — Keep error classes precise and test the ACP trajectory

Intent: Prevent an ordinary execution failure from being reported as a backend outage and prove the error response binds to its request.

Required changes: Update the safe error classifier for an ID line following the stable message; add a production-shaped failed-status ACP event sequence that runs through the public projection and asserts a matched ID, no unbound job call, no raw ID retained, and the appropriate fixed error class.

Acceptance evidence: The Python regression distinguishes `job_not_found` from a true `execution_backend_unavailable` error and verifies that the public safe summary contains no raw ID or error text.

### Work package C — Verify, build the paired binary, and requalify

Intent: Confirm the correction locally, then measure it against the unchanged live workload.

Required changes: Run the focused Rust job-tool tests, focused Python autonomy/diagnostic tests, test-quality guards, formatting, and diff checks. Rebuild the ARM64 CodeGG/helper pair from the corrected source, verify pair hashes and Rosetta probe, pass a fresh exact-head preflight, then run attempt 79 under the existing Muse authorization and unchanged limits.

Acceptance evidence: The offline regressions pass; the new preflight binds the exact source and binary hashes; the safe post-run aggregate matches campaign admission. H007 qualifies only if every original gate and the independent verifier pass.

## 8. Failure, cancellation, restart, and contention semantics

The correction performs no new job operation and does not retry a failed status query. If a valid requested UUID is known, the error response echoes that exact request ID and the bounded error detail only. A true backend-unavailable error, timeout, cancellation, permission denial, or malformed request continues to follow its existing typed path. The existing 300-second inactivity cancellation, 10-GiB runtime disk floor, 30-GiB preflight reserve, 1,200-request cap, 1,600-native-call cap, 3,300-second prompt-work limit, 3,600-second hard stop, and verifier reserve remain unchanged.

## 9. Compatibility and migration

No schema, database, public argument, or wire-protocol migration is required. Successful job status results remain unchanged. Failed valid-UUID status results gain an explicit `job_id` line and remain failures.

## 10. Required tests

### Focused unit tests

- A failed status lookup for a valid UUID retains that UUID in output, keeps `success: false`, and does not claim a job state.
- A malformed or absent ID does not enter the echo path.
- A backend-unavailable error remains typed as a backend failure.

### Integration tests

- A production-shaped ACP start plus failed status result correlates the requested ID to the response, produces no unbound job call, and retains no raw ID in the public diagnostic.
- The safe error classifier distinguishes `job_not_found`, generic tool execution failure, and true backend unavailability, including an appended `job_id` line.

### Restart and recovery tests

- No persistence or restart behavior changes; the focused job status tests continue to cover durable lookup.

### Security and negative tests

- No owner, job state, or raw private job content is returned for a failed lookup.
- Non-UUID, oversized, missing, and non-string IDs do not enter the new echo path.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tool::job
python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_diagnostics
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo fmt --check
git diff --check
```

Run additional focused checks selected by the actual changed files; do not claim a full verification sweep unless it completes cleanly.

## 12. Documentation updates

- Update the S019 closure status and subsystem roadmap with attempt 78 and S058 scope.
- Register S058 in `plans/registry.md` and record its closure evidence when completed.
- Preserve attempt 78's authorization and safe diagnostic records unchanged.

## 13. Acceptance criteria

- A failed valid-UUID `job.status` call carries its exact requested UUID in the returned error text without claiming a job state.
- Ordinary execution errors remain distinguishable from true backend-unavailable errors.
- The production-shaped ACP regression proves request/result ID correlation and confirms raw IDs/error text are absent from public diagnostics.
- Focused tests, guards, formatting, and diff checks pass.
- Attempt 79 uses the unchanged task, verifier, Muse route, request cap, native-call cap, disk reserve/floor, inactivity cancellation, prompt-work limit, wall stop, and cancellation rules. Qualification is based on all existing H007 gates and the independent verifier only.

## 14. Stop conditions

Stop before editing if the source path does not reproduce at the recorded baseline or if preserving the request ID would expose stored or foreign-owner job data. Stop before live admission for changed task, verifier, route, caps, mismatched ARM64 pair, unresolved campaign accounting, or less than the 30-GiB free-disk reserve. Do not expand into the independent continuation/verifier disagreement without new safe evidence.

## 15. Closure evidence required

Record the exact source and paired binary/helper hashes; focused test and guard results; regression assertions for failed status identity, error typing, and privacy; attempt 79 preflight/safe/monitor evidence hashes; provider aggregate reconciliation; exact duration and active work; job/event/review/repair/continuation gates; verifier outcome; disk start/minimum/end; external stop; current authorization counters; and unreported-USD treatment.

## 16. Handoff notes

Attempt 78 is preserved as unqualified. It completed both repair pairs and every public continuation check, but the review was unobserved, the agent timed out, and the independent verifier reported assertion failures. Its one failed status call had a string ID argument and no result ID; source inspection identified the missing-ID error path and a possible false backend-unavailable classification. The exact underlying call error remains unknown. The current user authorization has six starts remaining in `s019-supplemental-20261111-04`, with combined campaign and task authorizations also still available. Use only `opencode_go/muse-spark-1.3-contributor`; USD remains unknown if unreported. Preserve the frozen H007 task and verifier.
