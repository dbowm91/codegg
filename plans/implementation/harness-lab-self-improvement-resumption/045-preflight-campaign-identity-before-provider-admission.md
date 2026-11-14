# Harness Lab Self-Improvement Resumption S045 — Preflight Campaign Identity Before Provider Admission

Status: **closed at pre-provider identity-gate scope**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#39-s045--preflight-campaign-identity-before-provider-admission`

Long-term requirements: `plans/000-long-term-specification.md` §§26–29; `plans/002-long-term-roadmap.md`

Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: **invariant**

## 1. Objective

Reject an incomplete or inconsistent campaign preflight before the first provider request. The exact run identity used for campaign reconciliation must be validated before launch, so a malformed evidence record cannot consume requests and then stop during post-admission accounting.

## 2. Why this milestone is ready

Attempt 55 was admitted for two provider requests, then the safe monitor stopped it with `campaign_ledger_reconciliation_error`. Its preflight contained `attempt`, grant, route, model, fixture manifest digest, and binary digest, but omitted canonical `start_number` and `manifest_sha256` fields required by `dev/harness_lab/campaign_authorization.py`. Passing the recorded preflight directly to `reconcile_campaign_run_start` reproduces `CampaignLedgerError: campaign run identity is incomplete`. The wrapper’s inner exception was not observed; this source-level reproduction matches the safe stop classification.

Attempt 55 is preserved as an unqualified, provider-started run: two requests were observed; its grant and campaign counters were reconciled to 11/40 and 55/200. The active task authorization is a separate counter, now 26/120 consumed. Do not merge these accounting scopes or reuse attempt 55 for another provider session.

## 3. Current implementation evidence

