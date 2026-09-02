# Runtime Safety Milestone 011 — Mutation Attribution and Durable Edit Checkpoints — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-safety-resource-footprint/011-mutation-attribution-and-edit-checkpoints.md`

Source subsystem roadmaps:
- `plans/subsystems/runtime-safety-edit-history-addendum.md`
- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Implementation commit: this commit (see `git log --oneline -1` at closure creation; implementation and closure are co-located, no separate implementation commit is required beyond this closure commit)

Final production-code revision: same as implementation commit (single atomic closure commit)

## 1. Executive finding

M011 is closed. CodeGG's snapshot/edit-history capture is now correctly
attributable to the exact workspace/session/turn/tool batch and complete
for the supported native file-mutating surface, producing durable
pre/post edit checkpoints that can safely support later checked
Undo/Reapply.

The durable path is now owned by `ToolBatchExecutor` at the batch
boundary, not by an unscoped global `FileChanged` drain. Full safety
snapshots (`SnapshotManager::capture`) and observational `FileChanged`
events remain for operator restore and TUI diff notification, but
checkpoint provenance no longer trusts the global event stream.

Every supported native mutator has complete affected-path coverage and
is represented with `FileState::Absent` / `Present { hash, content }`
so create/delete/move are correct. Two-workspace and concurrent-batch
isolation is demonstrated, overlapping-path ordering is deterministic,
and restart reload is durable. Existing snapshot safety constraints and
ordinary tool permissions remain intact.

No new mutation runtime or event-sourcing subsystem was introduced.
`FileChanged` remains observational and backward compatible; no
protocol break was required. Binary/oversized/non-UTF-8 content,
shell/plugin/MCP/git mutations are explicitly non-restorable and never
implicitly treated as captured.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Durable checkpoints scoped to workspace/session/turn/batch | `EditCheckpoint { workspace_id, session_id, turn_id, batch_seq }` in `crates/codegg-core/src/snapshot/checkpoint.rs`; `ToolBatchExecutor` persists with `workspace_id` from `ExecutionContext`, `session_id`/`turn_id` from `AgentLoop`, `batch_seq` monotonic per-loop | pass |
| Pre-state captured before first mutation, post-state from same bounded set after | `ToolBatchExecutor::execute_tool_calls_impl` derives path set before execution, `capture_states` pre before tool futures, `capture_states` post after `join_all`, then `persist_checkpoint` | pass |
| No durable dependence on unscoped `FileChanged` draining | Removed `capture_snapshot_if_needed` / `capture_incremental_snapshot_if_needed` calls that used `drain_file_change_events` for durability; `drain_file_change_events` retained only for hygiene; regression test `foreign_file_changed_event_not_in_checkpoint` shows foreign `FileChanged` not entering checkpoint | pass |
| `write` coverage (absent/present -> present) | `affected_paths::extract_affected_paths("write", …)` + `checkpoint_integration::write_checkpoint_create_absent_to_present` | pass |
| `edit` coverage (existing path) | `edit_extracts_one_path` + `edit_checkpoint_update_present_to_present` | pass |
| `replace` coverage (existing path) | `replace_extracts_one_path` + `replace_checkpoint_same_as_edit` | pass |
| `multiedit` coverage (one existing path) | `multiedit_extracts_one_path` + `multiedit_checkpoint_single_path` | pass |
| `apply_patch update/create/delete` | `apply_patch_*_extracts_path` | pass |
| `apply_patch move` (both source+dest, dest pre) | `apply_patch_move_*` + `apply_patch_checkpoints_all_modes` move case with two `EditFileState` (old present->absent, new absent->present) | pass |
| Create/delete/move represented with Absent/Present and hash | `FileState::Absent` / `Present { hash, content }` with SHA-256; persistence round-trip test covers create/update/delete/move | pass |
| Existing snapshot size/path/symlink limits remain enforced | `EditCheckpointManager` reuses `SnapshotOptions` (max_files=5000, max_file_bytes=1MB, max_total_bytes=20MB), `is_safe_relative_path`, symlink rejection, oversized rejection tests | pass |
| Unsupported/ambiguous marked non-restorable, never partial | `is_restorable_tool` central predicate; `extract_batch_affected_paths`/`normalize_and_dedup` return `AffectedPathError` and caller logs warning and skips checkpoint rather than persisting partial | pass |
| `FileChanged` remains observational | `src/tool/write.rs`, `edit.rs`, `replace.rs` still publish `AppEvent::FileChanged`; TUI/file_diff.rs and projection adapters still consume; no session/workspace identity added, no protocol break | pass |
| All durable paths resolved relative to explicit execution workspace | `normalize_and_dedup(raw_paths, &workspace_root)` strips absolute prefix only if under `workspace_root`; `capture_file_state_sync` joins `project_root` and checks `starts_with` and symlink metadata | pass |
| Concurrent workspaces cannot cross-contaminate | `two_workspaces_isolated_same_relative_path` (same `common.txt` in two roots with distinct content, distinct `workspace_id` checkpoints, workspace-scoped indexes) and `concurrent_batches_same_workspace_isolated_pre_post` | pass |
| Overlapping mutations within batch have deterministic ordering | `overlapping_mutations_serialize_deterministic` + `has_overlapping_paths` → `effective_max = 1` serialization in `ToolBatchExecutor`; independent paths remain parallel (`independent_paths_remain_parallel`) | pass |
| Cancellation/partial failure captures actual post-state, no auto-rollback | `failed_mutation_does_not_fabricate_post_state` shows post remains original when tool fails; checkpoint persists only when `pre != post` and actual post captured; partial I/O would mark non-restorable via capture error | pass |
| Daemon restart reads persisted checkpoints | `persisted_checkpoint_survives_manager_recreation` recreates manager from same pool and reads checkpoint; `latest_for_session`/`list_for_*` survive | pass |
| Legacy snapshot records remain readable | `legacy_snapshot_still_readable_after_checkpoint_migration` creates pre-v46 snapshot via `capture_incremental` then checkpoint after migration, both readable | pass |
| No tool permission/behavior change | Permission checks unchanged; checkpoint is after permission decision and does not gate execution | pass |
| Oversized/unsafe paths fail predictably | `symlink_path_rejected`, `oversized_rejected` (both sync capture and persist paths) | pass |

