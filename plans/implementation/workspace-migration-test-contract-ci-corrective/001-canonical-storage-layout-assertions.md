# Workspace Migration Test-Contract C001 — Canonical Storage-Layout Assertions

Status: implemented

Repository baseline: working tree after `bba7b2a3`

Source roadmap: `plans/subsystems/workspace-migration-test-contract-ci-corrective-addendum.md`

Primary class: corrective / verification closure

## 1. Objective

Make migration integration tests follow the authoritative storage layout
version instead of stale historical literals, preserving all behavioral
assertions around additive schema changes and remigration.

## 2. Scope

- `crates/codegg-core/tests/continuation_checkpoint.rs`
- `crates/codegg-core/tests/work_plan_foundation.rs`
- Any additional stale terminal-version assertions found by the canonical
  workspace suite, provided they are test-contract corrections only.

## 3. Invariants

- `codegg_core::storage::STORAGE_LAYOUT_VERSION` remains the sole terminal
  layout authority.
- Tests continue to verify that required tables/indexes are present, data is
  preserved, and remigration is a no-op.
- No migration is added, removed, reordered, or weakened.

## 4. Acceptance criteria

- Continuation-checkpoint and WorkPlan integration suites pass.
- `scripts/verify.sh quick` passes.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passes.
- `cargo test --workspace --locked -- --test-threads=1` completes cleanly.
- Hosted `CI / verify` passes on the closure revision.

## 5. Closure requirement

Create `plans/closure/workspace-migration-test-contract-ci-corrective/001-status.md`
with exact focused/full/hosted evidence and a registry unblock audit. Do not
rewrite historical capability closure records.
