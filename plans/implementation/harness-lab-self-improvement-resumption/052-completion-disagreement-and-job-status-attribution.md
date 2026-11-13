# Harness Lab Self-Improvement Resumption S052 — Completion Disagreement and Job-Status Attribution

Status: **implemented; closed at diagnostic scope after attempt 66**

Repository baseline: `de301938587a32366b634d94ba30abe2e9950379`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#46-s052--completion-disagreement-and-job-status-attribution`

Long-term requirements:

- `plans/000-long-term-specification.md` §§26–29
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: **diagnostic correctness**

## 1. Objective

Resolve the two public diagnostic gaps exposed by H007 attempt 65 without changing the v97 task, verifier, route, or qualification rules. The attempt completed 35.29 active minutes, all public stages and gates, both repair pairs, required final jobs, review, and public artifacts. The independent verifier still failed in `continuation_boundary` and `transcript_job_lifecycle`. Safe continuation checks all reported true; the job sequence showed one failed, unbound status call and one unobserved request/result correlation among 35 applicable calls. Existing safe evidence cannot show whether that call's error class was `execution_backend_unavailable` or `tool_execution_failed`.

## 2. Why this milestone is ready

- S051 corrected pointer placement, final-job summaries, and terminal-state projection; attempt 65 exercised these changes on the unchanged workload.
- Attempt 65's campaign record is reconciled and its privacy-safe diagnostic is available. No private receipt, runner log, raw transcript, continuation body, or hidden-verifier output is needed for this scope.
- The parser's `completed_phases` and `contract_phases` helpers currently treat unrecognized statuses such as `in_progress` as complete and do not project their observed statuses. This can make public continuation evidence look complete when a strict verifier would reject a status.
- Failed job-status error counts and job-ID binding counts are currently separate aggregates. Their association is lost before campaign evidence is written, so the next owner cannot determine whether a backend error coincided with the sole unbound call.

## 3. Current implementation evidence

`dev/harness_lab/h007_results.py` projects the required stages and gates with explicit status allowlists, but `documented_phases` and `documented_contract_phases` only exclude a small set of unfinished values. `dev/harness_lab/diagnostics.py` records the failed job-status error-class histogram, argument-shape counts, result-ID presence, and request/result-correlation states separately. `campaign_authorization.py` applies bounded allowlists to each existing field but has no cross-tab for those values.

Attempt 65 provides safe aggregate evidence: 182 provider requests; 201 tool starts and terminals, zero event gaps and unpaired starts; 7/7 stages and 12/12 gates; 34/35 request/result matches and one unobserved correlation; one unbound `status:string:failed` call; and failed status error classes totaling one `execution_backend_unavailable` and six `tool_execution_failed`. The aggregates do not identify whether the single unbound call was the backend-unavailable call.

## 4. Invariants that must not regress

- Keep H007 v97 task text and hidden verifier unchanged. Do not inspect or invoke the hidden verifier.
- Use only `opencode_go/muse-spark-1.3-contributor` for authorized live attempts.
- Preserve the existing 1,200-provider-request limit, 1,600-native-call limit, 3,300-second prompt-work limit, 3,600-second hard stop, 300-second inactivity cancellation, 10-GiB runtime disk floor, 30-GiB preflight reserve, and cancellation rules.
- Preserve at least 30 minutes of active dependent Rust work, exact-ID overlapping background work, two failure/repair cycles, continuation evidence, and independent verification as distinct qualification gates.
- Never retain job IDs, argument values or digests, error bodies, continuation text, prompts, transcripts, hidden assertions, credentials, private receipts, or runner logs.
- Preserve the user-authorized campaign accounting; after attempt 65 the combined campaign is 65/200, the active supplemental grant is 21/40, and task authorization is 36/120. External billed USD remains `unknown` if unreported.

## 5. Scope

### In scope