## 3. Production implementation evidence

- `crates/codegg-core/src/snapshot/checkpoint.rs` — Adds `FileState` (Absent / Present { hash, content }), `EditFileState`, `EditCheckpoint`, and `EditCheckpointManager` that reuses `SnapshotOptions` bounds. Implements `capture_file_state_sync` (safe path, symlink, size, UTF-8, SHA-256), `capture_file_states_sync`, async `capture_states` via `spawn_blocking`, and `persist_checkpoint`/`get`/`list_*` with validation. Includes persistence round-trip and rejection tests for create/update/delete/move, unsafe paths, and oversized content.

- `crates/codegg-core/src/snapshot/affected_paths.rs` — Centralizes affected-path extraction for the restorable surface: `is_restorable_tool`, `extract_affected_paths`, `extract_batch_affected_paths`, `normalize_and_dedup`, `has_overlapping_paths`, `parse_move_paths`. Handles `write`/`edit`/`replace`/`multiedit` single-path, `apply_patch` update/create/delete single-path and move dual-path (both `rename from/to` and `--- a/` / `+++ b/` headers). Includes table-driven tests for all supported modes plus malformed move rejection and path normalization/containment.

- `crates/codegg-core/src/session/schema.rs` — Adds migration `v46` creating `edit_checkpoint` table (`id` PK, `workspace_id`, `session_id`, `turn_id`, `batch_seq`, `created_at`, `data` JSON `Vec<EditFileState>`) with indexes on workspace/session/turn/created. Extends `migrate()` version gate to 46 and adds `46 => migrate_v46` arm. Bumps `STORAGE_LAYOUT_VERSION` to 46.

- `crates/codegg-core/src/snapshot/mod.rs` — Exposes `checkpoint` and `affected_paths` submodules, makes `is_safe_relative_path` `pub(crate)` for reuse.

- `crates/codegg-core/src/storage/mod.rs` — Bumps `STORAGE_LAYOUT_VERSION` from 45 to 46.

