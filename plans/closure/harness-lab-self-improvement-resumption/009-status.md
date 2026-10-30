# Harness Lab Self-Improvement S009 — Closure

Status: **closed — positive generic synthetic intake validated; no campaign eligibility granted**.

## Implementation

`build_generic_dossier` and `--generic-manifest` add a generic path alongside the frozen H007 builder. The accepted schema requires distinct source, test, test-result, independent-verifier and protected-holdout artifacts, verifies their content digests and their cross-bindings, requires two distinct private-lane IDs with the same red signature, and validates normalized candidate scope. Repository path resolution rejects traversal and symlink escapes.

The initial generic path accepts only `evidence_kind: synthetic_fixture`. Its positive disposition is `synthetic_reproduction_validated`; `campaign_eligible` is always false and no candidate allowlist is emitted. Any claim of a current CodeGG defect needs a separately reviewed authority path. The immutable H007 builder, original receipts and S006 live-campaign denominator are unchanged.

## Evidence

- `python3 -m unittest dev.harness_lab.tests.test_defect_intake -v`: **10 passed**.
- Positive synthetic result ID: `synthetic-intake-828b1eca4a144893`.
- Stable identity SHA-256: `828b1eca4a144893b345668db2f0005d1076b8bded63f9c4bf674450b8971f54`.
- Negative cases reject duplicate lane IDs, an unbound verifier, a non-synthetic evidence label, protected-path scope and a symlink escape.
- The existing seven H007 records remain typed and ineligible; repeated S001 input still produces the same H007 dossier.

The temporary synthetic source/test/verifier/holdout files were removed after qualification. The receipt above retains their content-addressed identity only; it does not create a current defect, authorize a proposal or change any campaign count.
