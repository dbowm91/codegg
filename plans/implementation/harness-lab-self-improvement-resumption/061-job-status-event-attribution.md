# Harness Lab Self-Improvement Resumption S061 — Job-Status Event Attribution

Status: **active**

Repository baseline: `88d064dec604146ec657ee6cf706083bee7b6ae4`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#56-s061--job-status-event-attribution`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`

Primary class: infrastructure

## 1. Objective

Add bounded post-run attribution for failed H007 `job.status` calls, using run-local aliases and call order without retaining job IDs, arguments, outputs, or transcript content. Then run one unchanged H007 attempt (attempt 82) to determine whether the observed failed status calls cluster around particular jobs or sequences and whether the model can qualify the frozen task.

## 2. Why this milestone is ready

S060's attempt 81 completed all required implementation stages and final jobs but did not qualify. It recorded 123 `tool_execution_failed` job-status calls, two `job_not_found` calls, two unobserved request/result correlations, and three unbound job-tool calls. Existing safe output aggregates failure classes but discards the already-available run-local alias and event index. The current evidence does not identify a runtime owner; one bounded diagnostic improvement can answer that question without changing runtime behavior.

## 3. Current implementation evidence

- `dev/harness_lab/diagnostics.py::summarize_job_tool_sequence` already emits ordered job/test events with run-local aliases, action, status, error class, and request/result correlation while excluding raw IDs and tool payloads.
- `dev/harness_lab/campaign_authorization.py` projects the runner receipt through a strict allowlist, but currently does not copy a safe job-event timeline into the durable campaign diagnostic.
- `dev/harness_lab/h007_results.py` consumes sequence aggregates for qualification. Its predicates, task contract, and verifier outcome must not change in S061.
- Attempt 81 shows 425/427 applicable job request/result correlations matched and complete tool-event measurement; the cause of the generic status errors remains unresolved.

## 4. Invariants that must not regress

- Keep the frozen v97 task, independent verifier logic, route/model, limits, and acceptance gates byte-for-byte/behaviorally unchanged.
- Use only `opencode_go/muse-spark-1.3-contributor` for attempt 82.
- Record a run-local alias only; never retain raw job IDs or ID digests in the new trace.
- Include only fixed tool/action/status/error-class enums, bounded call indices and elapsed offsets, and correlation/presence states.
- The new projection is post-run diagnostic evidence only. It must not enter the model prompt, retry policy, scheduler, or qualification predicates.
- Preserve the 1,200 provider-request, 1,600 native-call, 3,300-second prompt-work, 3,600-second runner, 3,590-second external wall, 300-second inactivity, 10-GiB runtime disk, and 30-GiB preflight reserve limits.
- Retain private receipts/logs without inspection; USD remains unknown if unreported.

## 5. Scope

### In scope

- Validate and project bounded `job_tool_sequence_diagnostic.events` into safe campaign evidence.
- Add focused Python tests for alias, error-class, correlation, event-bound, and hostile-field redaction.
- Rebuild or verify the exact-head ARM64 CodeGG/sandbox-helper pair and run attempt 82 under the existing grant.
- Reconcile safe attempt-82 diagnostics, monitor summary, and aggregate campaign counters.

### Explicitly out of scope

- Changing `dev/harness_lab/h007_results.py`, the frozen task/verifier contract, prompt text, acceptance gates, or fixture manifest.
- Changing `src/tool/job.rs`, storage, daemon, scheduler, ACP, retry, or background-job semantics without attributable evidence of a concrete runtime failure.
- Increasing provider, wall-clock, prompt-work, or call limits.
- Adding synthetic waits, forced job failures, or task-specific source edits.
- Inspecting raw private logs, transcripts, tool arguments/results, verifier output, or credentials.

## 6. Required production changes

No CodeGG runtime behavior change is planned. The Python harness diagnostic projection must copy a bounded job/test event trace from the existing safe runner summary. The trace may contain only: run-local alias or null, call index, `job`/`test` tool class, action, call status, job status, fixed error class, job-ID argument state, result-ID presence, request/result correlation, and bounded elapsed offsets. Unknown fields are dropped. Invalid or oversized inputs yield `unavailable` or `truncated`; they must not be partially mislabeled as complete.

### Documentation and static guards

Update S019/S061 evidence only. No new static guard is warranted for this additive Python projection.

## 7. Ordered work packages

### Work package A — Freeze the diagnostic contract

Intent: Extend safe operator evidence without changing task or qualification behavior.

Required changes: Add a bounded sanitizer for existing job/test sequence events and attach it to safe campaign run fields. Keep run-local aliases and fixed enums only; reject unknown event schemas, malformed rows, oversize sequences, and hostile values.

Acceptance evidence: Focused unit tests prove valid aliases and error classes survive; raw IDs, ID digests, arguments, outputs, and injected unknown fields do not; oversized sequences are explicitly marked truncated.

### Work package B — Verify and commit the exact head

Intent: Ensure the live pair represents the diagnostic source and all existing runtime bits.

