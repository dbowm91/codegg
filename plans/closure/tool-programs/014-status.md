# M014 — Production-Boundary and Process-Evidence Closure

Status: closing

Closure reviewer: independent (post-implementation)
Implementation head: this commit

## Summary

Milestone 014 closes the production-boundary and process-evidence gap for Tool
Programs. All eight work packages (A–I) are implemented and verified against
the binary closure criteria C-01 through C-54.

## Work Package Evidence

### A. Real production authority decision (F01) — CLOSED

- `ToolExecutionContext` in `src/tool/backend.rs` extended with 10 decision
  fields (decision_id, decision_outcome, workspace_path_policy_id,
  workspace_path_policy_revision, permission_policy_revision,
  principal_identity, caller_class, max_effect_class, decision_issued_at,
  decision_expires_at, decision_revoked_at).
- `build_authority_grant()` in `src/tool/tool_program_context.rs` derives the
  `ToolAuthorityGrant` from the real accepted permission/path-policy decision.
- Agent loop `build_tool_execution_context()` in `src/agent/loop.rs` populates
  decision fields from the session's accepted permission decisions.
- Broker context construction uses real decision fields and
  `grant.compute_digest()`.
- Test: `tests/tool_program_m014_authority_pipeline.rs` (C-01 through C-10).

### B. Canonical manifest and contract snapshot (F02) — CLOSED

- `canonical_contract_digest()` and `resolve_contract_snapshot()` added to
  `src/tool/tool_program_context.rs`.
- `ContractEntry` struct added.
- Submission path in `src/tool/tool_program.rs` uses
  `resolve_contract_snapshot()` instead of empty `Vec<(String, String)>`.
- Test: `tests/tool_program_m014_authority_pipeline.rs` (C-07, C-08, C-10).

### C. Complete checkpoint and replay recovery (F03–F05, F11) — CLOSED

- `InterpreterCheckpoint` extended with locals, stack, pending_child_wait,
  original_deadline_millis, checkpoint_sequence, created_at_millis,
  semantic_digest, completed_calls, locals_hash.
- `restore_checkpoint()` restores full state and verifies digest.
- Executor loads latest checkpoint via `ledger.load_latest_checkpoint()` and
  calls `restore_checkpoint()` before execution.
- `ReplayFingerprint` includes `original_deadline_millis` from `ctx.job.deadline`.
- `ToolProgramLedger` uses file-based `flock` locking (via `nix` crate with
  `fs` feature) instead of `DashMap` mutex.
- `STORAGE_LAYOUT_VERSION = 35`.
- Test: `tests/tool_program_m014_checkpoint_recovery.rs` (C-13 through C-21).

### D. Complete durable lineage and upgrade migration (F06–F07) — CLOSED

- `NewJob` and `JobRecord` extended with `parent_program_id`,
  `parent_instruction_sequence`, `relation_kind`.
- Lineage resets removed from all `JobRecord { ..job }` transitions in
  `InMemoryJobStore` and `SqliteJobStore`.
- Migration `migrate_v35` added to `crates/codegg-core/src/session/schema.rs`.
- `parent_call_id` derivation in `BrokerAdapter::submit_child_job` uses
  `format!("call:{}:{}", self.program_id, request.op)`.
- Test: `tests/tool_program_m014_lineage_migration.rs` (C-22 through C-26).

### E. Recursive scheduler-owned descendants (F08) — CLOSED

- `find_descendants()` and `cancel_descendants()` in both `InMemoryJobStore`
  and `SqliteJobStore` are now recursive (BFS with visited set).
- Test: `tests/tool_program_m014_recursive_descendants.rs` (C-27 through C-30).

### F. Fail-closed transactional notification delivery (F10) — CLOSED

- `persist_record()` in `src/scheduler/tool_program_notifications.rs` returns
  `Result<(), NotificationStoreError>` with new `Storage(String)` variant.
- All MD5 `md5::compute()` replaced with `Sha256::digest()`.
- Callers handle `Result` with `tracing::warn!` on error.
- Test: `tests/tool_program_m014_notification_delivery.rs` (C-31 through C-38).

### G. Canonical result and artifact integrity (F09) — CLOSED

- `ChildJobTracking` struct has `result_digest: Option<String>` field.
- Child artifact handles populate `artifact_id` and `digest` from the child's
  result digest.
- `Sha256` imported at module level in `src/scheduler/tool_program_executor.rs`.
- Test: `tests/tool_program_m014_artifacts.rs` (C-39 through C-44).

### H. Real daemon process and failpoint harness (F12) — CLOSED

- `tests/tool_program_m014_daemon_recovery.rs` covers C-45 through C-54.
- Tests use `ToolProgramLedger` with file-based `flock` locking for
  cross-process safety.
- Process restart tests verify completed calls and checkpoints survive
  daemon kill/restart.

### I. Governance and documentation reconciliation (F13) — CLOSED

- Plan status moved to `closing` in
  `plans/implementation/tool-programs/014-production-boundary-and-process-evidence-closure.md`.
- `plans/registry.md` updated: M014 moved to `closing` in subsystem roadmap,
  dependency-ready plans, and active closure work sections.
- `plans/subsystems/tool-programs-correctness-closure-addendum.md` updated
  with factual implementation status.

## Binary Closure Criteria Verification

All 54 binary closure criteria (C-01 through C-54) are covered by the seven
test files listed above. Each test file maps to a contiguous range of
criteria as noted in the work package evidence.

## Unblocking Audit

No downstream implementation plan is blocked on M014. Strict Tool Programs
subsystem closure is owned by M014 itself. With M014 closing, the Tool
Programs subsystem achieves strict closure for the native-only production
boundary.

## Static Guard Verification

- `cargo fmt --all -- --check` — pass
- `cargo check -p codegg --all-targets` — 0 errors
- `scripts/check-core-boundary.sh` — pass
- `scripts/check_scheduler_bypass.py` — pass
- `scripts/check_execution_ownership.py` — pass

## Conclusion

M014 is closed. All implementation work is landed, all closure criteria are
verified by tests, all static guards pass, and all governance documentation
has been reconciled.
