# Harness Lab Self-Improvement Resumption S040 — Idempotent Live-Start Accounting

Status: **implemented; closed at ledger-accounting scope**

Repository baseline: `ac584c886f03847ff6b829f8509667637e42a052` (shared dirty worktree; preserve unrelated edits)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md#34-s040--idempotent-live-start-accounting`

Long-term requirements: `plans/000-long-term-specification.md` §§26–29; `plans/002-long-term-roadmap.md`

Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`

Primary class: infrastructure

## 1. Objective

Make each provider-started S019 run consume exactly one authorized start under its own attempt identity. Repeated progress reconciliation must never overwrite the prior attempt or lose the historical offset.

## 2. Why this is ready

Attempt 50 started under the approved route and used 183 provider requests. Its private wrapper selected the existing attempt-49 row, replacing its 286-request count and failing to append attempt 50. The safe receipts preserve both measurements, so the ledger can be repaired without examining transcripts, prompts, private command arguments, credentials, or hidden-verifier output.

## 3. Current implementation evidence

Live wrappers update `s019-muse-diagnostic-authorization-20261109.json` directly. Their row selection is local script logic and has no tested idempotency boundary. The new `dev/harness_lab/campaign_authorization.py` provides a pure, attempt-keyed reconciliation function; a focused test module covers retry behavior and refusal conditions.

## 4. Invariants

- The first observed provider request consumes one start, even if the run later fails.
- The immutable historical offset remains explicit and is added exactly once.
- A retry for the same attempt may advance request/status fields but cannot change its identity or decrease requests.
- Reconciliation for one attempt never modifies another attempt's row.
- Unknown or sensitive wrapper fields are not copied into the campaign ledger.
- The model route, request ceiling, and all other per-run caps remain unchanged.

## 5. Scope

### In scope

- A tested attempt-keyed, idempotent ledger reconciliation helper.
- Repair attempt 49 from its mode-0600 safe evidence and append attempt 50 from its own safe evidence.
- Update the live wrapper to call the helper before another provider start.
- Reconcile grant and combined start counters while preserving offset 1.

### Explicitly out of scope

- Changing authorization limits, providers, task fixtures, verifier behavior, runtime limits, or provider-key handling.
- Rewriting unrelated campaign history or changing historical failed-before-start rows.

## 6. Required production changes

Implement a pure function that validates attempt identity, expected start number, active grant/model/route, provider-request ceiling, combined cap, and grant cap before mutation. Copy only a documented allowlist of safe run fields. Reject duplicate attempt rows and conflicting replays. Use this helper in all subsequent S019 run wrappers.

## 7. Ordered work packages

### Work package A — Safe ledger reconciliation

Acceptance evidence: repeated updates preserve an earlier row, do not create duplicates, and keep request counts monotonic. Incorrect start numbers, over-cap requests, exhausted grants, conflicting identities, and duplicate rows refuse without mutation.

### Work package B — Historical repair

Restore attempt 49's 286 requests from its safe evidence; add attempt 50 with its own 183 requests and safe-evidence digest. The ledger must report 7/40 supplemental starts and 51/200 combined starts, preserving the one historical offset.

## 8. Failure and retry semantics

A run is counted when at least one provider request is observed. Repeating a reconciliation is idempotent. A counter decrease, identity drift, duplicate attempt, or cap mismatch is a hard refusal, not a best-effort write.

## 9. Compatibility

Keep the existing ledger fields and the one unrepresented historical start. Do not renumber any prior run.

## 10. Required tests

- Unit tests for idempotency, attempt isolation, preserved offset, invalid sequence, request overrun, duplicate identity, and safe-field allowlisting.
- Focused assertion-quality and duplication checks for new tests.

## 11. Required verification

```bash
rtk proxy python3 -m unittest dev.harness_lab.tests.test_campaign_authorization -v
rtk proxy python3 scripts/check_test_assertion_quality.py
rtk proxy python3 scripts/check_test_duplication.py
rtk proxy git diff --check
```

## 12. Documentation

Update the S019 status, authorization ledger, roadmap, and registry with reconciled attempt 49/50 counts and the safe evidence paths.

## 13. Acceptance criteria

The ledger contains separate, nonduplicated rows for attempts 49 and 50, with 286 and 183 requests respectively. It reports 7/40 grant starts and 51/200 combined starts. The helper's regression suite passes.

## 14. Stop conditions

Stop before another live start if the existing attempt identity, offset, grant ceiling, or recorded safe evidence cannot be reconciled without guessing.

## 15. Closure evidence

Record focused test results, the exact attempt/request/start counts, grant and combined remaining counts, corrected evidence hashes, and confirmation that the temporary credential value was not copied.

## 16. Handoff notes

Do not inspect private runner logs, transcripts, prompts, tool arguments, credential values, or hidden-verifier source/output. Keep the shared dirty worktree intact.
