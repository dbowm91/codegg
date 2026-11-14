# Harness Lab Self-Improvement Resumption S055 — Safe Runner Exception-Site Telemetry

Status: **implemented and offline-verified; attempt 74 pending exact-head live evaluation**

Repository baseline: `a4b37130708ac20ffde5242053b453b1854bdff3`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#49-s055--safe-runner-exception-site-telemetry`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Applicable ADRs: ADR-0040, evidence-based agent recovery and termination; ADR-0051, delegated-run continuation authority.

Primary class: infrastructure

## 1. Objective

When the H007 autonomy suite raises after admitting provider requests, retain a bounded source-function path from CodeGG's own harness frames so the next live result can locate the failing runner region without exposing exception text or private run data.

## 2. Why this corrective is ready

S054 successfully reconciled attempt 73's 396 provider admissions and one consumed start after an exceptional exit. The safe diagnostic records only `unexpected_exception`; completion, task, and verifier projections are unavailable. Attempt 73 ran 3,302.526 seconds with no external disk or wall stop, but it does not establish the failure source or H007 qualification. Its safe evidence is `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt73-safe-diagnostic-20261114.json`.

## 3. Current implementation evidence

`dev/harness_lab/__main__.py` catches the H007 suite exception, maps it to a fixed class, and reconciles the bounded provider-admission aggregate. `campaign_authorization.safe_campaign_run_fields` allowlists safe fields and drops raw exception values. The missing diagnostic is the source function within the runner call stack.

## 4. Invariants that must not regress

- Keep the v97 task, independent verifier, manifest, model route, request/time/native-call/disk/inactivity/follow-up caps, and qualification gates unchanged.
- Never retain exception messages, tracebacks, source paths, line numbers, locals, provider content, transcripts, tool payloads, verifier text, credentials, or job IDs.
- Retain only a small ordered list of validated `dev.harness_lab` module and function identifiers; exclude test modules and malformed/unbounded values.
- Continue reconciling starts only from the strict bounded provider-boundary aggregate. Never synthesize a private receipt after `run_suite` raises.
- An exceptional run remains unqualified even when failure-site telemetry is available.

## 5. Scope

### In scope

- Add a bounded function-site projection to the H007 exception boundary.
- Validate the projection again at the campaign-safe-field boundary.
- Add production-shaped tests proving in-repository frame capture and rejection of paths, test frames, malformed strings, and overlong values.
- Update attempt 73 evidence and close S054 at its accounting scope.

### Explicitly out of scope

- Changes to the workload, hidden verifier, model, provider route, run limits, or qualification criteria.
- Changes to ACP, scheduler, daemon, or provider architecture absent evidence from a later run.
- Recording exception text, traceback objects, file paths, line numbers, stack locals, or raw run artifacts.

## 6. Required production changes

### Runtime and safe projection

At the existing H007 `run_suite` exception boundary, derive at most eight de-duplicated frame labels from the traceback. Read only each frame's module name and code function name; require a strict identifier form under `dev.harness_lab`, exclude `.tests.` modules, and omit all file, line, local, and exception data. Project the labels through the campaign sanitizer with an explicit observed/unavailable state.

### Documentation and static guards

Record attempt 73's outcome accurately, reconcile campaign/grant/task counts, close S054 only at its accounting scope, and register S055 as active. Preserve H007 as unqualified.

## 7. Ordered work packages

### Work package A — Bounded safe frame extraction

Intent: Attribute an exceptional runner exit to CodeGG harness functions while preserving the existing privacy boundary.

Required changes: Add the bounded traceback-frame projection, attach it only to the positive-admission exception diagnostic, and independently validate its schema in `campaign_authorization.py`.

Acceptance evidence: Production-shaped test observes a harness function label; tests prove private exception text, test modules, paths, malformed entries, and overlong labels are not retained.

### Work package B — Reconcile S054 and register S055

Intent: Make the attempt 73 result and current authorization state reviewable.

Required changes: Add the safe preflight, campaign diagnostic, and corrected monitor summary; update the authorization ledger, S019 status, roadmap, and registry; add S054's accounting-scope closure record and this plan.

Acceptance evidence: Safe evidence hashes and counters agree; the documents state that task and verifier outcomes are unavailable and H007 is unqualified.

### Work package C — Offline verification and live evaluation

Intent: Verify the sanitizer before attempting to use it on the unchanged H007 workload.

Required changes: Run focused Harness Lab tests, test-quality guards, Python compilation, and diff checks. Build exact-head ARM64 binaries, run fresh preflight at the existing reserve, and execute attempt 74 with the existing Muse authorization and controls.

Acceptance evidence: Offline checks pass; a fresh preflight admits only the configured Muse route; attempt 74 records either a safe exception-site path or a normal safe completion projection. The run qualifies only if every pre-existing H007 gate passes.

Offline verification on 2026-11-14: the focused CLI, campaign authorization, and autonomy-run modules passed 114 tests. The new regression confirms that the safe site projection identifies `dev.harness_lab.autonomy_run:run_suite` without retaining the exception message or path. Python compilation, both test-quality guards, and `git diff --check` passed. Attempt 74 remains pending a pushed exact-head commit, a newly built ARM64 binary/helper pair, and fresh preflight.

## 8. Failure, cancellation, restart, and contention semantics

Attempt counting remains idempotent by preflight identity. A positive provider aggregate consumes one start after an exception; a verified empty aggregate does not; unavailable admission count blocks later starts. Existing 300-second inactivity cancellation, 10-GiB runtime disk floor, and 3,600-second hard wall stop remain controlling. The external supervisor continues sampling disk independently.

## 9. Authorization and resource limits

Use only `opencode_go/muse-spark-1.3-contributor`. Existing authorization covers up to eleven further starts in the current 40-start supplemental grant, until H007 qualifies or the grant is exhausted. Keep the per-run limits unchanged: 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds hard wall time, 100 follow-ups, 300-second inactivity cancellation, 10-GiB runtime floor, 30-GiB preflight reserve. External billed USD remains `unknown` when unreported. Do not start after grant exhaustion, failed exact-head identity checks, unresolved request accounting, or a preflight reserve below 30 GiB.

## 10. Acceptance criteria

- The safe diagnostic identifies at least one validated in-repository function frame for an exceptional H007 runner exit when such a frame exists.
- The frame projection is capped at eight entries and retains no path, line, locals, traceback serialization, exception message, or private run data.
- Invalid frame data projects to unavailable rather than being copied through.
- Positive request admissions remain reconciled once, and no normal private receipt is synthesized on exception.
- Focused offline verification passes.
- H007 remains unqualified unless a later attempt passes the unchanged horizon, lifecycle, continuation, repair, job, review, and independent-verifier requirements.

## 11. Stop conditions

Stop before live admission if the safe ledger is inconsistent, an unresolved admission marker exists, exact-head ARM64 binaries do not match, the task/verifier/route/limits drift, or free disk is below the 30-GiB reserve. Stop the run on the existing 10-GiB runtime floor, inactivity cancellation, or 3,600-second hard limit.

## 12. Handoff

Attempt 73 consumed start 73: 73/200 combined, 29/40 in grant `s019-supplemental-20261111-04`, and 44/120 task-authorized starts. Eleven starts remain in the current grant. Attempt 74 must use the same Muse route, task, verifier, and controls after this correction is verified and a new exact-head ARM64 pair and preflight are available. H007 remains unqualified.
