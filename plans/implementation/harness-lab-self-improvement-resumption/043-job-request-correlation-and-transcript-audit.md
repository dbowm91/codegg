# Harness Lab Self-Improvement Resumption S043 — Job Request Correlation and Transcript Audit

Status: **conditionally closed at safe diagnostic scope; attempt 54 unqualified; S044 active**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#37-s043--job-request-correlation-and-transcript-audit`

Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/003-planning-process.md` §§6–7.

Applicable ADRs: ADR-0040, evidence-based agent recovery and termination.

Primary class: infrastructure

## 1. Objective

Make the public job-lifecycle evidence prove that a `job` action requested the same opaque ID that its result reports, and prove that the transcript file consumed by the independent verifier preserves the ACP-projected event sequence. If either relation is false or unavailable, identify it as incomplete public evidence before another S019 live attempt.

## 2. Why this milestone is ready

Attempt 52 used `opencode_go/muse-spark-1.3-contributor` for 39.6 minutes of active work, completed the public task stages, exact-ID-looking job overlap, three repairs, reviewer work, and continuation; its unchanged verifier passed 4/5. The sole safe failure domain was `transcript_job_lifecycle`, although every current public lifecycle Boolean was true and no call was unbound. S042 records only whether a job-ID argument is string-shaped. Source inspection shows the ACP projector separately parses the response ID, so the current public summary does not establish request/response ID equality. The verifier consumes a serialized transcript while public summaries are built from the in-memory projection. These are concrete evidence gaps; they are not yet proof of the hidden assertion's cause.

## 3. Current implementation evidence

- `src/acp.rs` emits job-ID argument shape and an argument digest, but not a dedicated digest of the requested job ID.
- `dev/harness_lab/autonomy_run.py::_ToolCallProjector` derives response job metadata from the ACP tool result and `build_transcript` serializes the resulting calls.
- `dev/harness_lab/diagnostics.py::summarize_job_tool_sequence` correlates response IDs in memory and retains only run-local aliases.
- Attempt 52's safe receipt shows zero unbound job calls, complete event pairing, and a `verifier_public_projection_disagreement`; raw transcript and verifier details remain uninspected.

## 4. Invariants that must not regress

- Never retain or emit raw request IDs, transcript bodies, tool arguments/outputs, verifier text, test names, or credentials in public diagnostics.
- Preserve the v97 task and independent verifier, Muse route, provider and tool ceilings, timeouts, disk floor, and cancellation rules.
- A lifecycle is not exact-ID-qualified unless the requested ID and observed result ID match for every ID-bearing job action.
- Missing, malformed, truncated, or unparseable serialized evidence is unavailable/incomplete, never a pass.

## 5. Scope

### In scope

- Emit a SHA-256 of a valid string `job_id` argument for ID-bearing job actions in the ACP notification; carry it only in the private projected call needed for correlation.
- Compare that digest with the digest of the result ID in memory; persist only per-call match state and bounded match/mismatch/unobserved counts.
- After writing the verifier transcript, read it through a bounded path, parse it, and compare its safe ordered job/ACP projections with the source projection. Persist only counts, schema/shape booleans, and match state.
- Include both predicates in public lifecycle reconciliation, with explicit incomplete evidence on mismatch or unavailable serialization.
- Add production-shaped privacy, round-trip, mutation, and malformed-input regressions and a new live S019 diagnostic/requalification receipt.

### Out of scope

- Reading or invoking hidden verifier source outside the normal harness run; retaining verifier messages, names, or raw output; weakening any verifier or task gate.
- Changing the v97 fixture, task prompt, route, agent behavior, provider interface, scheduler, or job architecture.
- Inferring that attempt 52's verifier is defective or that the new telemetry explains its failure until the relation is measured.

## 6. Required production changes

Add privacy-bounded request-ID digest metadata at the ACP boundary. Carry only the digest in the in-memory harness projection and reduce the request/result comparison to a Boolean before receipts are written. Add a bounded serialized-transcript audit at the point the verifier transcript is persisted. Wire the safe audit state into S019 public reconciliation. Keep all changes compatible with existing ACP consumers by using optional namespaced metadata.

## 7. Ordered work packages

1. Implement typed request-ID digest extraction for supported `job` actions; invalid or absent IDs produce no digest.
2. Correlate request and response digests without retaining either in safe receipts. Add counts for matching, mismatching, not-observed, and not-applicable calls.
3. Audit the exact persisted transcript with a strict byte ceiling and bounded JSON parse. Compare ordered job and productive-overlap projections against the in-memory source.
4. Reconcile these booleans with `transcript_job_lifecycle`; ensure an incomplete audit cannot be reported as a verifier/public-projection disagreement.
5. Add and run focused Python/Rust regressions and privacy checks; update S042 evidence, S043 plan/roadmap/registry, and safe authorization counters.
6. Build a fresh paired ARM64 binary/helper and run the unchanged v97 case after validating every launch input, including the admitted budget-policy file, disk, identity, caps, and authorization. The launch preflight must verify the policy file exists and encodes the same 1,200-request and 3,600-second limits enforced by the outer runner. A refused launch with zero provider requests is recorded as a pre-provider operational failure and does not consume a start.
7. Continue the authorized diagnose/fix/run loop only while a corrective is evidence-led and starts remain.

## 8. Failure, cancellation, restart, contention semantics

The digest is observational and grants no authority. ACP event gaps, incomplete start/terminal pairs, oversized/malformed transcript files, digest mismatch, or unreadable persisted evidence produce an incomplete lifecycle result. The existing run-wide cancellation, disk-floor, request, call, inactivity, and wall-clock limits remain authoritative. No retry is counted as a fresh start unless provider admission occurs under the existing idempotent ledger rule.

## 9. Compatibility and migration

ACP fields are optional and namespaced; consumers that ignore them retain current behavior. The harness receipt schema gains an optional S043 diagnostic object. No durable product storage migration is required.

## 10. Required tests

- Rust: valid, absent, malformed, empty, and non-string job-ID arguments; digest matches the valid value without exposing it; unsupported actions are not misclassified.
- Python: request/response ID match, mismatch, missing digest, malformed response ID, and submit/no-ID non-applicability.
- Serialized transcript: exact production-shaped round-trip match; reordered, modified, oversized, invalid JSON, and wrong-shape evidence fail closed.
- Privacy: serialized safe diagnostics contain no raw job IDs, arguments, output body, transcript text, verifier text, or credential sentinel.
- Public reconciliation: matched evidence preserves a true state; mismatch/unavailable evidence is incomplete and cannot become a projection disagreement.

## 11. Required verification commands

- `python3 -m unittest dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_h007`
- `python3 -m unittest dev.harness_lab.tests.test_verifier_diagnostics`
- `python3 scripts/check_test_assertion_quality.py`
- `python3 scripts/check_test_duplication.py`
- `cargo fmt --check`
- Focused ARM64 Rust regression using the installed Rust 1.89 toolchain and `--target aarch64-apple-darwin`.
- `git diff --check`; inspect only safe diagnostic output and bounded source diffs.

## 12. Documentation updates

Update the S019 roadmap section, registry, and status records with the measured relation and accurate grant/run counts. Preserve attempt 52 and prior attempts as failed historical evidence. Document the source of each lifecycle match without copying identifiers or raw transcript/verifier content.

## 13. Acceptance criteria

- A job call is publicly exact-ID-qualified only when its request and result IDs match under a one-way digest comparison.
- The exact transcript file presented to the verifier parses within the bound and its safe ordered tool/job projections match the ACP source projection.
- All new negative and privacy regressions pass; existing focused tests and guards pass.
- The unchanged v97 Muse run either qualifies under all existing task/horizon criteria or yields safe, actionable evidence without a false task/lifecycle pass. A successful diagnostic implementation alone does not qualify H007.

## 14. Stop conditions

Stop before provider admission if free disk is below the 30-GiB preflight reserve, paired ARM64 identity or v97 manifest changes unexpectedly, request/call admission is not enforceable, the current supplemental grant is exhausted, credential handling would be exposed, or safe telemetry cannot be maintained. Stop a live run at the existing 10-GiB floor, 1,200 requests, 1,600 native calls, 3,300 prompt-active seconds, 3,600 wall seconds, 300 seconds of inactivity, or operator/cancellation trigger.

## 15. Closure evidence required

Record exact implementation SHA, tests/guards and ARM64 build result; safe before/after relation counters; paired binary/helper and manifest digests; request/time/disk/cancellation evidence; complete attempt/grant accounting; unchanged verifier summary; and unresolved classification. No closure may claim H007 qualification unless the independent verifier and every existing horizon/task gate pass.

## 16. Handoff notes

Use only `opencode_go/muse-spark-1.3-contributor` with the already rotated local OpenCode Go credential. Current ledger after attempt 52: 53/200 combined starts consumed; latest grant 9/40 consumed and 31 remain. External billed USD remains unknown when unreported. Preserve all current controls and don't inspect raw prompts, transcripts, tool arguments/results, runner logs, or hidden verifier source/output.

## 17. Attempt 53 launch finding

Attempt 53 exited before provider admission (exit code 2, zero requests/tool calls, no receipt, and no case lane). The private lane was missing `budget.json`; the autonomy CLI's first admission step reads and validates that file before loading the suite, creating a case lane, or contacting a provider. This is a launch-wrapper input omission, not a model or product-run failure. The authorization ledger and 9/40 grant count remain unchanged. Safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt53-launch-failure-20261111.json`. Attempt 54's preflight now binds the exact policy-file digest, verifies its request/wall-clock caps, and rejects the launch before subprocess creation if the file is absent or differs.

