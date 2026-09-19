# Project Work Orders and Task View CI Corrective C001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/project-work-orders-task-view-ci-corrective/001-migration-version-test-contract.md`

Source subsystem roadmap:

- `plans/subsystems/project-work-orders-task-view-ci-corrective-addendum.md`

Repository baseline reviewed: `4ec46a9b`

Implementation commit:

- `5c4e2966` — test: reconcile migration version closure

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
| Hosted canonical CI | Runs [35467200089](https://github.com/dbowm91/codegg/actions/runs/35467200089) and [35468898714](https://github.com/dbowm91/codegg/actions/runs/35468898714) | conditional | The corrected candidate passed all guards, formatting, and Clippy; workspace tests then exposed an unrelated encryption race, and the bounded retry hung in workspace tests until cancellation. |

## 3. Unresolved findings

None in this corrective's scope. The original stale assertion is corrected;
the later managed-key encryption failure is unrelated and is registered under
`plans/implementation/provider-connect-restoration-ci-corrective/001-managed-key-concurrency-ci-corrective.md`.

## 4. Roadmap and registry disposition

This follow-up does not reopen the closed WorkOrder capability roadmap or
change any migration. It is conditionally closed pending the separately owned
managed-key CI corrective; no future plan is blocked on this correction.
