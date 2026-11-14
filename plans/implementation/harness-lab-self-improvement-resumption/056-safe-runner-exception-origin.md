# Harness Lab Self-Improvement Resumption S056 — Safe Runner Exception Origin

Status: **active**

Repository baseline: `e94435fb1ad82c92e6bebd1bd2adcfee178f0e4b`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#50-s056--safe-runner-exception-origin`

Long-term requirements:

- `plans/000-long-term-specification.md` §§23–24, 26–29
- `plans/003-planning-process.md` §§6–7

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`
- `plans/adrs/ADR-0051-durable-delegated-run-transcript-and-continuation-authority.md`

Primary class: infrastructure

## 1. Objective

Make an H007 runner exception diagnosable at the bounded CodeGG call site and safe exception category, while retaining no exception message, file path, stack local, transcript, provider content, or verifier material.

## 2. Why this milestone is ready

S055's exact-head attempt 74 confirmed that safe function telemetry works: the traceback projected `autonomy_run.run_suite` and `run_case`. The run still exited with `unexpected_exception`, and the captured function names span too much code to locate the failing operation. The exact source line and exception type were not collected by S055, so its safe evidence cannot recover them retroactively. The task and verifier remain unchanged and a current bounded Muse authorization remains available.

## 3. Current implementation evidence

`dev/harness_lab/__main__.py` catches an H007 `run_suite` exception and records a fixed failure class plus up to eight validated CodeGG function labels. `campaign_authorization.safe_campaign_run_fields` independently sanitizes that projection. Attempt 74's safe record shows two frames, but not their source lines or the exception category. S056 extends only this exception diagnostic.

## 4. Invariants that must not regress

- Keep the v97 task, independent verifier, manifest, model, route, request/time/native-call/disk/inactivity/follow-up caps, and qualification gates unchanged.
- Keep exception messages, traceback serialization, paths, locals, provider content, transcripts, tool payloads, verifier text, credentials, and job IDs out of the projection.
- Permit source line numbers only for validated in-repository `dev.harness_lab` frames, bounded to positive integers and tied to the recorded source head.
- Permit exception types only from a fixed safe allowlist; map unrecognized/custom types to `non_builtin_exception`.
- Keep the existing function-label projection and campaign sanitizer as independent validation layers.
- An exceptional run remains unqualified even when safe source context is available.

## 5. Scope

### In scope

- Add a bounded exception-type and source-line context at the H007 exception boundary.
- Sanitize the new context independently in `campaign_authorization.py`.
- Add production-shaped tests proving the projected CodeGG call site, exception allowlisting, and rejection of paths, locals, messages, malformed lines, test frames, and arbitrary custom type names.
- Record attempt 74, close S055 at function-site scope, and perform an exact-head attempt 75 with the unchanged live workload and controls.

### Explicitly out of scope

- Changes to the v97 workload, hidden verifier, task phases, model/provider route, run caps, or qualification criteria.
- Changes to daemon, scheduler, ACP, provider, job, or tool-surface architecture without a reproducible finding from the safe source location.
- Capturing raw exception text, arbitrary exception class names, traceback data, paths, or verifier output.

## 6. Required production changes

### Runtime and safe projection

At the existing exception boundary, retain a bounded list of at most eight `{site, line}` records from validated in-repository frames. Keep `site` in the existing `dev.harness_lab.module:function` form; accept only integer lines in `[1, 1_000_000]`. Project a fixed exception type label from the built-in allowlist or known CodeGG harness error classes, mapping every other type to `non_builtin_exception`. The source commit is recorded by the external monitor, anchoring line interpretation.

At the campaign serialization boundary, revalidate the exact location keys, function-label grammar, positive bounded line integers, and exception-type allowlist. Drop unknown fields and fail closed when all locations are invalid.

### Documentation and static guards

Reconcile attempt 74's request count, source frames, timing, disk measurements, and authorization counters. Close S055 only at its achieved function-site scope. Register S056 active, keep H007 unqualified, and keep all current grant/task counts explicit.

## 7. Ordered work packages

### Work package A — Safe exception context

Intent: Narrow the exceptional runner region to a safe CodeGG call-site and useful exception category.

Required changes: Add source-line and allowlisted type projection; add independent campaign validation; preserve S055's existing function labels.

Acceptance evidence: Tests show a production CodeGG frame and positive bounded source line, recognize an allowed built-in/CodeGG type, map custom types to the fixed fallback, and reject malformed/path-bearing/context-bearing inputs.

### Work package B — Reconcile S055 and record attempt 74

Intent: Close the telemetry scope S055 actually achieved and leave an auditable attempt record.

Required changes: Add safe attempt 74 evidence; update the authorization ledger, S019 status, roadmap, and registry; add S055 closure and this plan.

Acceptance evidence: The safe aggregate and campaign row both report 233 requests; attempt 74 remains unqualified; counters read 74/200 combined, 30/40 grant, and 45/120 task starts, with 10 grant and 75 task starts remaining.

### Work package C — Offline verification and live reevaluation

Intent: Validate the source-context sanitizer before using it on the unchanged H007 workload.

Required changes: Run the focused Python tests, test-quality guards, Python compilation, and diff checks. Push the corrective, build/verify the exact-head ARM64 pair, perform fresh preflight, and execute attempt 75 with the current Muse authorization.

Acceptance evidence: Offline checks pass; exact route, task, verifier, limits, and binaries pass preflight; attempt 75 reports the safe context or a normal completion projection. H007 qualifies only when every original duration, active-work, lifecycle, continuation, repair, review, and independent-verifier gate passes.

## 8. Failure, cancellation, restart, and contention semantics

Campaign start accounting remains idempotent by preflight identity. Reconcile positive request admissions once from the bounded provider-boundary source; a verified empty aggregate consumes no start; a missing aggregate leaves a fail-closed unresolved marker. Preserve the 300-second inactivity cancellation, 10-GiB runtime floor, 30-GiB preflight reserve, and 3,600-second external hard stop.

## 9. Compatibility and migration

The addition is an optional safe diagnostic field. Existing campaign evidence and old records without it remain valid and project the context as unavailable; no schema migration is needed.

## 10. Required tests

### Focused unit tests

- Safe source location and exception-type projection.
- Independent sanitizer accepts only exact bounded fields.
- Negative inputs cover paths, test modules, line booleans/zero/oversize, arbitrary class names, and injected message/local keys.

### Integration tests

- Production-shaped H007 exception evidence includes safe source context without synthesizing a private receipt.
- Positive provider admissions still reconcile exactly once.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_cli dev.harness_lab.tests.test_campaign_authorization
python3 -m py_compile dev/harness_lab/__main__.py dev/harness_lab/campaign_authorization.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
```

