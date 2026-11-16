# Harness Lab Self-Improvement Resumption Milestone S025 — Domain-Scoped Verifier Diagnostics

Status: **implemented; attempt 32 completed unqualified; S026 active**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` with the authorized S019–S024 work in the shared worktree.

Source roadmap:

- `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md` §19

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None; this is a bounded, privacy-preserving operator diagnostic.

Primary class: **reliability**

## 1. Objective

Make H007 independent-verifier failures actionable when the public continuation and job projections appear complete, without reading or retaining hidden test identities, assertion text, raw verifier output, task results, transcripts, job IDs, or argument values. Use that telemetry in one unchanged Muse run to determine whether the remaining failures identify a task defect or a harness evidence gap.

## 2. Evidence and diagnosis

Attempt 31 used the unchanged v86 task on `opencode_go/muse-spark-1.3-contributor`. It ran for 1,940,553 ms with 1,933,992 ms active work and crossed the 30-minute active-work gate. The exact-ID job lifecycle, productive overlap, two required repair pairs, three observed/documented repair cycles, all continuation stages and gates, reviewer lifecycle, event measurement, and resource limits were satisfied. The agent ended normally. The complete trace recorded 1,447 events, 679/679 paired tool calls, no event gaps, and three correlated repair cycles. All ten test jobs reached terminal states. Lowest free disk was 165,652,250,624 bytes, above the 10-GiB floor.

The task still failed. The unchanged independent verifier passed 2/5 checks. Its safe domains were `continuation_boundary: 1`, `task_behavior: 1`, and `transcript_job_lifecycle: 1`. Public projections marked continuation and transcript/job lifecycle complete, so the existing reconciliation classifies those two as verifier/public-projection disagreements. `task_behavior` is currently unmapped to a public task diagnostic. The current receipt therefore proves a task/verifier mismatch but does not identify the failed public invariant. Provider transport completed; four provider attempts failed before retry/success, with 639 admitted requests total. No inactivity or disk stop occurred. USD 0.006132 was reported as OpenCode Go allowance; billed USD remains unknown.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt31-safe-diagnostic-20261110.json`; attempt receipt SHA-256 is recorded there. The run's detailed private diagnostic is not a reason to inspect hidden verifier identities or assertions.

## 3. Scope

### In scope

1. Extend the verifier diagnostic with an aggregate summary per coarse failure domain: failure count, assertion-operation category, exception class, safe message category, and counts of already-allowlisted diagnostic terms.
2. Keep the summary independent of hidden test IDs and values: do not emit names, text, paths, values, raw output, stack frames, or per-test rows in this new field.
3. Version the diagnostic schema and add synthetic regressions covering domain separation, aggregation, and redaction.
4. Run focused offline verification and then reattempt the unchanged v86 task with the existing Muse route and per-run caps. Use only the new aggregate receipt fields to classify any remaining verifier failure.

### Explicitly out of scope

- Reading, invoking, weakening, or modifying the hidden verifier or its acceptance criteria.
- Inspecting hidden test identifiers, assertion text, raw verifier output, provider logs, task result bodies, transcripts, raw job IDs, or tool arguments.
- Changing the Rust task, public contract, prompt, fixture, model/provider route, daemon, scheduler, ACP protocol, request cap, or paired binaries.
- Treating a horizon pass as task success or a verifier failure as a harness defect without causal evidence.
- Adding artificial delays, another test corpus, or broad harness redesign.

## 4. Invariants

- Use only `opencode_go/muse-spark-1.3-contributor` and the current paired ARM64 binaries.
- Preserve the 1,200-request, 1,600-native-call, 3,600-second wall, 3,300-second prompt-work, 300-second inactivity, 10-GiB disk-floor, and 100-follow-up limits. Do not add a USD cap; report provider allowance and report billed USD as `unknown` when unavailable.
- Keep fixture and task criteria unchanged. The live attempt must use the exact v86 manifest and verifier identity already qualified in S024 preflight.
- Do not expose hidden test IDs or assertion content. New telemetry must aggregate only existing allowlisted categories and terms, omit values, and retain no per-test detail.
- Preserve separate task, verifier, and horizon outcomes. Do not call the task qualified unless every unchanged verifier check and horizon requirement passes.
- Preserve all prior failed attempts and authorization records; the next invocation consumes one start from the already-authorized grant.

## 5. Ordered work packages

### A — Preserve attempt 31

Record the safe receipt projection, digest, v86 and binary/verifier identities, actual model route, request and resource measurements, horizon evidence, repair-count reconciliation, verifier domain counts, and the current 31/40 supplemental starts remaining and 88 combined starts remaining. Update S019, the S024 closure, roadmap, and registry. Do not store protected payloads.

