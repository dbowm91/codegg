# Post-Audit Correctness, Simplification, and Footprint C002 — Sandbox File Rights Correction

Status: superseded
Repository baseline: `8a556f05ab2f446ab8577f568bfb90912e49274e`

Superseded by: `plans/implementation/post-audit-correctness-simplification/011-sandbox-rights-correction-and-strict-closure.md`

This file is retained as the original C002 registration stub for planning history. Implementation agents must use the detailed successor plan above; no work should be executed from this stub independently.

Source roadmap/addendum: `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
Related corrective closure: `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`
Primary class: invariant

## 1. Original objective

Correct the concrete sandbox setup defect exposed by hosted workspace tests: `/dev/null` is
currently registered with directory-only `ReadDir` rights, causing Python-script executor
tests to fail before exercising their intended behavior.

## 2. Original scope

Inspect the sandbox path-rights model and make the smallest platform-safe correction for
special files such as `/dev/null`. Preserve the existing sandbox policy, filesystem
isolation, subprocess restrictions, and evidence reporting. Add or adjust focused tests so
regular files, directories, and special files cannot receive incompatible rights.

Out of scope: dependency upgrades, CI changes, broad Landlock redesign, new capabilities,
or changes to the merged post-audit workstream.

## 3. Original required verification

- Focused Python-script executor tests covering all seven failures from hosted CI.
- Relevant sandbox contract/static guards.
- `scripts/verify.sh quick`.
- Hosted `verify` on the resulting merge candidate.

## 4. Original stop conditions

Stop and report if the fix requires changing sandbox authority, supported platform policy,
or the independent runtime-safety/Landlock architecture. Do not hide a platform-specific
limitation by weakening tests or silently dropping filesystem restrictions.

## 5. Original closure evidence

Create a C002 closure record with the exact source/test changes, focused and hosted results,
security/invariant review, and a blocked-work audit before marking the corrective addendum
strictly closed.