- `dev/harness_lab/campaign_authorization.py` defines the canonical identity fields and idempotent counted-start reconciliation.
- `dev/harness_lab/tests/test_campaign_authorization.py` covers post-start reconciliation, but not validating the same identity before provider admission.
- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt55-preflight-20261112.json` preserves the preflight as recorded.
- `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt55-safe-diagnostic-20261112.json` records the provider-started failure using safe monitor data only.

## 4. Invariants that must not regress

- A preflight refusal happens before any provider request and mutates no authorization ledger.
- The canonical identity includes `attempt`, `start_number`, `authorization_grant_id`, `model`, `route`, `manifest_sha256`, and `binary_sha256`.
- The `fixture_manifest_sha256` evidence field may be retained for compatibility, but it must equal the canonical `manifest_sha256` used for campaign admission.
- A provider-admitted run consumes one start even when it fails immediately. Reconciliation is idempotent for that attempt and never rewrites another row.
- Task-level 120-start authorization, lifetime campaign authorization, and supplemental-grant counts remain distinct and are reconciled independently.
- H007 task, verifier, provider route/model, request/time/disk/inactivity limits, and cancellation behavior remain unchanged.
- Never retain or print credentials, prompt/transcript content, tool arguments/results, raw runner logs, or hidden verifier output.

## 5. Scope

### In scope

- Add a pure preflight validator that checks canonical run identity and campaign/grant headroom against a copied ledger without mutation.
- Add a canonical identity constructor or normalization boundary for preflight records so required names cannot drift (`manifest_sha256` versus `fixture_manifest_sha256`).
- Wire the preflight validator into the H007 live launch path before the child process can contact a provider.
- Add a regression proving missing identity fields fail before launch and leave both ledgers unchanged.
- Run one new, separately counted attempt only after preflight passes; do not retry attempt 55.

### Explicitly out of scope

- Changing task content, fixture/verifier, route/model, provider caps, task authorization, campaign authorization, or inactivity/disk cancellation limits.
- Inferring attempt 55's hidden task outcome from private artifacts.
- Reopening ACP, scheduler, daemon, reviewer recovery, or request-cap architecture.

## 6. Required production changes

Implement the pure validator next to `reconcile_campaign_run_start`. It should call the existing reconciliation logic against a deep copy with a one-request admission probe, return the predicted start/grant/campaign counts, and leave the caller’s ledger untouched. The live launch path must run it with the exact immutable preflight identity before spawning CodeGG or contacting the provider. After the first provider request, the existing reconciler records the real observed request count and the safe run outcome.

Do not make the post-request reconciler silently repair absent identity fields. A malformed preflight remains a pre-admission refusal with a safe typed reason.

## 7. Ordered work packages

### Work package A — Canonical identity validation

Intent: make preflight and post-start reconciliation consume one identity contract.

Required changes: provide a side-effect-free validator; validate canonical manifest digest, expected start number, active grant/model/route, request ceiling, and remaining campaign/grant capacity. Preserve any manifest alias only when it exactly matches the canonical digest.

Acceptance evidence: valid preflight returns the expected next start and balances without modifying input; missing or conflicting identity, wrong start number, inactive/mismatched grant, or exhausted cap refuses without mutation.

### Work package B — Pre-provider launch gate

Intent: move identity failure ahead of provider spend.

Required changes: invoke the validator before creating the live subprocess or admitting the first provider request; write a bounded safe preflight record with canonical fields; keep post-start accounting idempotent.

Acceptance evidence: an injected missing `start_number` or `manifest_sha256` yields zero provider requests, zero tool starts, and unchanged authorization counters. A valid run records one start after its first observed request.

### Work package C — New authorized qualification start

Intent: continue H007 only after the corrective path is proven offline.

Required changes: use a new attempt identity (56), unchanged v97 task/verifier and Muse OpenCode Go route, 1200 request / 3600 second / 1600 native-call / 3300 active-second / 300-second inactivity / 10-GiB runtime-floor limits, and 30-GiB preflight reserve. Count the start in both the active task authorization and durable campaign/grant ledger after provider admission.

Acceptance evidence: safe preflight and campaign preview agree on start 56; no raw evidence is inspected; terminal status, duration, exact job lifecycle, repair/continuation gates, hidden-verifier pass/failure category, and resource telemetry are reconciled from safe projections.

## 8. Failure, cancellation, restart, and contention semantics

Preflight errors refuse before provider admission and do not consume a start. Once a provider request is observed, the start is consumed even if the child exits or cancellation follows. Reconciliation retries for that identity may only increase observed requests and advance status. The existing wall-clock, inactivity, request, and disk cancellation controls remain in force.

## 9. Compatibility and migration

Keep the durable campaign ledger schema and historical offset intact. Existing preflight records are immutable evidence; do not rewrite attempts 1–55 to add fields. New records use canonical identity names and may include the legacy fixture-manifest digest as a checked alias.

## 10. Required tests

### Focused unit tests

- Valid identity passes preview and does not mutate the ledger.
- Missing `start_number` and missing `manifest_sha256` each refuse.
- Conflicting manifest alias, wrong start number, wrong model/route, inactive grant, and exhausted caps refuse without mutation.
- The actual live launch path calls the preview before process/provider admission.

### Integration and failure tests

- A malformed preflight refuses with zero provider requests and zero child/tool starts.
- A valid first provider request consumes one start; repeated reconciliation for that attempt is idempotent.
- An immediately terminated provider-started run remains counted and does not overwrite prior attempts.

### Privacy checks

- Safe preflight/reconciliation evidence contains no credential values, prompts, transcript, arguments, results, raw errors, or verifier text.

## 11. Required verification commands

```bash
python3 -m unittest -v dev.harness_lab.tests.test_campaign_authorization
python3 -m unittest -v dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/campaign_authorization.py dev/harness_lab/tests/test_campaign_authorization.py
git diff --check
```

Run the focused live preflight/launch regression without provider credentials or network. Do not start another live run until all offline checks pass and free disk is at least 30 GiB.

## 12. Documentation updates

Update the S019 roadmap, registry, and closure evidence with attempt 55's consumed start and the exact preflight-field mismatch. Preserve the caveat that the private wrapper exception itself was not inspected.

## 13. Acceptance criteria

- Incomplete campaign identity is rejected before provider admission.
- Attempt 55 remains an unqualified historical start and both durable counters reconcile correctly.
- A new attempt uses a canonical preflight that passes the side-effect-free validator before provider use.
- The unchanged H007 verifier and all horizon, task, lifecycle, recovery, disk, request, and cancellation requirements determine qualification.

## 14. Stop conditions

- Stop before provider admission if the preflight validator, active authorization, or 30-GiB disk reserve fails.
- Stop if any start cannot be reconciled to exactly one active authorization grant or if task and campaign counters would be merged.
- Stop if route/model attribution, request accounting, safe diagnostics, or cancellation enforcement is unavailable.
- Stop without inspecting private logs/receipts, raw model outputs, tool arguments/results, credentials, or hidden verifier output.

## 15. Closure evidence required

- Focused test/guard results and the valid/invalid preflight preview results.
- Attempt 55 safe diagnostic and post-run campaign ledger showing 55/200 and 11/40.
- Any new run preflight and safe diagnostic, with exact binaries, manifest/verifier hashes, route/model, resource limits, and both authorization counters.
- A clear H007 qualification decision; no successful claim without the unchanged independent verifier and every required gate passing.

## 16. Handoff notes

Attempt 55 is the failed control case for the corrective. It used two provider requests and four paired tool events before stopping; disk never approached the reserve or runtime floor. The source-level campaign reconciler reproduces the incomplete-identity error when given the saved preflight. The wrapper’s private output remains uninspected, so the cause is strongly localized but not attributed to an observed inner exception.

Closure: the side-effect-free preview and pre-provider gate passed for attempts 56, 57, and 58. Attempt 56's post-run per-case digest collision remains a preserved historical row and is closed by S046's distinct canonical-identity correction; it is not rewritten here. See `plans/closure/harness-lab-self-improvement-resumption/045-status.md`.
