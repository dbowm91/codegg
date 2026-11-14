# Harness Lab Self-Improvement Resumption S054 — Provider Attempt Exception Reconciliation

Status: **active; exception reconciliation is implemented and offline-verified; attempt 73 remains**

Repository baseline: `b4f927cfc324049498afdbc23aee8687cf625d01`

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#48-s054--provider-attempt-exception-reconciliation`

Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/003-planning-process.md` §§6–7.

Applicable ADRs: ADR-0040, evidence-based agent recovery and termination; ADR-0051, delegated-run continuation authority.

Primary class: infrastructure

## 1. Objective

When the H007 autonomy runner exits exceptionally after admitting provider requests, preserve the safe request count, reconcile exactly one authorized start, and write bounded failure evidence without retaining exception text or private run contents. If the provider-admission count cannot be determined, mark that attempt unresolved and refuse another campaign start until accounting is reconciled.

## 2. Evidence and reason for this corrective

Attempt 72 passed exact-head preflight for the unchanged v97 task, verifier, Muse route, caps, and ARM64 binary pair. The runner exited with code 1 after 3,306.180 seconds; no external disk or wall stop fired. It did not write the private receipt or normal safe campaign diagnostic. The campaign ledger therefore had no attempt row even though the safe provider-boundary admission counter returned 66 requests. This count is below the unchanged 1,200-request ceiling and is sufficient to reconcile the consumed start. Minimum observed free disk was 47,693,434,880 bytes. External billed USD is unknown.

Attempt 72's task outcome, transcript/event measurements, continuation state, verifier outcome, and underlying exception category are unavailable. No private receipt, runner output, raw provider content, transcript, tool arguments/results, or verifier material was inspected. The evidence proves an accounting and safe-observation gap after provider admission; it does not establish the underlying runner failure cause or a model, daemon, scheduler, ACP, tool, or verifier defect.

## 3. Invariants

- Keep the v97 task, case manifest, independent verifier, Muse route, tools, request/tool/time/disk/inactivity/follow-up caps, and qualification gates unchanged.
- Count starts only from the bounded provider-boundary request aggregate. A missing run row is not proof that no provider request occurred.
- On an exceptional runner exit, retain only a fixed failure category, request count and source, exit classification, authorization identity, and explicit unavailable states.
- Never retain exception text, traceback, private receipt contents, transcript, tool payloads, verifier source/output, continuation body, credential value, or job IDs.
- An unavailable or malformed request aggregate is unresolved, not zero. Block later preflights until that attempt is reconciled.
- The independent verifier remains authoritative. Exception evidence can never qualify H007.

## 4. Required production changes

Add a narrow H007 exception boundary around the autonomy suite call. Map caught exception types into a fixed safe category set and discard all messages. Read the per-case provider admission file only through the strict provider-boundary JSONL aggregate counter, with a fixed source-size ceiling; missing, oversized, unreadable, non-JSON, unknown-kind, or malformed input is unavailable rather than zero.

- Positive count: write a safe exception diagnostic and idempotently reconcile the attempt and request total into the authorization ledger as unqualified.
- Zero count from a readable valid empty admission file: write a safe no-admission diagnostic and leave campaign/task/grant start counters unchanged.
- Count unavailable or malformed, including a missing admission file after an exception: persist one safe unresolved-attempt marker, leave counters unchanged, and make subsequent preflight fail closed until that attempt is reconciled.

Do not synthesize or write a normal private run receipt when `run_suite` failed to return one.

Implementation: `dev/harness_lab/__main__.py` catches exceptions only for the campaign-accounted H007 lane, projects a fixed exception class, counts provider admissions from the existing per-case JSONL receipt, and writes safe evidence without creating the private receipt. `dev/harness_lab/autonomy_run.py` caps aggregate-source reads at 16 MiB and maps oversized, invalid UTF-8, missing, non-JSON, unknown-kind, and malformed receipt sources to unavailable on this path. `campaign_authorization.py` stores identity-only unresolved markers, blocks preflight while one remains, and clears a matching marker only after positive request-count reconciliation.

## 5. Ordered work packages

1. Add safe exception-category projection and admission-count reconciliation for an H007 runner exception.
2. Add unresolved-admission state with duplicate protection, fail-closed preflight, and resolution when a positive count is later reconciled.
3. Add production-shaped tests for exception after positive admissions, zero admissions, unknown/malformed admission count, no raw message retention, idempotent accounting, and blocked follow-on preflight.
4. Run the focused CLI/campaign/request-counter tests, both Python test-quality guards, Python compilation, and `git diff --check`.
5. Preserve attempt 72 as unqualified with 66 requests; do not change its task/verifier result to a guessed failure.
6. Rebuild the exact-head ARM64 CodeGG/helper pair, perform a fresh preflight, then run attempt 73 under the unchanged v97 controls. Use the post-run safe campaign diagnostic for start accounting and retain the external disk/wall supervisor.

## 6. Acceptance criteria

- Any positive safe provider-admission count is reconciled once even if the autonomy call raises before returning a receipt.
- A verified zero count remains unconsumed; a missing or unavailable count blocks the next campaign preflight.
- Safe diagnostics expose no raw exception or private run contents.
- Focused offline verification passes.
- H007 remains unqualified unless a later attempt completes the original horizon, lifecycle, continuation, repair, job, review, and unchanged verifier gates.

Offline verification on 2026-11-14: the focused CLI, campaign authorization, and autonomy-run modules passed 112 tests; Python compilation, both test-quality guards, and `git diff --check` passed. This qualifies the accounting correction offline only. It does not qualify H007 or explain attempt 72's underlying runner exception.

## 7. Stop conditions

Do not launch a subsequent live attempt if request admission remains unresolved, the ledger and attempt identity disagree, the task/verifier/manifest/route/caps changed, either exact-head binary hash or architecture is wrong, or free disk is below the 30-GiB reserve.

## 8. Handoff

Attempt 72's 66 provider requests consume one start: 72/200 campaign, 28/40 in grant `s019-supplemental-20261111-04`, and 43/120 task-authorized starts. Twelve starts remain in that grant and 77 in the task authorization. Attempt 73 follows this corrective and a fresh exact-head preflight. H007 remains unqualified.
