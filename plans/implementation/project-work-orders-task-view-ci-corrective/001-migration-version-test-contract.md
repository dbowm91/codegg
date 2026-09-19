# Project Work Orders and Task View CI Corrective C001 — Migration Version Test Contract

Status: implemented

Repository baseline: `4ec46a9b`

Source roadmap: `plans/subsystems/project-work-orders-task-view-ci-corrective-addendum.md`

Primary class: corrective / verification closure

## 1. Objective

Repair the stale WorkOrder remigration test assertion exposed by hosted CI so
it follows the canonical storage layout marker without changing production
storage behavior.

## 2. Scope

Update only the test assertion in
`tests/work_orders_m001_foundation.rs` from a historical literal to
`codegg_core::storage::STORAGE_LAYOUT_VERSION`. Preserve all existing
remigration, legacy-row, and constraint checks.

## 3. Acceptance criteria

- `daemon_remigration_is_additive_and_empty_by_default` passes with the
  current layout version 65.
- The WorkOrder foundation integration suite passes.
- No production files change.
- Hosted `CI / verify` passes.

## 4. Required verification

```bash
cargo test --test work_orders_m001_foundation -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

## 5. Closure evidence

Create `plans/closure/project-work-orders-task-view-ci-corrective/001-status.md`
with the hosted run URL/result, focused test result, explicit no-production-
delta statement, and registry unblock audit.