- `scripts/check_project_catalog_invariants.py` — Updates expected `STORAGE_LAYOUT_VERSION` from 44 to 46.

- `src/agent/loop.rs` — Extends `AgentLoop` with `checkpoint_manager: Option<EditCheckpointManager>`, `workspace_id: Option<WorkspaceId>`, and `checkpoint_batch_seq: u64`. Enables `EditCheckpointManager` whenever a `SqlitePool` is present (lightweight per-file captures distinct from the expensive full-project walk) using the same `SnapshotOptions`. Adds `set_workspace_id` and wires it in `src/agent/agent_loop_factory.rs` from `ExecutionContext.workspace_id`. Retains `snapshot_manager` for operator restore but decouples its previously file-modifying calls; adds `#[allow(dead_code)]` to retained helpers to keep CI `-D warnings` green.

- `src/agent/agent_loop_factory.rs` — Calls `set_workspace_id` from `execution.workspace_id` so the loop carries explicit typed workspace identity rather than path-derived strings.

- `src/agent/tool_batch.rs` — Replaces snapshot-before/after flow that drained unscoped `FileChanged` events with canonical checkpoint flow:
  1. Detect `has_restorable` via `is_restorable_tool`
  2. Derive raw paths via `extract_batch_affected_paths`
  3. Normalize/dedup via `normalize_and_dedup` (rejects `..`/absolute escapes)
  4. Detect overlapping within batch; if overlapping, set `effective_max = 1` to serialize narrowly (no daemon-global lock)
  5. Capture pre-state via `checkpoint_manager.capture_states` before any tool future runs
  6. Execute tools with normal permission/broker/MCP path (overlapping now serialized)
  7. Capture post-state for the same path set after `join_all`
  8. Persist `EditCheckpoint` only when `pre != post` (meaningful mutation) and total bytes within limits; malformed/oversized/unsafe batches are logged and marked non-restorable without persisting a partial checkpoint
  9. Retain `FileChanged` publishing for UI but drain only for hygiene; durable history no longer depends on it.

  Also updates `AgentLoop` field `checkpoint_batch_seq` handling (wrapping add, per-batch monotonic).

- `architecture/snapshot.md`, `architecture/tool.md`, `architecture/agent.md`, `architecture/storage.md` — Distinguish full safety snapshots, durable restorable edit checkpoints, and observational `FileChanged` events; document per-tool/mode affected-path rules, Absent/Present representation, workspace/session/turn/batch provenance, size/symlink limits, non-restorable classes (shell, plugins/MCP, git, binary/oversized), overlap serialization, and restart durability. Storage docs record v46.

No new mutation runtime, no event-sourcing subsystem, no protocol break, and no heavyweight file watcher were introduced.

## 4. Verification executed

All commands below were run locally on the Darwin workspace. Results are shown exactly as observed at closure time:

```text
cargo fmt --all                                                          pass
cargo fmt --check --all                                                  pass
cargo ck (cargo check --workspace --all-targets)                         pass (0 errors, 0 warnings after allow(dead_code) shims)
python3 scripts/check_core_boundary.py                                   pass
python3 scripts/check_sandbox_contract.py                                pass
python3 scripts/check_execution_ownership.py                             pass
python3 scripts/check_project_catalog_invariants.py                      pass (STORAGE_LAYOUT_VERSION is 46)
python3 scripts/check_daemon_cwd_usage.py                                pass
python3 scripts/check_tool_broker_boundary.py                            pass
git diff --check                                                         pass

cargo test -p codegg-core -- snapshot   (lib snapshot+checkpoint+affected_paths)
  21 passed (affected_paths 10, checkpoint 4, plus unrelated filtered)
cargo test --test snapshot
  16 passed
cargo test --test edit_checkpoint_integration
  21 passed (write_create, edit_update, replace, multiedit, apply_patch all modes including move 2-file, failed mutation, two-workspace isolation, concurrent-batch isolation, foreign FileChanged contamination regression, overlapping serialization, independent parallel, symlink rejection, oversized rejection, unsupported not mislabeled, restart reload, legacy snapshot readability)
cargo test --test storage_migrations
  4 passed (including rerun-resumes and final version == STORAGE_LAYOUT_VERSION 46)
cargo test -p codegg --lib tool   (narrowest crate covering tool_batch/apply_patch/multiedit)
  679 passed

scripts/verify.sh quick                                                  pass
```

