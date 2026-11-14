# Harness Lab Self-Improvement Resumption S046 — Preserve Campaign Identity and Safe H007 Completion Evidence

Status: **closed at safe campaign identity/completion-projection scope; H007 remains unqualified**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#40-s046--campaign-identity-and-safe-completion-evidence`

Long-term requirements: `plans/000-long-term-specification.md` §§26–29; `plans/002-long-term-roadmap.md`

Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: **invariant**

## 1. Objective

Keep the immutable launch identity authoritative during post-run accounting and make the already-produced, content-free H007 task-completion and continuation diagnostics available in the safe campaign record. A failed long run must distinguish task incompletion, public stage/gate progress, and verifier outcome without opening the private receipt, prompt, transcript, tool payloads, or verifier output.

## 2. Why this milestone is ready

Attempt 56 passed its pre-provider campaign preview and was admitted on the existing Muse route. It used 184/1,200 requests, lasted 3,299.112 seconds with 3,266.637 seconds of active work, and ended unqualified: `task_outcome=failed`, `agent_terminal=timeout`, `stop_reason=turn_timeout`, and `horizon_reason=task_not_passed`. The bounded trace showed 230 paired native tool starts/terminals, 58 job observations, four repair-cycle events, and complete event measurement. Disk stayed above the reserve. This is not evidence of an early daemon or ACP stall.

Attempt 56's safe campaign record does not identify the first incomplete public stage or gate. Attempt 57 exercised the first S046 projection, but all three nested public diagnostic states were still `unavailable`. The receipt shape explains this: `run_case` attaches `continuation_diagnostic` and `task_completion_diagnostic` to the H007 row inside `run_suite`'s outer `runs` list. The initial S046 implementation passed that outer suite envelope to a case-level projector, so the allowlisted diagnostics were not found. The corrective now passes the selected H007 row itself and tests the production-shaped envelope.

There was also a post-admission identity overwrite. Attempt 56's preflight records the launched suite manifest digest `e87faea3f5da93ae300f2ae21ec46e6ff1f899cff0fdf6c1b9e860c97ffe57ee`; the per-case row's `manifest_sha256` is the build-provenance fixture metadata digest `c703c1cb256f268500029cf0ca35faf9a8f246485afb2e3e5d98f166f15e4358`. The canonical identity precedence correction now preserves the preflight digest. Attempt 57's safe record confirms the launched digest remained `e87faea3f5da93ae300f2ae21ec46e6ff1f899cff0fdf6c1b9e860c97ffe57ee`; do not rewrite attempt 56's historical row.

Attempt 57 lasted 2,388.925 seconds (2,374.841 seconds of active work), used 171/1,200 provider requests, and ended normally with `task_outcome=failed`, `agent_terminal=completed`, `stop_reason=end_turn`, and `horizon_reason=task_not_passed`. Duration and event measurement qualified. Its safe trace recorded 218 paired tool starts/terminals, 40 job observations, four repair-cycle events, one continuation, and verifier start/terminal. It remains unqualified because the task and independent verifier did not pass. The safe completion projection was unavailable due to the receipt-envelope mismatch above, so attempt 57 does not establish the first incomplete task stage or gate.

## 3. Current implementation evidence

- `dev/harness_lab/autonomy_run.py` generates content-free continuation and task-completion diagnostics on the per-case row; `run_suite` returns those rows inside its outer `runs` envelope.
- `dev/harness_lab/campaign_authorization.py` projects one case row through explicit nested allowlists and takes campaign identity only from the immutable preflight.
- Attempt 56 and 57 preflight/safe evidence is preserved at `plans/closure/harness-lab-self-improvement-resumption/evidence/s019-attempt56-*.json` and `.../s019-attempt57-*.json`. Attempt 56's historical manifest mismatch remains unchanged; attempt 57 confirms the canonical manifest identity fix.
- Attempt 57's projection gap is covered by a CLI regression with the same outer suite envelope and nested H007 case row used in production. Attempt 58 confirms the safe task, stage/gate, verifier, and launch-identity states are populated.

## 4. Invariants that must not regress