### B — Add domain-scoped safe diagnostic aggregation

In `dev/harness_lab/diagnostics.py`, aggregate assertion operation, exception class, safe message category, and allowlisted term counts under the already-coarse failure-domain keys. Do not include hidden test IDs, paths, file/line data, exact values, message shapes, or text. Bump the diagnostic schema and add synthetic tests proving domain separation, stable counts, and redaction.

### C — Verify offline

Run focused verifier-diagnostic and H007 result tests, relevant assertion-quality and duplication guards, Python compilation, and `git diff --check`. Confirm the new telemetry is absent for passed suites and bounded for failures. No Rust binary rebuild is needed.

### D — Reattempt 32

Recheck the existing authorization route, credential presence without exposing its value, private lane, fixture/verifier/binary identities, request budget, and disk reserve. Run the exact v86 case under the unchanged hard caps. Monitor request admissions, active duration, paired events, exact-ID jobs, reviewer state, repair cycles, disk, and inactivity. After terminal state, inspect only the safe receipt projection including the new aggregated failure summary.

## 6. Acceptance criteria

- Synthetic tests prove the new per-domain summary distinguishes the three coarse domains while excluding hidden test IDs, assertion text, values, paths, and per-test rows.
- Focused offline verification passes and the v86 fixture/verifier/task identity remains unchanged.
- Attempt 32 is recorded with separate task outcome, verifier outcome, and horizon outcome. If any verifier domain remains failed, the safe aggregate must be sufficient to identify the relevant kind of public contract mismatch; otherwise create the next narrow corrective from that evidence.
- H007 qualifies only if task completion, all five unchanged verifier checks, active-duration requirements, exact-ID overlap, two repair pairs, continuation, reviewer lifecycle, complete event/job evidence, and every resource limit pass.
- All caps remain respected. Any task/verifier/horizon failure consumes the start, is preserved as unqualified evidence, and drives a narrow evidence-backed next plan while authorization remains.

## 7. Stop conditions and authorization

- Do not start unless the Muse route is available, the existing credential entry is present, fixture and paired-binary/verifier identities match, budget admission succeeds, and the preflight disk reserve is at least 30 GiB.
- Stop on user cancellation, 10-GiB disk floor, 300-second inactivity limit, 1,200 provider requests, 1,600 native calls, 3,300 seconds prompt work, 3,600 seconds wall time, or 100 follow-ups.
- The existing user authorization permits up to 40 additional Muse starts. Attempt 31 consumed start 9/40; attempt 32 passed preflight and its start is counted as 10/40, leaving 30 supplemental starts. The combined ledger is 33/120 starts consumed, with 87 remaining.

## 8. Closure evidence

Record the exact changed files and tests, schema version, synthetic privacy assertions, unchanged v86/verifier identity, preflight and authorization counts, safe receipt digest, model route, separate task/verifier/horizon outcomes, provider allowance or `unknown`, event/job/repair/reviewer telemetry, provider attempt counts, resource minima, and next owner if unqualified. Do not store credentials, raw logs, transcript, task result body, hidden test identities or assertions, raw IDs, or arguments.

## 9. Attempt 32 result

Attempt 32 used the unchanged v86 workload on the authorized Muse route and ended normally after 1,904,940 ms total and 1,894,823 ms active work. It crossed the 30-minute gate and satisfied measured horizon conditions: all public stages and gates complete; three observed and documented repair cycles; both named required pairs qualified; exact-ID productive overlap; all 18 submitted test jobs terminal; and reviewer lifecycle complete.

The independent verifier passed 3/5 checks, so the task remains unqualified. The remaining safe domains were one `continuation_boundary` and one `transcript_job_lifecycle` assertion. Both were pattern operations with a redacted `assertion_message` category and the same aggregate allowlisted term set. The public continuation and lifecycle projections were complete. S025 improved the diagnostic from domain-only counts to a bounded summary, but the exact public contract mismatch is still not established; S026 directs a public sequence audit through the next model run.

The trace had 981 rows, 436/436 paired tool calls, and no explicit event gaps or drops. The maximum gap between native tool events was 240,305 ms. Late repair-cycle markers are appended with their original occurrence times; do not calculate a native activity gap by comparing those backfilled markers with controller markers. Provider requests were 379/1,200, with 379 attempts started and succeeded and zero failed. Provider allowance was USD 0.014992; billed USD remains unknown. Lowest free disk was 151,965,368,320 bytes; the 10-GiB floor did not trigger.

Safe evidence: `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt32-safe-diagnostic-20261110.json`; private receipt SHA-256 `12e670619a4916bb281f413f9c1c08586babddf51daefb3add7d270c3eb14fa6`.