- Parse and project status values for `completed_phases` and `contract_phases` using only the documented complete/incomplete status vocabulary. Unrecognized statuses must not count as complete.
- Carry only fixed phase names and allowlisted status values into safe continuation evidence; reject malformed, duplicate, unknown, and private fields.
- Add a bounded job-status failure cross-tab joining the fixed error class with job-ID argument shape, result-ID presence, and request/result-correlation state. It must distinguish an unbound model call from a status call that failed before returning a job result without storing any identifier or error text.
- Validate every new field through the safe campaign projector with explicit enums and counts capped at 1,600.
- Use the next separately counted attempt, after exact-head preflight, to validate the corrected diagnostics on the unchanged v97 task.

### Explicitly out of scope

- Changing the task, verifier, provider route, model, request/time/disk limits, daemon, scheduler, ACP protocol, job semantics, or qualification predicates.
- Treating an unobserved result as a successful lifecycle observation.
- Inferring which failure class belongs to attempt 65's unbound call from separate histograms.
- Widening the workload or redesigning the harness architecture.

## 6. Required production changes

### Core/domain

- None.

### Storage and migrations

- None; this is an additive harness evidence change.

### Protocol and DTOs

- None; use existing ACP telemetry and safe campaign evidence.

### Runtime and concurrency

- Do not change job execution or status behavior. Preserve correlation in memory only while producing the safe cross-tab.

### Frontend or operator surface

- None.

### Security and authorization

- Apply explicit enum and numeric bounds in both source diagnostics and campaign projection. Omit raw IDs, digests, arguments, outputs, paths, and arbitrary labels.

### Documentation and static guards

- Document the two attempt-65 failure domains and the distinction between a verifier/public-projection disagreement and a demonstrated product defect.

## 7. Ordered work packages

### Work package A — Strict phase status evidence

Intent: make the public continuation projection agree with the accepted status vocabulary.

Required changes:

- Add a shared parser for the phase and contract-phase entries that preserves only fixed required labels and allowlisted statuses.
- Count only `complete` or `completed` as complete; represent `pending`, `failed`, `in_progress`, and `not_started` as incomplete, and unknown status as `other`.
- Include phase and contract-phase status summaries in the safe diagnostic and require all required entries to be complete for `document_valid`.

Acceptance evidence:

- Production-shaped fixtures cover complete, pending, failed, in-progress, not-started, unknown, duplicate, missing, and malformed phase values.
- The campaign projection retains only known fields, required labels, allowlisted statuses, and bounded counts.

### Work package B — Per-class job-status binding attribution

Intent: preserve enough safe evidence to identify which fixed status-error class is associated with a missing result ID or unmatched request.

Required changes:

- Aggregate each failed `job status` call by fixed error class, argument-shape enum, result-ID-presence state, and request/result-correlation enum.
- Emit only bounded aggregate counts; do not include a job ID, digest, raw argument, or output.
- Validate nested or row-shaped aggregates fail-closed in the campaign projection, including duplicate dimensions, unknown enum values, booleans-as-counts, and values over 1,600.

Acceptance evidence:

- Production-shaped ACP event fixtures prove an `execution_backend_unavailable` status failure can be distinguished from a model-provided missing/malformed ID and from a tool-execution failure.
- Privacy assertions show that all supplied private markers and job identifiers are absent from serialized diagnostics.

### Work package C — Offline verification and one continued live attempt

Intent: validate the corrected projections on the unchanged workload.

Required changes:

- Run the focused Python suites, applicable assertion-quality and duplication guards, Python compilation, and whitespace checks.
- Verify current branch head, exact v97 manifest and paired ARM64 artifacts, authorization headroom, credential availability without printing it, and at least 30 GiB free disk before admission.
- Use the existing live monitor on the lane-local request receipt and safe causal trace. Do not infer in-progress status from the post-run campaign ledger.
- Reconcile only the safe diagnostic and campaign counts after completion.

Acceptance evidence:

- Record the next attempt's exact preflight and safe diagnostic paths, active duration, provider requests, event/job aggregates, minimum sampled free disk, and qualification decision.
- H007 qualifies only if the unchanged independent verifier passes and every existing gate passes. Otherwise preserve the attempt and state the remaining discrepancy without claiming qualification.