- The validated campaign preflight is the sole source of attempt, grant, route, model, launched suite manifest, and paired binary identity after provider admission.
- A per-case fixture metadata digest cannot replace the launched suite manifest digest. Any distinct digest retained must have a field name that states its own meaning.
- Safe evidence contains only bounded enums, booleans, counts, and contract-defined public stage/gate identifiers; never note bodies, assertion text, verifier output, prompts, provider text, transcript, tool arguments/results, raw errors, or credentials.
- Missing nested diagnostics are recorded as unavailable. They never become a task pass or verifier pass.
- Provider-started attempts and all three start counters remain idempotent and distinct. Attempt 56's request count, status, and historical row are not rewritten.
- The task, fixture, independent verifier, Muse route, request ceiling, active-work limit, wall-clock cap, inactivity cancellation, runtime disk floor, and preflight disk reserve remain unchanged.

## 5. Scope

### In scope

- Make campaign safe projection accept the selected H007 case row from the suite envelope and project only explicit H007 completion and continuation fields.
- Give canonical identity precedence when composing post-run accounting records; reject or safely classify contradictory runtime identity instead of silently overwriting launch identity.
- Add offline regression tests for safe task/stage/gate/verifier projection, canonical identity precedence, missing-diagnostic behavior, and raw-field exclusion.
- Preserve attempt 56 and record the identity discrepancy in a separate correction note.
- After offline checks and the 30-GiB preflight reserve pass, use an authorized start with the same task/model/route/caps to validate the improved safe record. This acceptance was completed by attempt 58.

### Explicitly out of scope

- Changing task difficulty or wording, verifier assertions, task stages/gates, route/model, budgets, authorization limits, or request/time/disk/cancellation policy.
- Inspecting attempt 56's private receipt, hidden verifier output, prompt/transcript, tool arguments/results, raw logs, or credentials.
- Reopening daemon, scheduler, ACP, provider transport, or background-job lifecycle architecture absent new independent evidence.
- Claiming H007 qualification from elapsed time, a verifier-start event, or partial task evidence.

## 6. Required production changes

Extend the campaign projection boundary to take the case row, immutable preflight identity, and the same case row as the source of H007 diagnostics. The caller must select that row from the outer suite receipt's `runs` list by the exact H007 case ID. Project a versioned safe completion object with task-completion state/reason enums, verifier outcome and allowed failure-domain counts/classification, plus public continuation validity and required stage/gate statuses, counts, and first missing identifiers. Enforce value and collection bounds and drop all unrecognized nested keys.

When preparing the counted row, retain canonical identity from the preflight and merge only non-identity runtime fields. If a runtime per-case digest has a distinct fixture meaning, omit it from campaign identity; do not relabel it as the suite digest.

## 7. Ordered work packages

### Work package A — Canonical identity preservation

Intent: ensure the post-run row remains bound to the exact launch that passed preflight.

Required changes: provide a tested merge/projection API that prevents runtime identity fields from overriding canonical preflight fields. Keep attempt 56 unmodified and add a separate factual note describing its digest mismatch.

Acceptance evidence: a per-case `manifest_sha256` that differs from preflight cannot alter the counted row's canonical `manifest_sha256`; other identity fields are equally protected.

### Work package B — Content-free H007 completion projection

Intent: expose enough safe evidence to identify the task completion boundary.

Required changes: select the exact H007 row from `receipt["runs"]` and pass its existing `task_completion_diagnostic` and `continuation_diagnostic` through a dedicated bounded projector. Retain only fixed schema/state/reason values, booleans, bounded counts, public stage/gate slugs and their status enums, next/first-missing public identifiers, and coarse verifier failure-domain categories. Include an explicit unavailable state if the source is missing or malformed.

Acceptance evidence: synthetic full and incomplete H007 receipts project correct public stage/gate/task/verifier status; embedded prompt, note, transcript, tool, assertion, and raw verifier text are absent even when injected into unknown fields.

### Work package C — Offline verification and next unchanged live attempt

Intent: verify diagnostics and identity without spending provider requests, then test the projection on the next authorized attempt.

Required changes: run focused campaign/CLI/autonomy/verifier projection tests and privacy guards; verify at least 30 GiB free before the live start. Use the unchanged Muse Spark 1.3 Contributor route, fixture/verifier and recorded H007 caps. Preserve a safe attempt preflight and post-run diagnostic.

Acceptance evidence: offline tests pass; attempt 58 reconciles against its exact preflight identity; safe output identifies task completion versus verifier status and public stage/gate progress without raw content. H007 remains unqualified unless the unchanged independent verifier and all qualification gates pass.

## 8. Failure, cancellation, restart, and contention semantics

