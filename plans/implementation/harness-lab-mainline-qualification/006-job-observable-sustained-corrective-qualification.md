# H006 — Job-Observable Sustained Corrective Qualification

Status: **offline prerequisites proven; exact-head hosted CI passed; fresh explicit live authorization required before live qualification**

Subsystem roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`

Prior evidence: `plans/closure/harness-lab-mainline-qualification/005-status.md`

## 1. Objective

Correct the two evidence gaps exposed by H005 and run one separately authorized, attainable sustained Rust engineering task through CodeGG's existing `opencode_go` integration. The work must prove that an asynchronous job remains observable by exact ID while the agent continues dependent work, and that the provider request cap is bound and enforced at the provider boundary.

H005's result remains unchanged: its one live attempt was consumed, did not meet the duration or verifier requirements, and has unknown actual provider request usage. H006 inherits no remaining request, time, or spend authority.

## 2. Work classification and boundaries

- **Infrastructure:** exact-ID observation of a submitted CodeGG job through running and terminal state, with bounded projection that exposes lifecycle status but not job output or workspace data.
- **Invariant:** the configured request limit reaches the child process and the `opencode_go` provider boundary refuses request `N+1` before transport; absent, malformed, or unreadable cap configuration must fail closed for this qualification.
- **Capability:** a single CodeGG native agent session performs dependent Rust development, overlaps real asynchronous validation with continued work, recovers from a diagnosed failure, and completes a continuation stage.

Use only CodeGG's existing `opencode_go` adapter and the already selected Muse Spark route, subject to fresh authentication and budget admission. Do not introduce another provider adapter, scheduler, job system, or approval path. Do not retry H005 under its consumed authorization.

## 3. Prerequisites — offline only

1. Trace the H005 live job path from tool submission through job ID extraction, status observation, exact-ID wait, terminal result, and receipt projection. Identify why all seven observations were null; a unit test that only parses a synthetic status string is insufficient evidence for the composed path.
2. Add or correct focused offline coverage so the composed projection records the submitted ID and observes both `running` and a terminal state for that same ID while the session continues. Unknown, missing, mismatched, or unobserved status must remain explicit and must fail the verifier; it cannot be counted as a successful background job.
3. Verify request-cap binding from the run policy through `SubjectSpec.environment()` into the child and `crates/codegg-providers` request boundary. Tests must prove a trial-provided limit works without inherited parent environment, exactly `N` admissions succeed, admission `N+1` is refused before transport, and the receipt records each admitted request. Malformed or absent cap configuration must not silently report an enforced bounded run.
4. Run focused job-observation and provider-cap tests. Record exact commands/results. Keep the private lane and receipt free of API secrets, raw provider response bodies, and unnecessary job output.
5. Review the staged task against the independent hidden verifier before live admission. Keep a bounded dependency chain (for example, repair ingestion/parsing, add a derived index, recover a journal after a deterministic failure, then integrate a CLI query). The task must make useful progress possible after each stage and must include two diagnosable failure/repair cycles without requiring external network access.

If these prerequisites uncover a product defect, make the narrow corrective on the H006 branch and pass its focused offline tests before requesting live budget. H005 and PR #211 remain separately reviewable.

## 4. Live authorization gate

No live attempt is authorized by this plan. Before starting one, obtain a new explicit approval that names:

- `opencode_go` and the selected model route;
- a hard provider-request ceiling that includes any fresh credential/catalog probes;
- a hard wall-clock limit no longer than 60 minutes, with a target of 30–60 minutes of active engineering;
- a free-disk stop floor and cancellation behavior;
- the treatment of unreported USD cost;
- one attempt and the stop conditions below.

The runner must bind and verify the request cap before its first provider request. Do not infer remaining budget from H005's receipt: its actual request usage is unknown. If no fresh approval arrives, stop after offline work and record H006 as blocked before live qualification.

The offline job-observation gate uses bounded job metadata parsed from native ACP tool output. It does not claim a separate automatic terminal notification or define a structured job-lifecycle event protocol; those require separate production-path evidence and scope.

## 5. Live workload and evidence

On approval, run one coherent native CodeGG session against the immutable H006 fixture and private workspace. The task must:

- make progress through multiple dependent Rust stages;
- submit a real CodeGG-owned asynchronous test/validation job and observe that exact job ID in `running` state;
- continue meaningful downstream coding while that job remains active;
- wait on the same ID and observe a terminal status/result before using its evidence;
- diagnose and repair two distinct seeded or naturally occurring failures;
- survive the planned continuation/recovery boundary without losing task state or repeating already completed work;
- reach at least 30 minutes of active session time and stop no later than 60 minutes.

The verifier checks stage outputs, both repair chains, exact job lifecycle ordering, continued work during the active interval, continuation state, event integrity, request/time/disk bounds, and cleanup. A failed or unreachable verifier, null/mismatched job status, cap uncertainty, insufficient active duration, or incomplete cleanup is a failed qualification, even if some source files compile.

Retain a sanitized result receipt with source and candidate digests, exact run configuration, bounded counters/timings, provider admission count, observed job lifecycle, verifier outcome, disk minima, stop reason, and cleanup result. Do not retain the raw transcript; retain only its digest if needed for integrity.

## 6. Stop conditions

Stop before live startup if the new authorization is absent, cap configuration cannot be verified, the selected route cannot be bound without creating unapproved credential state, or disk is below the approved floor. During the run, cancel on request/time/disk limit, any provider-cap accounting gap, unrecoverable daemon/job-state loss, or evidence that the projected job ID/status is not the submitted job. Never restart a stopped attempt under the same authorization.

## 7. Completion and handoff

H006 closes as sustained-qualified only when all offline prerequisites and live workload criteria pass with a complete sanitized receipt and exact-head hosted CI. Otherwise close or leave it blocked with the failed criterion and evidence stated precisely. A successful offline cap or projection test is infrastructure evidence; it is not sustained autonomous capability evidence.

PR #211 can be reviewed and merged independently on its own exact-head hosted checks. H006 must use a separate branch and must not recast H005 as qualified.
