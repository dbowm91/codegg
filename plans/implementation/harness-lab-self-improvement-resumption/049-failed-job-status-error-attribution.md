# Harness Lab Self-Improvement Resumption S049 — Failed Job-Status Error Attribution

Status: **active**

Repository baseline: `7bc59352a0c7a337f442bea5aa64947315042712`

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#43-s049--failed-job-status-error-attribution`

Long-term requirements:

- `plans/000-long-term-specification.md` §§26–29
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: **diagnostic observability**

## 1. Objective

Preserve one fixed, content-free error class for a failed native `job status` call so the next Muse run can distinguish a missing/foreign/expired job ID from a generic tool failure or an unclassified result. Attempt 60 recorded one unbound `status:string:failed` call among 167 applicable job calls, with 166 request/result IDs matched and one unobserved. The private result text is intentionally not inspected.

## 2. Why this milestone is ready

Attempt 60 completed 3,299.655 seconds with 3,288.685 seconds of active work, 390/1,200 provider requests, and exit code 2 at the prompt-work ceiling. Event measurement was complete and the serialized transcript projection matched the source. The public trace identifies exactly one failed job-status call with a string-shaped ID argument but no result ID. The attempt cannot identify its cause from current safe output because job-status error text is not projected to a fixed error class.

The same trace shows `catalog_format.failed_job_status_called_immediately=false` while the `catalog_store` repair group passed. This is evidence that the agent did not follow the required immediate status sequence for one seed job. S049 must preserve that observed predicate and must not reclassify it as a daemon or ACP lifecycle defect.

## 3. Invariants

- Keep H007 v97 fixture text, hidden verifier, route, and qualification criteria unchanged.
- Use only `opencode_go/muse-spark-1.3-contributor`.
- Keep the 1,200-provider-request cap, 1,600-native-call cap, 3,600-second hard wall, 3,300-second prompt-work ceiling, 300-second verifier reserve, 300-second inactivity cancellation, 10-GiB runtime floor, and 30-GiB preflight reserve.
- Never persist job IDs, argument text/digests, raw tool output, prompt/transcript content, verifier assertions, credentials, private receipt, or runner log.
- The new output is an allowlisted class histogram for failed `job status` calls only. Unknown text becomes `unclassified`; missing classification remains `unobserved`.
- Preserve every admitted or refused attempt. USD remains unknown if unreported.

## 4. Scope

### In scope

- Classify failed `job status` tool output using a fixed allowlist, including `job_not_found`, known generic tool-error classes, `unclassified`, and `unobserved`.
- Add bounded counts to the safe job-sequence projection and H007 public verifier reconciliation.
- Add privacy tests for recognized, unknown, malformed, missing, and hostile outputs; assert the output text and IDs do not survive.
- Run the unchanged v97 task once as attempt 61 after the focused offline checks and a fresh 30-GiB preflight.

### Explicitly out of scope

- Changing CodeGG job semantics, daemon, scheduler, ACP event lifecycle, provider integration, model, task prompt, verifier, request/time limits, cancellation behavior, or repair qualification predicates.
- Treating the failed immediate-status predicate as an instrumentation failure.
- Inspecting attempt 60's private receipt, runner log, raw transcript, arguments, or output.

## 5. Required implementation

Add a pure classifier over the already delivered ACP `rawOutput` that returns only fixed class names for failed `job`/`status` calls. Prefer the exact known `job not found` tool error before generic error-prefix classes. The projector records no raw string and does not classify successful calls. The diagnostic aggregates one count per fixed class, bounded by the existing job-call limit. The safe H007 reconciliation copies only known classes and bounded positive integer counts; unknown names, private strings, booleans-as-counts, and oversized values are discarded.

## 6. Acceptance evidence

- A production-shaped ACP notification sequence for a failed `job status` call yields a safe class and retains neither its output nor its job ID.
- Unknown output maps to `unclassified`; missing class maps to `unobserved`.
- Safe sequence and campaign projections preserve only allowlisted classes and bounded counts.
- Focused Python tests, relevant assertion/duplication guards, Python compilation, and `git diff --check` pass.
- Attempt 61 uses the unchanged v97 manifest, binary/helper identity, Muse route, and all recorded caps. Its safe result determines whether S050 is needed. H007 remains unqualified unless the unchanged independent verifier and every existing requirement pass.

## 7. Ordered work packages

### A — Safe failed-status classifier

Add a pure fixed-output classifier and capture its result only for failed job-status calls.

### B — Bounded projection

Carry fixed class counts through the job sequence and public campaign projection. Add tests for allowlisting, count bounds, unknown names, private markers, and missing instrumentation.

### C — Offline verification and live attempt

Run the focused tests and guards; verify the case/binary identity, active grant, credential availability without reading or emitting its value, and fresh disk reserve. Launch attempt 61 only after all preflight checks pass.

## 8. Stop conditions

- Stop if the classifier requires storing raw tool output or argument content.
- Stop if the live preflight is below 30 GiB, grant/task authorization is exhausted, or any binary/manifest identity differs.
- Stop and preserve evidence if the provider request, wall, inactivity, tool-call, runtime-disk, or cancellation cap fires.
- Do not declare qualification from duration, stage counts, or partial verifier success.

## 9. Closure evidence required

Record test results, attempt 61 preflight/safe-evidence paths, route/model, campaign/grant/task counters, provider requests, terminal cause, public error-class counts, minimum sampled free disk, and the qualification decision. State explicitly that private raw artifacts were not inspected.
