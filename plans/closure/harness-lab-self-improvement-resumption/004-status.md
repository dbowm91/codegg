# Harness Lab Self-Improvement S004 — Closure

Status: **closed — deterministic offline intake capability implemented; no current campaign is eligible**.

## Requirement results

- Added `dev/harness_lab/defect_intake.py`, a bounded reader for the S001 frozen input manifest and attribution report. It binds the report to its input digest, validates the seven unique evidence IDs and known dispositions, refuses stale/incomplete evidence and emits a stable `defect_dossier_v1` identity.
- Added six focused tests in `dev/harness_lab/tests/test_defect_intake.py` covering the clean H007 disposition plus stale manifest, digest mismatch, incomplete report, unsupported status and duplicate identity refusal.
- Dossier ID: `h007-intake-9b3ddb668677c723`; disposition: `no_reproducible_defect`; candidate scope: empty; live eligibility: false. Machine-readable result: [`evidence/s004-h007-intake.json`](evidence/s004-h007-intake.json).
- Dossier evidence preserves `unknown` for unavailable trajectory fields, separates route and fixture failures from observed task failures, identifies historical C003 as historical only, and includes falsifiers for future attribution.
- The H007 receipt source, hidden verifier and consumed live approvals were not changed. No provider calls occurred.
- Current verification: S002's isolated `scripts/verify.sh quick` passed, including the full Harness Lab suite and static guard set; see [`002-status.md`](002-status.md).

## Limits and handoff

This closes the intake mechanism, not defect discovery. The S001 evidence has no complete event/turn trajectory, and no current failing-first CodeGG reproduction exists. Thus the dossier is correctly non-eligible; it does not establish a cause or justify a patch. A later eligible dossier must bind a fresh exact-main red reproduction, independent verifier/holdout, source and binary identities, and protected controls before any campaign.