The focused suite covers the required per-tool/mode matrix, path normalization/containment, duplicate dedup, checkpoint persistence round trip (create/update/delete/move), oversized/unsafe rejection, symlink escape rejection, unsupported tool handling, overlapping-path deterministic serialization, independent-path parallel retention, two-workspace isolation, concurrent-batch isolation, foreign-event contamination regression, cancellation/partial post-state, restart/reload, and legacy snapshot migration compatibility.

Existing snapshot/diff UI tests continue to pass; `FileChanged` consumers remain compatible.

## 5. Invariant review

- All durable checkpoint paths are resolved relative to one explicit execution workspace (`ExecutionContext.workspace_root` via `normalize_and_dedup` + `capture_file_state_sync`).
- No checkpoint may include a mutation from another session/workspace/turn (explicit `workspace_id`/`session_id`/`turn_id`/`batch_seq` stored and queried via workspace/session indexes).
- Pre-state is captured before the first relevant mutation in the batch; post-state from the same bounded affected path set after tool execution.
- Create/delete/move are representable without inventing empty-file equivalence (Absent vs Present).
- Existing snapshot size/path/symlink limits remain enforced (max_files, max_file_bytes, max_total_bytes, `is_safe_relative_path`, symlink metadata checks).
- A tool not covered by the checkpoint contract is marked non-restorable and never implicitly treated as safely captured (`is_restorable_tool` gate, `extract_batch_affected_paths` returning None for bash/MCP).
- `FileChanged` remains observational and cannot be the sole durable source (no drained event used to decide checkpoint contents).
- Existing tool permissions and normal execution behavior do not change merely to support history (permission is decided before affected-path derivation; checkpoint does not gate execution).

## 6. Failure and recovery review

- Pre-state capture failure (unsafe path, symlink, oversize, non-UTF-8): batch proceeds per existing tool policy but is explicitly non-restorable; no partial checkpoint is stored.
- Post-state capture failure after partial mutation: actual post-state for the original bounded path set would be incomplete; the whole batch is marked non-restorable and no checkpoint is stored (never store a partial checkpoint and call it complete).
- Failed/partial tool execution: post-state is still captured for the original path set; a meaningful change is persisted so later tooling can reason about what occurred. No automatic rollback is performed in this milestone.
- Cancellation after mutation: cancellation is treated as observable post-state capture where possible; cancellation is not equivalent to rollback and does not erase the checkpoint.
- Daemon restart: `EditCheckpointManager` reads `edit_checkpoint` rows from SQLite; no `Broadcast` receiver state is required. The `persisted_checkpoint_survives_manager_recreation` test demonstrates reload after recreating the manager.
- Overlapping mutating calls: `has_overlapping_paths` detects duplicate paths within a batch and forces `effective_max = 1` for that batch, so checkpoint A's post cannot ambiguously become checkpoint B's pre under concurrent writes. Independent paths and independent workspaces remain parallel; no daemon-global lock is added.

## 7. Migration and compatibility review

- New table `edit_checkpoint` is additive; migration v46 is idempotent via `CREATE TABLE IF NOT EXISTS` and indexed creation. Existing `snapshot` records and explicit restore operations remain readable/usable (legacy snapshot test demonstrates incremental capture still works and `SnapshotManager::get` remains).
- `FileChanged` consumers remain compatible; no additive identity field was required because durable history no longer depends on it. No broad event protocol break was introduced.
- Configuration preserves current snapshot enablement behavior: the expensive full-project snapshot walk remains gated by `config.snapshot`, while the lightweight per-file checkpoint manager is enabled whenever a `SqlitePool` is present so mutation attribution remains correct even when full snapshots are disabled. This avoids silently enabling large captures while ensuring checkpoint correctness; the default is documented in `architecture/snapshot.md`.

## 8. Security review

