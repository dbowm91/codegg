# Project Work Orders and Task View CI Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view-ci-corrective/001-migration-version-test-contract.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-ci-corrective-addendum.md`

Repository baseline and closure revision: `c9c37cca`

Implementation commits:

- `5c4e2966` — test: reconcile migration version closure
- `c9c37cca` — hosted CI closure revision; also closes the separately owned managed-key and migration-fixture correctives

Hosted evidence:

- [CI / verify run 35483642396](https://github.com/dbowm91/codegg/actions/runs/35483642396)

## 1. Executive finding

The hosted CI failure was a stale test expectation: the repository's canonical
storage layout is version 65, while the WorkOrder remigration test still
expected the predecessor version 63. The assertion now uses
`codegg_core::storage::STORAGE_LAYOUT_VERSION`; no production schema or
migration behavior changed.

## 2. Evidence

| Requirement | Evidence | Result |
|---|---|---|
| Canonical version assertion | `tests/work_orders_m001_foundation.rs` compares with `STORAGE_LAYOUT_VERSION` | pass |
| Focused WorkOrder suite | `cargo test --test work_orders_m001_foundation -- --test-threads=1` | pass — 12 tests |
| Full local workspace suite | `cargo test --workspace --locked -- --test-threads=1` | pass — 11,540 passed, 3 ignored |
| Hosted canonical CI | [run 35483642396](https://github.com/dbowm91/codegg/actions/runs/35483642396) | pass — guards, formatting, Clippy, workspace tests, cleanup, and completion all passed |

## 3. Unresolved findings

None. The original stale assertion is corrected and the managed-key failure
that initially prevented strict hosted closure was resolved by its separately
owned corrective.

## 4. Roadmap and registry disposition

This follow-up does not reopen the closed WorkOrder capability roadmap or
change any migration. It is formally closed on the green hosted run; no future
plan is blocked on this correction. The historical WorkOrder capability
closure records remain unchanged.
