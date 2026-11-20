# Harness Lab Self-Improvement Resumption S062 — Job-Status Query Success Semantics

Status: **closed at job-status query semantics; attempt 83 completed unqualified**

Repository baseline: `2e2b5701eaf79f4f38a2c6d5312f9a14058aeb36`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#57-s062--job-status-query-success-semantics`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Primary class: infrastructure

## 1. Objective

Correct one model-facing job-tool result semantic identified by S061 and attempt 82: a successful `job.status` lookup must be a successful tool call even when the stored job state is `failed`, `cancelled`, or `timed_out`. Preserve the terminal job state in the structured value and bounded output. A missing, unauthorized, or unreadable job lookup remains a failed call.

Attempt 83 used the existing S019 authorization with the frozen v97 task, verifier, manifest, Muse/OpenCode Go route, and established resource caps. The status-query correction was exercised. H007 remains unqualified because the continuation record was invalid and the agent timed out; S063 owns one final unchanged attempt under the remaining grant start.

## 2. Evidence and diagnosis

Attempt 82 preserved complete relevant lifecycle measurement: 338/338 tool events paired, zero gaps, 87/87 applicable request/result correlations matched, and no unbound job calls. Failed-status events clustered on run-local aliases J01 and J03 even though their identifiers correlated. `src/tool/job.rs::execute_structured` sets `success` from the returned job state for every action. Thus `action=status` plus a stored failed job state yields `success=false` after a successful store lookup. The broker reports a failed tool call, and the harness classifies the event as `tool_execution_failed`; repeated status attempts are a predictable response. No event-loss, daemon, scheduler, or ACP defect was established.

Authoritative evidence is recorded in `plans/closure/harness-lab-self-improvement-resumption/061-status.md` and the attempt 82 safe diagnostic. The private receipt, runner logs, transcript, raw tool/provider payloads, and hidden verifier source/output remain uninspected.

## 3. Invariants

- `status` reports the job outcome as data; successful observation is not conflated with job success.
- `wait`, `output`, `cancel`, and all lookup-error behavior retain their existing semantics.
- Missing/foreign jobs, absent scheduler, store failures, malformed IDs, and authorization failures must not be reported as successful observations.
- Do not change job state, event lifecycle, scheduler/daemon/ACP architecture, retry policy, prompt/task/verifier/manifest, provider route, qualification predicates, or resource caps.
- The live route remains `opencode_go/muse-spark-1.3-contributor` only. Respect the existing 1,200 provider-request, 1,600 native-call, 3,300-second prompt-work, 3,590-second external wall, 300-second inactivity, 10-GiB runtime disk, and 30-GiB preflight reserve bounds. USD remains unknown if unreported.
- Preserve private run material without inspection.

## 4. Scope and work packages

### A. Correct the status result bit

For a successfully returned `action=status` value, set `StructuredToolResult.success=true` independent of the projected job-state string. Keep `value.status`, terminal envelope, summary, and bounded output unchanged. Keep structured failure results false when the lookup itself fails.

### B. Add production-shaped regression coverage

Create a durable in-memory job, transition its attempt to `Failed`, then call the actual `JobTool::execute_structured` status path as the exact owner. Assert that the tool call succeeds while `value.status == "failed"` and the bounded output still reports `job failed`. In the same test, query a canonical missing job ID and assert failure with the fixed `job_not_found` class. Existing no-scheduler status coverage must continue to assert failure.

### C. Verify exact source and paired binaries

Run formatting, focused scheduler-job and job-tool tests, the relevant quick guard(s), and `git diff --check`. Build the CodeGG and sandbox-helper ARM64 binaries using the qualified ARM64 Rust 1.89 toolchain and `/opt/local/bin/pkg-config`; verify both artifacts are ARM64 and bind their hashes to the exact source head. Keep disk usage monitored and preserve at least 30 GiB free before any live start.

### D. One live requalification

Run attempt 83 only after the standard side-effect-free campaign preflight passes and the existing grant remains available. The external watcher must poll the safe provider-boundary request aggregate during the run, enforce the disk floor and 3,590-second hard wall, and record cancellation reason. The frozen H007 task/verifier and acceptance criteria remain unchanged; task outcome and long-horizon qualification remain separate. Do not inspect private run content.

## 5. Acceptance and disposition

The offline corrective passes when the regression demonstrates successful failed-state observation, actual missing-job failure, and the existing no-scheduler failure. S062 may close at implementation scope only if those checks and exact ARM64 build pass. Attempt 83 qualifies H007 only if every original gate passes, including at least 30 minutes active dependent Rust work, exact-ID job lifecycle, productive overlap, two required repair cycles, continuation, and independent verifier success.

Attempt 83 is unqualified and leaves H007 unqualified. Its safe evidence identifies no additional runtime defect: every incomplete public check is in continuation layout or the timeout terminal record. S063 is a bounded unchanged requalification, not a new runtime correction. Do not change task, prompt, verifier, event architecture, scheduler, daemon, ACP, or qualification predicates.

## 6. Stop conditions

Stop before provider admission if the campaign preview is not clean, no grant start remains, the exact task/verifier/manifest identity changes, the ARM64 pair is stale or mismatched, the model route is unavailable, free disk is below 30 GiB, or live request-boundary telemetry cannot be polled. During execution, stop at 10 GiB free disk, 3,590 seconds, 300 seconds without native tool progress, explicit cancellation, or 1,200 admitted requests. The monitor terminates gracefully for 15 seconds before force-killing the runner.

## 7. Closure evidence

Record the implementation SHA, focused checks, build artifact architectures/hashes, preflight and safe attempt 83 evidence hashes, provider-boundary and reconciled request counts, prompt/tool counts, activity horizon/gap, exact-ID lifecycle and repair outcomes, continuation/verifier outcome, minimum/end free disk, stop reason, authorization counters, and unknown USD when unreported. Never copy private receipt/log/transcript/tool data or credentials.
