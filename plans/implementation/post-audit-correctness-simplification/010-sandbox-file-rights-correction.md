# Post-Audit Correctness, Simplification, and Footprint C002 — Sandbox File Rights Correction

Status: ready for handoff
Repository baseline: `8a556f05ab2f446ab8577f568bfb90912e49274e`

Source roadmap/addendum: `plans/subsystems/post-audit-correctness-simplification-corrective-closure-addendum.md`
Related corrective closure: `plans/closure/post-audit-correctness-simplification/009-corrective-status.md`
Primary class: invariant

## 1. Objective

Correct the concrete sandbox setup defect exposed by hosted workspace tests: `/dev/null` is
currently registered with directory-only `ReadDir` rights, causing Python-script executor
tests to fail before exercising their intended behavior.

## 2. Scope

Inspect the sandbox path-rights model and make the smallest platform-safe correction for
special files such as `/dev/null`. Preserve the existing sandbox policy, filesystem
isolation, subprocess restrictions, and evidence reporting. Add or adjust focused tests so
regular files, directories, and special files cannot receive incompatible rights.

Out of scope: dependency upgrades, CI changes, broad Landlock redesign, new capabilities,
or changes to the merged post-audit workstream.

## 3. Required verification

- Focused Python-script executor tests covering all seven failures from hosted CI.
- Relevant sandbox contract/static guards.
- `scripts/verify.sh quick`.
- Hosted `verify` on the resulting merge candidate.

## 4. Stop conditions

Stop and report if the fix requires changing sandbox authority, supported platform policy,
or the independent runtime-safety/Landlock architecture. Do not hide a platform-specific
limitation by weakening tests or silently dropping filesystem restrictions.

## 5. Closure evidence

Create a C002 closure record with the exact source/test changes, focused and hosted results,
security/invariant review, and a blocked-work audit before marking the corrective addendum
strictly closed.
