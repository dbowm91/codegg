# Workspace Migration Test-Contract CI Corrective Addendum

Status: closed

Repository baseline reviewed: working tree after `bba7b2a3`

Related closed work:

- `plans/subsystems/context-continuity-compaction-roadmap.md`
- `plans/subsystems/long-horizon-work-execution-roadmap.md`
- `plans/subsystems/project-work-orders-task-view-roadmap.md`

## 1. Corrective trigger

The canonical workspace suite exposed stale integration assertions that
expected storage layout version 62 even though the repository's authoritative
`codegg_core::storage::STORAGE_LAYOUT_VERSION` is 65. The affected tests are
the continuation-checkpoint and WorkPlan migration fixtures; the WorkOrder
fixture was corrected separately by C001 in the Project Work Orders CI
corrective.

These are test-contract failures, not production migration regressions. They
were missed because the original milestones closed against the then-current
layout before later accepted migrations advanced the canonical version.

## 2. Scope

One corrective milestone:

- replace historical terminal-version literals in affected migration tests
  with the canonical storage-layout constant;
- preserve additive-table, remigration, legacy-data, and empty-state checks;
- run the full workspace suite and hosted `CI / verify` to find any remaining
  stale terminal-version assertions; and
- record the registry unblock audit without reopening closed capability scope.

## 3. Non-goals

- No production schema or migration changes.
- No weakening of migration compatibility or remigration checks.
- No new migration framework or CI lane.
- No changes to Security Review, provider authentication, or managed-key
  implementation.

## 4. Milestone

### C001 — Canonical storage-layout assertions

Status: closed; closure record:
`plans/closure/workspace-migration-test-contract-ci-corrective/001-status.md`.

Plan: `plans/implementation/workspace-migration-test-contract-ci-corrective/001-canonical-storage-layout-assertions.md`.

## 5. Completion definition

The corrective is closed. The focused continuation/WorkPlan migration tests,
the canonical quick and Clippy gates, the full workspace suite, and hosted
`CI / verify` all pass on `c9c37cca` in run 35483642396. No future plan is
unblocked by this test-only correction.
