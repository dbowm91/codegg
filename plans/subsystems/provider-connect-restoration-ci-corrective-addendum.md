# Provider /connect Restoration — Managed-Key CI Corrective Addendum

Status: closed

Repository baseline reviewed: `5c4e2966`

Related closed work:

- `plans/subsystems/provider-connect-restoration-corrective-addendum.md`
- `plans/closure/provider-connect-restoration-corrective/001-status.md`

## 1. Corrective trigger

Hosted canonical CI run [35468898714](https://github.com/dbowm91/codegg/actions/runs/35468898714)
passed all repository guards, formatting, and Clippy, then failed in
`codegg-config::encryption::tests::concurrent_first_writes_converge_on_one_key`.
Concurrent first-write initialization observed `CorruptKeyFile` instead of
converging on the winner's key. A bounded retry remained in workspace tests for
approximately 25 minutes and was cancelled, so the behavior is not yet safely
classifiable as only a hosted flake.

This is outside the Security Review command-surface corrective. It touches the
managed-key bootstrap contract previously closed by provider-connect M001 and
must be investigated under its own plan.

## 2. Scope

One corrective milestone:

- reproduce the hosted concurrent first-write failure under the repository's
  canonical test command;
- inspect the atomic create/read-back and file-validation ordering;
- fix the narrowest production or test contract defect that is proven;
- preserve fail-closed behavior for corrupt, partial, unsafe, or replaced key
  files; and
- restore bounded, deterministic workspace-test completion before strict CI
  closure is claimed by any dependent plan.

## 3. Non-goals

- No Security Review command or dialog changes.
- No weakening of key-file validation or permission checks.
- No broad provider-auth redesign, key rotation, or migration.
- No CI exclusion or test retry masking the race.

## 4. Milestone

### C001 — Managed-key concurrent first-write CI corrective

Status: closed; closure record:
`plans/closure/provider-connect-restoration-ci-corrective/001-status.md`.

Plan: `plans/implementation/provider-connect-restoration-ci-corrective/001-managed-key-concurrency-ci-corrective.md`.

## 5. Completion definition

The corrective is closed. The focused encryption and relevant credential-store
concurrency tests pass repeatedly, `scripts/verify.sh quick` and the
repository-standard Clippy command are green, the workspace suite completes
without the managed-key race or hang, and hosted `CI / verify` passes on
`c9c37cca` in run 35483642396.

No current downstream plan is unblocked by this corrective registration.
