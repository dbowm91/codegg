# Harness Lab Self-Improvement S009 — Positive Generic Defect Intake

Status: **closed — positive synthetic intake path validated; campaign eligibility remains impossible**

Repository baseline: `e8dbb8af3793acf46741eae89b3c5509fc952e47` (PR #219)

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S009.
Long-term requirements: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/003-planning-process.md`.
Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md`.
Primary class: **infrastructure**.

## 1. Objective

Extend defect intake beyond the H007-only negative cohort and prove the positive schema path using a deterministic, isolated test fixture. A positive fixture validates intake mechanics only; it is not a real eligible campaign or evidence that H007's unfinished Rust work was a harness defect.

## 2. Why this corrective is ready

S004 truthfully emits an ineligible H007 dossier but its builder hard-codes that report, H007 identifiers and a null current red reproduction. It has no positive generic intake test. S001 found no eligible current agent-harness defect, so a generic fixture is needed to validate the positive contract without inventing a production defect.

## 3. Current implementation evidence

`dev/harness_lab/defect_intake.py::build_dossier` derives dispositions from the S001 H007 attribution report. Tests cover repeatability, stale/incomplete reports, symlink refusal and only negative current eligibility. S006 correctly excludes H007 and historical C003 from its new-live-campaign denominator.

## 4. Invariants that must not regress

- Eligibility requires a current CodeGG-owned failing-first reproduction, exact source/binary/case/verifier/holdout identities, repeat evidence and independent expected outcomes.
- A task failure, provider timeout, verifier setup failure, H007 Rust implementation miss, historical C003 record, or synthetic fixture cannot be promoted as a genuine current defect.
- Unknown evidence remains unknown; untrusted paths, symlinks, changed digests and incomplete red/green pairs are refused.
- Positive test fixtures are visibly marked synthetic and can never satisfy live-campaign admission.

## 5. Scope

### In scope

- Versioned generic reproduction intake separate from the immutable H007 dossier.
- One deterministic fixture that exercises the positive validation path and additional malformed/ineligible controls.
- Stable dossier identity derived from normalized evidence content and evidence digests.
- Explicit handoff requiring a new real independently reproduced defect before any S003/S006 attempt.

### Explicitly out of scope

- Claiming a current production defect from a test fixture or from the H007 four-crate task.
- Running a model/provider, opening an autonomous campaign, reusing consumed authorizations, or changing the hidden verifier.
- Rewriting the historical S001/S004 evidence.

## 6. Required production changes

Add a generic, bounded parser/builder alongside `build_dossier`; preserve its H007-only behavior. Require normalized repository-relative evidence paths and SHA-256 checks for source revision, red test, independent verifier, holdout and protected controls. Require repeated matching red observations plus an independently verified expected result; reject candidate-controlled evidence. Make `synthetic_fixture` an explicit non-campaign disposition. Do not modify S003/S006 live authorization state.

## 7. Ordered work packages

### A — Specify generic evidence schema

Define fields for defect ID, owner, symptom, reproduction command, source and binary identity, red-test repeats, verifier/holdout, expected result, protected controls, and synthetic/production provenance.

Acceptance evidence: schema version and invariants documented, with malformed-input behavior enumerated.

### B — Add positive and negative intake tests

Use a temporary repository fixture containing a deterministic modeled CodeGG harness regression and independent verifier. Prove that a correctly bound production-shaped dossier is parsed as positive *intake mechanics*, while its synthetic marker keeps `campaign_eligible` false. Refuse changed source, one-off failure, H007 task failure, provider timeout, verifier setup error, missing holdout, path escape and mismatched expected results.

Acceptance evidence: stable positive dossier ID across repeated input; no fixture can produce real campaign eligibility.

### C — Connect real defect handoff without inventing one

Keep the committed H007 dossier unchanged. Add operator guidance that a future real intake must originate from an independently reproduced current CodeGG harness failure and require fresh per-campaign authorization separately.

Acceptance evidence: no real defect is asserted by this plan; registry and S006 status keep current eligibility at zero.

## 8. Failure, cancellation, restart, and contention semantics

Generic intake is read-only and deterministic. Duplicate dossier creation from the same bytes yields the same ID. A partial evidence set, read error or mismatched hash returns a refusal with no candidate scope or side effect. Intake never mutates the source, verifier or receipt.

## 9. Compatibility and migration

Do not alter `defect_dossier_v1` or existing H007 dossier bytes. Introduce a separate schema version and parser. Preserve consumers that rely on H007's stable identifier and disposition.

## 10. Required tests

- Positive fixture mechanics with deterministic stable ID and synthetic/non-campaign outcome.
- Repeated red evidence, independent verifier pass/fail pairing, current source and binary digest binding.
- Negative H007-like task failure, route timeout, fixture setup error and historical record cannot become eligible.
- Symlink/path traversal, missing/changed digest, duplicate, malformed JSON and incomplete holdout refusal.

## 11. Required verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_defect_intake
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
```

## 12. Documentation updates

Document the difference between generic intake mechanics, synthetic eligibility tests, and a real production defect. Keep S006's current count at zero until new evidence exists.

## 13. Acceptance criteria

Generic positive intake behavior is tested repeatably; synthetic fixtures cannot authorize campaigns; all negative cases fail closed; H007 remains ineligible; no new provider call or campaign authorization is created.

## 14. Stop conditions

Stop if positive eligibility would require reclassifying H007's Rust workload failure or using historical C003 as a new defect. If no genuine defect exists, leave campaign eligibility at zero and report the missing evidence; do not seed a fake production finding.

## 15. Closure evidence required

Record schema, positive/negative test matrix, stable fixture dossier hash, explicit non-campaign marker, unchanged H007 dossier hash, test and guard results, and the current genuine-defect eligibility count.

## 16. Handoff notes

All fixtures and verifiers live in temporary test repositories. Never run model/provider requests; preserve any user-owned fixture edits in the main worktree.