Projection failure does not undo or duplicate a provider-started campaign count. Missing or malformed diagnostic subobjects become `unavailable` and cannot satisfy any gate. Reconciliation remains idempotent by start number; an attempted replay with conflicting identity refuses. Existing external stop, inactivity cancellation, disk-floor, request-cap, and hard-stop behavior remains in force.

## 9. Compatibility and migration

No ledger schema migration is required. Existing attempt records and preflight files remain unchanged; the attempt 56 mismatch is recorded separately. New campaign safe evidence may add a versioned nested diagnostic object while retaining the current identity and counter fields.

## 10. Required tests

### Focused unit tests

- Canonical preflight identity wins over every conflicting per-case identity field.
- H007 task-completion and continuation projections retain safe state, reasons, verifier domain counts, public stage/gate progress, and first-missing identifiers.
- Missing, malformed, and oversized nested diagnostics produce an explicit unavailable projection.
- Unknown/private nested fields, assertion text, transcript, prompt, tool arguments/results, raw errors, and credentials are dropped.
- Projection does not mutate either input mapping or authorization counters.

### Integration tests

- The H007 CLI path supplies the selected case-level diagnostics from a production-shaped suite receipt and immutable preflight identity to campaign evidence.
- A provider-started unqualified result reconciles request count and status once without changing another attempt.

### Security and negative tests

- Hostile nested strings and unknown field names do not pass through the safe evidence projection.
- A mismatched launch/per-case manifest cannot be silently accepted as the same identity.

## 11. Required verification commands

```bash
python3 -m unittest -v dev.harness_lab.tests.test_campaign_authorization dev.harness_lab.tests.test_cli
python3 -m unittest -v dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_verifier_diagnostics dev.harness_lab.tests.test_h007
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 -m py_compile dev/harness_lab/campaign_authorization.py dev/harness_lab/__main__.py dev/harness_lab/tests/test_campaign_authorization.py
git diff --check
```

Do not start the next live attempt until these focused checks pass and the preflight disk reserve is verified. Do not claim a broader Rust or workspace verification that was not run.

## 12. Documentation updates

- Update the S019 roadmap and registry with attempts 55–58, current counters, the S045 preflight result, the attempt 56 identity collision, the attempt 57 projection gap, and S046 ownership.
- Preserve the attempt 56 identity correction note; do not alter either original preflight, safe diagnostic, or historical campaign row.

## 13. Acceptance criteria

- Campaign identity after provider admission is byte-for-byte consistent with the launch preflight.
- The safe campaign artifact identifies whether a run stopped in task completion, public continuation, or verifier evaluation and includes bounded public stage/gate progress when available.
- A negative privacy regression proves unknown and raw fields cannot escape.
- The next authorized run uses unchanged H007 content and controls and provides its safe outcome; no H007 pass is claimed without the unchanged independent verifier and every other gate.

## 14. Stop conditions

- Stop before provider admission if the preflight identity disagrees with the current suite/binary, authorization headroom is absent, safe evidence paths collide, or free disk is below 30 GiB.
- Stop if a diagnostic projection requires retaining raw prompt, transcript, tool payload, raw verifier output, or credential material.
- Stop if the proposed fix requires altering H007 content, verifier, provider route, or caps; create a separate scoped plan instead.
- Stop if a repeated campaign identity cannot be reconciled idempotently.

## 15. Closure evidence required

- Focused unit/integration test and privacy-guard results.
- A safe attempt-56 identity-correction note preserving both original artifacts and clearly identifying the field-source collision.
- Attempt 57's preflight and safe diagnostic confirm the exact identity match and expose the projection gap without private data. Attempt 58 shows populated public task/stage/gate and verifier summaries, 175 requests, normal terminal classification, and safe runtime telemetry.
- An explicit H007 qualification decision, keeping task correctness and long-horizon duration distinct.

## 16. Handoff notes

Attempt 58 demonstrates sustained harness operation for 34 minutes and confirms all seven public stages and twelve gates were recorded complete. It ended normally but failed independent verification in the continuation-boundary and transcript-job-lifecycle domains. S046 closes its identity and safe completion-projection scope; S047 owns the narrow missing per-check diagnostic needed to decide whether the next corrective belongs to task implementation or evidence reconciliation. Do not infer a daemon, scheduler, ACP, provider, or disk defect from the verifier failure alone.