Required changes: Run focused Python tests, both repository test-quality guards for changed test bodies, Python compilation, `git diff --check`, and the ARM64 exact-head release build/hash checks.

Acceptance evidence: All required checks pass; task manifest and verifier-contract fingerprints remain unchanged; source branch is pushed before attempt 82.

### Work package C — Requalify attempt 82

Intent: Determine whether failed job-status calls align with malformed/unbound IDs, repeated calls for specific aliases, storage/tool failures across valid IDs, or ordinary agent retries.

Required changes: Run one case with the existing v97 task and verifier on Muse/OpenCode Go under the current grant and watchdog. Do not inspect private run material.

Acceptance evidence: Safe preflight, event-attributed safe diagnostic, monitor summary, provider count, and reconciled start counters. H007 qualifies only if all original predicates and the unchanged verifier pass.

### Work package D — Choose the next owner

Intent: Select a narrow corrective from the new evidence.

Required changes: If errors cluster on exact valid IDs under normal concurrent use, trace only that failure class to the responsible CodeGG owner and register a separate corrective. If errors align with malformed/unbound requests or duplicate model polls, record model/task behavior and avoid runtime changes. If the safe trace remains insufficient, identify the precise remaining observation gap before another live start.

Acceptance evidence: S061 closure names the cause supported by evidence, preserves H007's qualification result, and updates S019 counters without exposing raw identifiers.

## 8. Failure, cancellation, restart, and contention semantics

The diagnostic is post-run and read-only. A malformed, truncated, or unavailable trace cannot change a run result or retry/cancellation behavior. Attempt 82 retains the external watchdog's 3,590-second stop, 15-second termination grace, 10-GiB floor, and 300-second inactivity bound. No jobs, receipts, or live state are modified by post-run projection.

## 9. Compatibility and migration

No persistent schema or protocol migration. The safe campaign evidence receives one additive versioned diagnostic field; older receipts without the field project as `unavailable`.

## 10. Required tests

### Focused unit tests

- Event trace preserves bounded alias, index, action, status, safe error class, argument state, and ID-correlation state.
- Reject unknown enum values and malformed event shapes; drop injected private fields.
- A failed status with no canonical result ID remains unbound and does not gain an alias.
- A trace over the configured event bound is marked truncated and cannot claim complete attribution.

### Integration tests

- Safe campaign projection forwards the trace from a runner receipt and retains no raw identifier or payload.

### Other test classes

No Rust, migration, restart, contention, cancellation, or live-server tests are in scope because no CodeGG execution behavior changes.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_campaign_authorization
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m compileall -q dev/harness_lab
git diff --check
cargo build --release --locked -p codegg --bin codegg --bin codegg-sandbox-helper
```

Then run one exact-head live attempt 82 with the existing campaign preflight/watchdog command. Do not claim a check that was not run.

## 12. Documentation updates

- Close S060 conditionally with attempt 81's evidence and counters.
- Update `plans/closure/harness-lab-self-improvement-resumption/019-status.md` and roadmap counters with attempt 81.
- Record attempt 82 evidence and decide the next owner; keep H007 unqualified unless every gate passes.

## 13. Acceptance criteria

- The safe trace is bounded, versioned, rejects hostile fields, and contains no raw identifiers, argument values, output text, or transcript material.
- The prompt/task/verifier/manifest/gates/route and all resource caps are unchanged.
- Attempt 82 is counted only after provider admission; all evidence remains privacy-safe.
- Qualification requires the full original task, exact-ID lifecycle, two named repair pairs, continuation, duration, and independent verifier gates.
- Unqualified evidence distinguishes model/task sequencing from an attributable CodeGG runtime defect; no runtime fix is invented from aggregate error counts alone.

## 14. Stop conditions

Stop before provider admission if the grant is exhausted, the 30-GiB preflight reserve is not available, the exact ARM64 pair or source identity differs, the frozen task/verifier digest changes, the route is unavailable, or the campaign counters cannot be reconciled. During execution use the unchanged watchdog bounds. Stop planning a runtime correction if the event trace does not support a specific failure owner.

## 15. Closure evidence required

Record S061 source commit, focused check results, task/verifier fingerprints, attempt-82 preflight/safe diagnostic/monitor hashes, provider count, prompt/tool event counts, failed-status event distribution by run-local alias, ID-correlation and unbound states, repair-pair outcomes, continuation/verifier outcome, disk minimum/end, stop reason, and post-run authorization counters. Never include raw IDs or private run material.

## 16. Handoff notes

Attempt 81 is immutable failed evidence. After it, grant `s019-supplemental-20261111-04` has three starts remaining; combined campaign counters are 81/200 and current task authorization is 52/120. Attempt 82 uses one of those starts. The shell is translated x86_64 under Rosetta on an ARM64 Apple Silicon host; use the qualified ARM64 Rust toolchain and `/opt/local/bin/pkg-config`. Maintain at least 30 GiB free disk before start and the 10-GiB runtime floor. USD cost is unknown if unreported.