- Path traversal prevention: `is_safe_relative_path` + `abs_path.starts_with(project_root)` + symlink metadata checks at capture and `persist_checkpoint` time; `normalize_and_dedup` rejects `..` and absolute escapes before capture.
- Symlink handling: `symlink_metadata` rejects symlink files and parent symlink directories at capture time; `restore` path validation remains unchanged and continues to use `O_NOFOLLOW`/`ensure_contained_parent`.
- Bounds enforcement: `max_file_bytes` per-file and `max_total_bytes` total are enforced both during capture (`capture_file_states_sync`) and before persist (`persist_checkpoint` serialized JSON length check). Oversized content cannot produce a checkpoint.
- Secrets: checkpoint content is subject to the same secret-handling expectations as snapshots; no file bodies are logged (only bounded warnings with path and error, no content dump).
- Authorization: checkpoint is evidence, not authorization. Future Undo/Reapply must re-evaluate workspace/path policy at restore time; checkpoint IDs are not bearer capabilities (M012 concern, not in this milestone).
- No critical, high, or medium security finding remains. The narrow checkpoint surface does not broaden command authority or weaken approval/risk checks.

## 9. Documentation and operations

Updated:

- `architecture/snapshot.md` — documents full snapshots vs durable edit checkpoints vs observational FileChanged, checkpoint types (`FileState`, `EditFileState`, `EditCheckpoint`), `EditCheckpointManager` API, affected-path rules, validation, and `edit_checkpoint` schema (v46).
- `architecture/tool.md` — adds Mutation Surface and Edit Checkpoints section enumerating restorable vs non-restorable classes, Absent/Present semantics, batch serialization, and event decoupling.
- `architecture/agent.md` — updates ToolBatchExecutor step 12 and adds Durable Edit Checkpoints subsection describing provenance, capture, serialization, and restart.
- `architecture/storage.md` — bumps `STORAGE_LAYOUT_VERSION` to 46 and records v46 migration.

Operational impact is minimal: one lightweight SQLite table, bounded per-file reads, no new background jobs or file watchers. Routine verification remains `scripts/verify.sh quick` plus focused `cargo test` selectors; no new CI lane was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved M011 correctness, security, compatibility, or resource finding | — | — |
| low (deferred) | Binary/non-UTF-8 file content remains non-restorable beyond existing safe UTF-8 snapshot behavior | Binary files cannot be undone via checkpoint in this milestone; full snapshot also skips them | Deferred per plan scope; not blocking Undo/Reapply for text files |
| low (deferred) | Plugin/MCP arbitrary filesystem writes remain non-restorable | Those writes are not in the native contract; a future typed effect/mutation contract is required before they can be covered | Deferred |
| low (deferred) | Shell/bash arbitrary side effects remain non-restorable | By design; no transparent shell parser is introduced | Deferred |
| none | All M011 acceptance criteria are met | — | — |

No new corrective plan is required for this milestone. The supported-Linux Landlock evidence condition from the broader runtime-safety workstream remains unchanged and is unrelated to M011.

## 11. Roadmap disposition

M011 is closed. Its closure satisfies the hard dependency for M012 (`plans/implementation/runtime-safety-resource-footprint/012-checked-undo-reapply.md`), which is now dependency-ready.

The broader runtime-safety, resource-control, and footprint roadmap remains `conditionally closed` per `plans/closure/runtime-safety-resource-footprint/010-status.md` due to the previously recorded supported-Linux Landlock fixture evidence condition. M011 does not alter that conditional disposition and does not require a new C003.

No new work is registered for browser-specific security, generic hook-taxonomy expansion, duplicate plugin/MCP runtimes, or opportunistic scheduling.

## 12. Registry updates

- Marked M011 `closed` via this record.
- Removed M011 from active/dependency-ready implementation plans; added M012 to `Dependency-ready implementation plans` as ready (hard dependency on M011 now satisfied).
- Updated `Blocked work` to remove the hard-block on M012.
- Updated `Active subsystem roadmaps` row for `Runtime safety — checked edit-history follow-up` to reflect M011 closed and M012 ready.
- Added this closure to `Recently closed or conditionally closed control points`.
- Left the existing `Runtime safety, resource control, and footprint` conditional closure (C002) unchanged; its single named supported-Linux evidence condition persists.

