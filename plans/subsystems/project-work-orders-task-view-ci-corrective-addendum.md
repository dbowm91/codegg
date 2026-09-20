# Project Work Orders and Task View — CI Migration-Test Corrective Addendum

Status: closed

Repository baseline reviewed: `4ec46a9b`

Predecessor work:

- `plans/subsystems/project-work-orders-task-view-roadmap.md`
- `plans/closure/project-work-orders-task-view/001-status.md`
- `plans/closure/team-collaboration-corrective/004-status.md`

## 1. Corrective trigger

Hosted `CI / verify` run 35467200089 reached the workspace tests and failed
only in `tests/work_orders_m001_foundation.rs`:
`daemon_remigration_is_additive_and_empty_by_default` asserted migration
version 63, while the repository's current canonical storage layout is 65
after the accepted team-collaboration v64/v65 migrations.

This is stale test metadata, not a WorkOrder production regression.

## 2. Scope

One test-contract correction: compare the remigration result with
`codegg_core::storage::STORAGE_LAYOUT_VERSION` so the additive migration test
tracks the canonical terminal layout while continuing to verify legacy-row
preservation and rebuilt constraints.

## 3. Non-goals

- No production schema or migration change.
- No rewrite of historical WorkOrder or team-collaboration closure records.
- No new CI lane, migration framework, or version policy.

## 4. Milestone

### C001 — WorkOrder remigration assertion convergence

Status: closed; closure record:
`plans/closure/project-work-orders-task-view-ci-corrective/001-status.md`.

Plan: `plans/implementation/project-work-orders-task-view-ci-corrective/001-migration-version-test-contract.md`.

## 5. Completion definition

The corrective is closed. The focused WorkOrder foundation suite, canonical
quick/Clippy gates, full workspace suite, and hosted `CI / verify` all pass on
the closure revision. No future plan is unblocked by this stale assertion
correction.