## 8. Failure, cancellation, restart, and contention semantics

The new evidence is derived after each ACP call and is finalized once the live attempt terminates. A missing, malformed, or unbounded value is projected as unavailable or omitted; it never defaults to a successful match. Cancellation, request/time/disk limits, and launch failures remain counted according to the existing campaign rules. No cross-run identifiers are retained.

## 9. Compatibility and migration

No product or persisted workspace schema changes. Existing campaign records without the new diagnostic continue to project as unavailable; new records contain only additive, allowlisted aggregate fields.

## 10. Required tests

### Focused unit tests

- Phase and contract-phase parser truth table, including unsupported statuses.
- Failed-status attribution cross-tab for matched, mismatched, unobserved, and non-string/missing argument shapes.

### Integration tests

- Safe task and campaign projections preserve the new aggregates and reject private or malformed data.

### Restart and recovery tests

- Not applicable to this Python-only, post-run projection change.

### Contention and cancellation tests

- Preserve existing campaign accounting and do not create an attempt row before provider admission or lose a counted start after admission.

### Security and negative tests

- Reject unknown labels, statuses, classes, duplicate aggregate rows, private identifiers, raw strings, Boolean counts, and out-of-range counts.

### Migration and compatibility tests

- Older safe evidence without the new fields remains readable as unavailable.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_h007 \
  dev.harness_lab.tests.test_autonomy_run \
  dev.harness_lab.tests.test_verifier_diagnostics \
  dev.harness_lab.tests.test_campaign_authorization
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/h007_results.py \
  dev/harness_lab/diagnostics.py dev/harness_lab/autonomy_run.py \
  dev/harness_lab/campaign_authorization.py
git diff --check
```

## 12. Documentation updates

- Update S051 closure and the subsystem roadmap/registry to record attempt 65.
- Add S052 status evidence after offline verification and the next live attempt.

## 13. Acceptance criteria

- Unrecognized or unfinished phase statuses no longer appear complete in safe diagnostics or `document_valid`.
- Failed job-status error classes can be safely associated with argument shape, result-ID presence, and request/result-correlation state.
- Existing private-data exclusions and bounds remain enforced.
- One fresh unchanged-task attempt records whether the public continuation mismatch and failed status call are now attributed; H007 stays unqualified unless the independent verifier passes.

## 14. Stop conditions

- Stop if safe per-call attribution requires retaining private IDs, argument digests, raw outputs, or the hidden-verifier text.
- Stop if the run identity or authorization differs, the 30-GiB preflight reserve is unavailable, or authorization headroom is exhausted.
- Preserve evidence if any existing request, time, inactivity, call-count, disk, or cancellation limit fires.
- Do not change task content or widen the route/cap envelope to improve the result.

## 15. Closure evidence required

Record implementation commit, exact focused command results, preflight and safe-evidence paths, safe aggregate fields, campaign/grant/task accounting, minimum sampled disk, unreported-USD treatment, and whether attempt 66 qualified. State explicitly that private receipts, runner logs, raw transcripts, and hidden-verifier output were not inspected.

## 16. Handoff notes

Attempt 65 used the unchanged v97 workload, Muse Spark 1.3 Contributor via OpenCode Go, and the existing paired ARM64 binary/helper. It completed 35.29 active minutes with 182 requests and 201 measured tool calls; the verifier failed. The active grant has 19 starts remaining after that attempt, the combined campaign has 135, and task authorization has 84. Maintain 20-second safe monitoring samples and continue user-facing progress updates at least every minute during a live run.

Attempt 66 exercised the S052 diagnostics on the unchanged v97 workload. The phase/status and job-status error cross-tab were projected safely, but H007 remained unqualified after a prompt-work timeout; see `plans/closure/harness-lab-self-improvement-resumption/052-status.md`. At attempt 66, the grant had 18 starts remaining, the combined campaign had 134, and task authorization had 83. Correct the monitor's lane-local provider-admission path before the next attempt.