## 12. Documentation updates

- S019 status, the S019 subsystem roadmap, and `plans/registry.md`.
- S055 implementation and closure records; S056 closure after its gates are met.
- Safe per-attempt preflight, diagnostic, and monitor evidence.

## 13. Acceptance criteria

- The safe diagnostic narrows a failure to one or more allowlisted CodeGG function/line locations when such frames exist.
- Only a fixed exception-type label is emitted; custom types collapse to the fallback label.
- Independent validation removes malformed or unapproved location data.
- No private exception/run content is retained or inspected.
- Attempt 75 uses unchanged task, verifier, route, and caps; it is marked qualified only if every original H007 gate passes.

## 14. Stop conditions

Stop before live admission if the authorization ledger is inconsistent, an unresolved admission marker exists, exact-head ARM64 binaries or identities do not match, the task/verifier/route/limits drift, or free disk falls below the 30-GiB reserve. Stop at the 10-GiB runtime floor, 300-second inactivity bound, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt-work limit, or 3,600-second hard stop.

## 15. Closure evidence required

Record exact commit/source-head and binary hashes; focused test/guard results; attempt 74 and 75 safe evidence hashes; request aggregate reconciliation; task/verifier availability and exact outcome; active-work/horizon/lifecycle gates; disk start/minimum/end; stop reason; and remaining authorization. Never infer task completion or verifier pass from runtime duration.

## 16. Handoff notes

After attempt 74, the current grant has 10 starts remaining and task authorization has 75 remaining. Use only `opencode_go/muse-spark-1.3-contributor`; retain `unknown` USD when unreported. Preserve the exact v97 manifest and hidden verifier. Do not inspect private runner output, receipt contents, transcript, raw payloads, or verifier source/output.
