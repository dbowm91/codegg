# Harness Lab Self-Improvement Resumption S047 — Per-Check Public Verifier Reconciliation

Status: **implemented; attempt 59 unqualified; S048 active**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#41-s047--per-check-public-verifier-reconciliation`

Long-term requirements: `plans/000-long-term-specification.md` §§26–29; `plans/002-long-term-roadmap.md`

Applicable ADRs: ADR-0040, evidence-based agent recovery and termination

Primary class: **diagnostic observability**

## 1. Objective

Expose the existing fixed, content-free H007 verifier/public-projection checks in campaign evidence. This will distinguish a missing public continuation field from a verifier disagreement with an otherwise-complete public projection, without retaining hidden assertion text, transcript content, job IDs, prompts, tool payloads, or verifier output.

## 2. Evidence motivating the plan

Attempt 58 used the unchanged Muse route and v97 task, ran 2,060.705 seconds with 2,039.993 active seconds, and ended normally. Its safe completion record reports all seven stages and twelve gates complete, required review observed, release overlap confirmed, all public artifacts present, and `task_outcome=failed` with verifier outcome `assertion_failures`.

The existing bounded comparison found one `continuation_boundary` verifier failure whose public projection state is `unavailable`, and one `transcript_job_lifecycle` failure whose public projection state is `complete` and classification is `verifier_public_projection_disagreement`. This rules out an early stop or unobserved run horizon, but does not identify which check made the continuation projection unavailable or what exact public lifecycle assertion disagreed.

Source inspection shows `h007_results._verifier_public_reconciliation` already emits allowlisted Boolean-or-null `public_checks` for these two domains. The campaign projector currently retains only aggregate domain count/state/classification and drops those check-level values. The next corrective owner cannot be selected precisely until the safe boundary carries that existing bounded evidence.

## 3. Invariants

- Keep H007 v97 task content, independent verifier, Muse model/route, provider cap, tool cap, prompt-active limit, wall-clock stop, inactivity cancellation, disk reserve/floor, and qualification gates unchanged.
- Do not read or retain private receipt contents, hidden verifier output, assertion text, prompts, transcript, raw job identifiers, tool arguments/results, credentials, or raw error strings.
- Project only fixed check names and Boolean or null values already produced by the public diagnostic summarizer. Unknown names and non-Boolean values are dropped.
- A missing check remains `null`/unavailable; it never becomes `true`, a task pass, or verifier pass.
- Preserve attempts 56–58 and all authorization counters. Each provider-admitted retry consumes one task, grant, and campaign start.

## 4. Scope

### In scope

- Add a fixed allowlist for the `continuation_boundary` and `transcript_job_lifecycle` public-check names emitted by `_verifier_public_reconciliation`.
- Carry bounded check-level Boolean/null values into the S019 safe campaign diagnostic, nested under their existing failure-domain names.
- Add unit and production-shaped CLI tests for false, true, null, unknown keys, private strings, collection bounds, and the suite-envelope/per-case-row path.
- Re-run the existing offline diagnostic/privacy checks and, after a fresh 30-GiB disk reserve preflight, use the next authorized Muse start with unchanged task and controls.

### Out of scope

- Changing verifier assertions, task wording, stage/gate definitions, public artifacts, tool lifecycle architecture, model route, budgets, or cancellation behavior.
- Reopening daemon, scheduler, ACP, provider, or job architecture absent new independent evidence.
- Treating a complete horizon or public task document as a successful verifier result.

## 5. Required changes

Project `public_checks` through two exact per-domain allowlists. Each domain output is a map from one of its known fixed check names to `true`, `false`, or `null`; cap its size at the allowlist length. Reject booleans masquerading as integers and reject all strings, lists, and nested objects. Keep the existing aggregate failure-domain projection alongside the new map.

The campaign wrapper must continue selecting the exact H007 case row from the suite's `runs` list, and canonical launch identity must remain sourced exclusively from preflight. Keep the task-level and verifier outcomes separate.

## 6. Ordered work packages

1. Define and test the public check-name allowlists in `campaign_authorization.py`.
2. Extend `_safe_h007_completion_projection` to retain only per-domain Boolean/null checks from the already-safe case diagnostic.
3. Add hostile-field and production-shaped CLI regressions; verify no prompt, verifier text, IDs, or arbitrary keys escape.
4. Run campaign/CLI tests, the autonomy/verifier/H007 suite, test-quality guards, Python compilation, and `git diff --check`.
5. Verify the exact suite/binary identity, separate authorization headroom, and free disk ≥30 GiB. If below reserve, continue independent source/plan work but do not start a provider session.
6. Run the next unchanged Muse H007 attempt under the existing 1,200-request/3,600-second/1,600-tool/3,300-active-second/300-second inactivity/10-GiB runtime-floor controls. Inspect only preflight, safe campaign evidence, bounded trace counters/types/elapsed times, and free-space measurements.
7. Assign the next corrective from the per-check evidence: incomplete public checks go to the relevant task/evidence contract; all-complete checks with verifier failure remain a bounded projection/verifier disagreement for separate review. Do not change the verifier in this plan.

## 7. Acceptance criteria

- Campaign safe evidence identifies each applicable public check as true, false, null, or absent under a fixed known name.
- Injected private strings, unknown keys, and wrong value types are absent from the projection.
- The exact selected case row and canonical preflight identity reach the campaign evidence.
- The next live attempt records populated check-level diagnostics or a safe explicit unavailable state; its H007 result remains unqualified unless every existing independent-verifier and horizon gate passes.

## 8. Failure and stop conditions

- Refuse before provider admission on an identity mismatch, authorization exhaustion, evidence path collision, unavailable required telemetry, or free disk below 30 GiB.
- Cancel under the existing hard wall, inactivity, request cap, or 10-GiB runtime floor.
- Stop this plan if the check projection requires raw/private fields or changes the verifier/task contract; register a separate plan for such a change.

## 9. Verification commands

```bash
python3 -m unittest -v dev.harness_lab.tests.test_campaign_authorization dev.harness_lab.tests.test_cli
python3 -m unittest -v dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics dev.harness_lab.tests.test_h007
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/campaign_authorization.py dev/harness_lab/__main__.py dev/harness_lab/tests/test_campaign_authorization.py dev/harness_lab/tests/test_cli.py
git diff --check
```

## 10. Closure evidence

Record exact offline checks, the next attempt's preflight and safe diagnostic, all three counter scopes, observed disk floor, and a qualification decision. Preserve private receipts and raw verifier output without inspecting them.

Attempt 59 populated the fixed per-check projection. H007 remains unqualified; S048 owns the remaining safe count and timeout-boundary detail.