## 18. Attempt 54 diagnostic result and bounded closure

Attempt 54 used `opencode_go/muse-spark-1.3-contributor` under the unchanged v97 task, verifier, and run limits. It consumed 1,063/1,200 provider requests and 1,031/1,600 native tool calls, ran for 49.1 active minutes, and reached the 100-followup limit with about 11 minutes of wall time remaining. The task failed and the unchanged verifier passed 3/5; H007 remains unqualified.

S043's two evidence predicates are now measured: all 55 applicable request/result job-ID correlations matched, and the exact serialized transcript projection matched the source projection. The public lifecycle remains incomplete for one failed background `test` submission without an observed job ID. The safe projection confirms the ACP tool call failed but cannot classify the typed tool error. This is a bounded missing diagnostic, assigned to S044; it does not establish that the failed submission caused either verifier assertion.

The reviewer run reached terminal `failed/deadline_exceeded` at about 21.9 minutes. The parent fetched its result once after 9.1 minutes, then issued 74 status/wait calls and did not use the one permitted continuation. The diagnostic establishes that the terminal failure was surfaced and the continuation action was available; it does not indicate a lost daemon/ACP event. The task tool's recovery instructions are too easy to miss at the terminal decision point, so S044 owns a narrow agent-facing recovery cue and safe repeated-poll evidence.

S043 is conditionally closed at its declared correlation/transcript-audit scope, not as an H007 pass. Attempt 54 is counted. The current supplemental authorization has 10/40 starts consumed and 30 remaining; the combined ledger is 54/200. Preserve the unchanged controls and keep unreported external USD cost as unknown. Safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt54-safe-diagnostic-20261111.json`.
