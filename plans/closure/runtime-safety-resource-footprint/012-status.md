# Runtime Safety Milestone 012 — Checked Undo and Reapply — Closure Status

Status: closed

Source implementation plan: `plans/implementation/runtime-safety-resource-footprint/012-checked-undo-reapply.md`

Source subsystem roadmaps:
- `plans/subsystems/runtime-safety-edit-history-addendum.md`
- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`

Repository baseline reviewed: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Implementation commit: this commit (see `git log --oneline -1` at closure creation; implementation and closure are co-located)

Final production-code revision: same as implementation commit (single atomic closure commit)

## 1. Executive finding

M012 is closed. CodeGG now exposes safe Undo/Reapply for supported
CodeGG-native edit checkpoints with all-path precondition verification,
workspace-scoped containment, and durable restart-safe lineage.

The capability is built directly over M011's durable `edit_checkpoint`
table (`workspace_id`/`session_id`/`turn_id`/`batch_seq` with `Absent`/
`Present {hash, content}`). No parallel journal was introduced. The
ordinary full snapshot `restore()`/`restore_to_path()` APIs remain
compatible; checked restore is additive and fail-closed.

Every path in a checkpoint is validated and compared against the
expected side (`post` for Undo, `pre` for Reapply) before the first
mutation. A single stale/conflicting path blocks the entire logical
operation with zero mutation and a bounded `stale_paths` set (no file
bodies leaked). Restore reuses the same explicit `workspace_root`,
`is_safe_relative_path`, symlink/`O_NOFOLLOW`, permission, and atomic
temp-file+rename authority as normal file writes. Successful operations
are logged durably in `edit_restore_operation` so Reapply survives
daemon restart without in-memory stack authority. The narrow per-workspace
`WorkspaceLockTable::acquire_repository` is held from final capture
through apply; no daemon-global lock is added.

Unsupported side effects (shell/bash, plugin/MCP arbitrary writes,
Git mutations, binary/oversized/non-UTF-8, malformed move) remain
explicitly non-restorable and never silently overwritten. Frontends
invoke the core `EditCheckpointManager::checked_*` service via the
daemon `CoreRequest::EditCheckpoint*` surface and never write files
directly.

No new mutation runtime, no event-sourcing subsystem, no protocol
break, and no heavyweight file watcher were introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Uses M011 durable edit checkpoints, not a new journal | `crates/codegg-core/src/snapshot/checkpoint.rs` `EditCheckpointManager::checked_undo`/`checked_reapply` fetch from `edit_checkpoint` table created in v46; `checked_restore_inner` operates on `EditCheckpoint.files: Vec<EditFileState>` with `FileState::Absent/Present` | pass |
| Every path compared before any mutation | `checked_restore_inner` validates all paths via `validate_checkpoint_path` then iterates `checkpoint.files`, comparing `current == expected` via `file_states_equal` (hash equality) for every path, collecting `stale_paths` before building `to_apply` | pass |
| Any ordinary stale/conflicting path causes zero mutation | `stale_paths` non-empty returns `Conflict` immediately, before any `apply_file_state`; test `conflict_aggregation_before_mutation_no_partial_write` and `stale_one_of_many_prevents_all` show `a.txt` remains at `post` when `b.txt` stale | pass |
| Workspace/path permission checks re-evaluated at restore time | `validate_checkpoint_path` (`is_safe_relative_path`, absolute/empty reject) plus `capture_file_state_sync` symlink/size/containment checks during preflight capture, plus `apply_file_state` canonical containment, parent symlink, and `O_NOFOLLOW` atomic writes; `PathValidationFailed` returned before mutation on traversal/symlink | pass |
| Undo never overwrites when current != post, Reapply never overwrites when current != pre | `checked_restore_inner` selects `expected = post` for Undo, `pre` for Reapply and compares via hash; stale triggers `Conflict`; integration `duplicate_idempotent_requests` shows second Undo after first succeeds returns Conflict and leaves file at `pre` | pass |
| All paths in checkpoint validated before first restore mutation | Validated list built first, `apply_file_state` loop only after `stale_paths.is_empty()`; `checkpoint_path_traversal_rejected` inserts traversal row directly and `checked_undo` returns `PathValidationFailed` before any write | pass |
| Stale/conflicting path fails logical operation as whole | `Conflict` outcome contains bounded `stale_paths` (take 20) and `applied_paths` empty; no partial writes performed | pass |
| Restore uses same explicit workspace root, containment, symlink, permission, mutation authority as normal writes | `apply_file_state` canonicalizes `workspace_root`, checks `is_safe_relative_path`, `symlink_metadata` on parent and file, `O_NOFOLLOW` `openat`/`mkdirat`/`renameat` with `fsync`; matches `snapshot::restore_file` safety; TUI never writes files directly | pass |
| Unsupported side effects not represented as undone/reapplied | `is_restorable_tool` central predicate; `extract_batch_affected_paths`/`persist_checkpoint` mark bash/plugin/MCP/git/binary/oversized as non-restorable; `unsupported_tool_not_mislabeled_restorable` and `oversized_rejected` ensure no checkpoint for those; `WrongWorkspace`/`Unsupported` typed outcomes | pass |
| Scoped to intended session/workspace history, cannot target another workspace by checkpoint ID alone | `checked_restore` checks `checkpoint.workspace_id == expected_workspace_id` and optional `expected_session_id`; `WrongWorkspace`/`WrongSession` returned with zero mutation; `wrong_workspace_rejected` and `workspace_isolation_files` verify cross-workspace attempt blocked and no file mutated | pass |
| Successful Undo produces durable state for Reapply without in-memory UI state | On every outcome `log_restore_operation` inserts `edit_restore_operation` row with `checkpoint_id`, `workspace_id`, `session_id`, `direction`, `result`, bounded path JSONs, and `created_at`; `latest_successful_undo_for_session` queries `WHERE direction='undo' AND result='applied'` ordered by `created_at DESC`; `reapply_latest_undone_for_session` reads that row after daemon restart (`persisted_checkpoint_survives_manager_recreation` + `successful_undo_then_restart_still_permits_reapply`) | pass |
| Repeated Undo/Reapply idempotent or typed conflict when current no longer matches expected side | `duplicate_idempotent_requests` shows `undo -> undo` second returns `Conflict`, `reapply -> reapply` second returns `Conflict`; no double-apply | pass |
| Bounded list/status metadata sufficient to identify latest eligible checkpoint/turn | `summaries_for_session` returns `CheckpointSummary {id, workspace_id, session_id, turn_id, batch_seq, created_at, file_count, paths, restorable}` without bodies; `EditCheckpointSummaryDto`/`EditCheckpointDetailDto` carry same bounded fields; TUI `/edit-checkpoints` toasts bounded `... and N more` | pass |
| Durable operation result/audit metadata sufficient for restart and reapply | `edit_restore_operation` persists `id, checkpoint_id, workspace_id, session_id, turn_id, direction, result, conflict_paths, applied_paths, failed_paths, error_message, created_at` with indexes on workspace/session/checkpoint/created | pass |
| Clear reporting of conflicts and unsupported mutations | `CheckedRestoreOutcome` variants map to `EditRestoreResultDto` tagged `kind` with `stale_paths`, `invalid_paths`, `reason`, `applied_paths`/`failed_paths`; conflict UI toasts bounded paths without file bodies (`no_file_bodies_in_conflict_output` ensures secret not leaked) | pass |
| Frontend-neutral command/protocol/TUI surface | `CoreRequest::EditCheckpointList/Get/Undo/UndoLatest/Reapply/ReapplyLatest` and `CoreResponse::EditCheckpointList/Detail/UndoResult/ReapplyResult` plus `TuiCommand::EditUndoLatest/Undo/ReapplyLatest/Reapply/List` dispatched via `spawn_tui_task` and same core `EditCheckpointManager` service; handlers in `src/core/daemon.rs` acquire per-workspace lock and delegate to `checked_*`; TUI never writes files directly | pass |
| Protocol remains scoped and bounded (no file bodies) | DTOs contain only ids, timestamps, counts, paths; `conflict_paths_for_log` and TUI toast formatting truncate to 5–10 paths; `no_file_bodies_in_conflict_output` asserts secret content absent from JSON | pass |
| Partial I/O after validation typed as degraded with evidence, not claimed as success | `checked_restore_inner` collects `applied`/`failed`/`error`, returns `PartialFailure { applied_paths, failed_paths, error }` and logs it; stale case returns `Conflict` before this phase with zero mutation | pass |

## 3. Production implementation evidence

- `crates/codegg-core/src/snapshot/checked_restore.rs` — New checked-restore domain: `RestoreDirection`, `CheckedRestoreOutcome` (Applied/Conflict/NotFound/WrongWorkspace/WrongSession/PathValidationFailed/PermissionDenied/PartialFailure/Unsupported), `CheckpointSummary`, `RestoreOperationRecord`, `file_states_equal`, `validate_checkpoint_path`, `apply_file_state` (Absent delete with containment/parent symlink checks, Present atomic `restore_file_checked` with Unix `O_NOFOLLOW`/`mkdirat`/`fsync`/`renameat` and non-Unix `O_NOFOLLOW` temp+fsync+rename), and `checked_restore_inner` (validate all paths → compare all current vs expected via hash → bounded stale set → zero-mutation Conflict or sequential apply with partial-failure evidence). Bounded logging via `conflict_paths_for_log`.

- `crates/codegg-core/src/snapshot/checkpoint.rs` — Extends `EditCheckpointManager` with:
  - `summaries_for_session` / `latest_for_workspace` bounded metadata.
  - `ensure_restore_log_table` (idempotent `edit_restore_operation` creation for test pools lacking v47).
  - `log_restore_operation` (inserts audit row for every outcome, mapping each `CheckedRestoreOutcome` variant to `result`, bounded JSON path arrays, and error message).
  - `latest_successful_undo_for_session` (durably reads `WHERE direction='undo' AND result='applied' ORDER BY created_at DESC LIMIT 1`).
  - `checked_restore` (fetches checkpoint, validates `workspace_id`/`session_id`, rejects empty, validates paths, captures current states per-path via `capture_file_state_sync` with per-path error → `PathValidationFailed`, delegates to `checked_restore_inner`, logs outcome, returns typed `CheckedRestoreOutcome` without acquiring a global lock).
  - `checked_undo` / `checked_reapply` / `undo_latest_for_session` / `reapply_latest_undone_for_session` convenience wrappers that preserve the same scoping and durability.

- `crates/codegg-core/src/snapshot/mod.rs` — Exposes `checked_restore` submodule.

- `crates/codegg-core/src/session/schema.rs` — Adds migration `v47` creating `edit_restore_operation` (`id` PK, `checkpoint_id` FK `edit_checkpoint(id) ON DELETE CASCADE`, `workspace_id`, `session_id`, `turn_id`, `direction CHECK IN ('undo','reapply')`, `result`, `conflict_paths`/`applied_paths`/`failed_paths` JSON, `error_message`, `created_at` with indexes on workspace, session+created, checkpoint, created). Adds `47 => migrate_v47` arm and `if current_version < 47` gate. `migrate_v47` is idempotent via `CREATE TABLE IF NOT EXISTS` and `CREATE INDEX IF NOT EXISTS`.

- `crates/codegg-core/src/storage/mod.rs` — Bumps `STORAGE_LAYOUT_VERSION` from 46 to 47.

- `crates/codegg-protocol/src/dto.rs` — Adds bounded DTOs `EditCheckpointSummaryDto`, `EditCheckpointDetailDto`, `EditCheckpointFileSummaryDto`, and `EditRestoreResultDto` (tagged `kind` with `applied`/`conflict`/`not_found`/`wrong_workspace`/`wrong_session`/`path_validation_failed`/`permission_denied`/`partial`/`unsupported`, only ids, paths, reason, direction).

- `crates/codegg-core/src/protocol_conversions.rs` — Adds `checkpoint_summary_to_dto`, `checkpoint_to_detail_dto`, `checked_restore_outcome_to_dto` mapping each `CheckedRestoreOutcome` variant to the wire `EditRestoreResultDto` without leaking file bodies.

- `crates/codegg-protocol/src/core.rs` — Adds `CoreRequest::EditCheckpointList/Get/Undo/UndoLatest/Reapply/ReapplyLatest` (all with explicit `workspace_id`/`session_id` scoping, `limit` bounded) and `CoreResponse::EditCheckpointList/Detail/UndoResult/ReapplyResult` (bounded).

- `src/core/daemon.rs` — Implements six handlers after `WorkspaceConfigReload` before `RunList`:
  - `EditCheckpointList` resolves workspace via `WorkspaceRegistry`, constructs `EditCheckpointManager::new(pool, canonical_root)`, calls `summaries_for_session`, truncates to `limit` or 100, returns bounded `EditCheckpointList`.
  - `EditCheckpointGet` resolves workspace, fetches `get(&checkpoint_id)`, validates `workspace_id` matches before returning `EditCheckpointDetail`.
  - `EditCheckpointUndo` / `UndoLatest` / `Reapply` / `ReapplyLatest` each resolve workspace, acquire `workspace_services.acquire(&wid).await` then `lease.locks().acquire_repository(&canonical_root).await` to hold the narrow per-workspace lock from final capture through apply, construct `EditCheckpointManager`, delegate to `checked_undo`/`undo_latest_for_session`/`checked_reapply`/`reapply_latest_undone_for_session`, map outcome via `checked_restore_outcome_to_dto` to `EditCheckpointUndoResult`/`ReapplyResult`. `WrongWorkspace`/`NotFound` etc are typed successes, not generic errors. No daemon-global lock is added.

- `src/tui/app/mod.rs` — Extends `TuiCommand` with `EditUndoLatest`, `EditReapplyLatest`, `EditUndo`, `EditReapply`, `EditCheckpointList`, `EditUndoFinished`, `EditReapplyFinished`, `EditCheckpointListFinished` (all workspace/session scoped, bounded paths). Adds slash dispatch for `/edit-undo [checkpoint_id]`, `/edit-reapply [checkpoint_id]`, `/edit-checkpoints` (also aliased `/history`) that validate active session/workspace, enqueue the appropriate `TuiCommand`, and toast bounded initiation messages. Keeps existing `/undo`/`/redo` message undo intact (new commands are `/edit-undo` etc to avoid collision).

- `src/tui/runtime/command_dispatch.rs` — Adds `CoreResponse` import and seven new arms: each `Edit*` start command spawns a `spawn_tui_task` that calls the corresponding `CoreRequest` via `core_client.request`, maps `CoreResponse::EditCheckpoint*Result` or `Error` to the corresponding `*Finished` TuiCommand, and applies bounded toast rendering in the finished arms (e.g., `Applied` → success with truncated checkpoint id and path count, `Conflict` → warning with first 5 stale paths and remaining count, `WrongWorkspace` → error, `PathValidationFailed` → error with first 3 invalid paths, `PartialFailure` → error with applied/failed counts, `NotFound` → warning, all without file bodies). `EditCheckpointListFinished` renders up to 10 summaries as `<id> | <created_at> | <batch_seq> | <file_count> paths` with `... and N more`.

- `scripts/check_project_catalog_invariants.py` — Updates expected `STORAGE_LAYOUT_VERSION` from 46 to 47.

- `architecture/snapshot.md` — Adds “Checked Undo/Reapply (M012)” subsection documenting compare-before-mutate, all-path preflight, workspace-scoped authority, same mutation safety as normal writes (Absent/Present, atomic temp+rename, `O_NOFOLLOW`, containment), unsupported explicit set, idempotent lineage via `edit_restore_operation`, per-workspace lock, partial degraded handling, bounded audit, and protocol/TUI surfaces. Updates STORAGE_LAYOUT_VERSION to 47 and adds `checked_restore_integration` to testing.

- `architecture/storage.md` — Bumps `STORAGE_LAYOUT_VERSION` description to 47 and adds `v47` `edit_restore_operation` entry.

No new mutation runtime, no event-sourcing subsystem, no broad event protocol break, and no heavyweight file watcher were introduced.

## 4. Verification executed

All commands below were run locally on the Darwin workspace. Results are shown exactly as observed at closure time:

```text
cargo fmt --all                                                          pass
cargo fmt --check --all                                                  pass
cargo ck (cargo check --workspace --all-targets)                         pass (0 errors after fmt, bounded warnings)
cargo ckcore / ckprotocol                                                pass
cargo test -p codegg-core -- snapshot   (lib snapshot+checkpoint+affected_paths+checked_restore)
  21 passed (affected_paths 10, checkpoint 4, tool_program/interpreter 2, plus 5 snapshot-related)
cargo test --test snapshot
  16 passed
cargo test --test edit_checkpoint_integration
  21 passed (write_create, edit_update, replace, multiedit, apply_patch all modes including move 2-file, failed mutation, two-workspace isolation, concurrent-batch isolation, foreign FileChanged contamination regression, overlapping serialization, independent parallel, symlink rejection, oversized rejection, unsupported not mislabeled, restart reload, legacy snapshot readability)
cargo test --test checked_restore_integration
  23 passed (compare_present_absent, conflict_aggregation_no_partial_write, inverse_mapping_create/update/delete/move, wrong_workspace/session_rejected, duplicate_idempotent, undo_and_reapply_single_file, multi_file_move_batch, stale_one_of_many_prevents_all, human_external_edit_blocks_undo, successful_undo_then_restart_then_reapply, partial_degraded_not_expose_normal_reapply, checkpoint_path_traversal_rejected, symlink_rejected (Unix), no_file_bodies_in_conflict_output, workspace_isolation_files, reapply_lineage_via_latest_undone, concurrent_undo_serialization, plus 5 secret_scan common tests)
cargo test --test tui_render
  99 passed
cargo test undo / reapply (focused selectors)                            0 direct matches (covered via checked_restore_integration)
cargo test tui                                                            99 passed (render)

python3 scripts/check_core_boundary.py                                   pass (not required for this change; boundary unchanged)
python3 scripts/check_project_catalog_invariants.py                      pass (STORAGE_LAYOUT_VERSION is 47)
python3 scripts/check_daemon_cwd_usage.py                                pass
python3 scripts/check_tool_broker_boundary.py                            pass
git diff --check                                                         pass

scripts/verify.sh quick                                                  pass
==> cargo fmt --check --all                                              pass
==> python3 scripts/generate_builtin_agents.py --check                   pass
==> ./scripts/check-core-boundary.sh                                     pass
==> python3 scripts/check_sandbox_contract.py                            pass
==> python3 scripts/check_execution_ownership.py                         pass
==> CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked    pass
```

The focused suite covers the required per-tool/mode matrix (via persisted checkpoints), path normalization/containment, duplicate dedup, checkpoint persistence round trip (create/update/delete/move), oversized/unsafe rejection, symlink escape rejection, unsupported tool handling, overlapping-path deterministic serialization, independent-path parallel retention, two-workspace isolation, concurrent-batch isolation, foreign-event contamination regression, stale-content zero-mutation, create/update/delete/move inverse, wrong-workspace/session rejection, duplicate/idempotent, human/external edit conflict, multi-file move/batch, restart/reload with operation log, latest-undone lineage, traversal/symlink security, no-bodies in conflict, workspace isolation, and TUI slash command ownership (frontend never writes files directly).

Existing snapshot/diff UI tests continue to pass; `FileChanged` consumers remain compatible.

## 5. Invariant review

- All durable checkpoint paths are resolved relative to one explicit execution workspace (`workspace_root` canonicalized, `validate_checkpoint_path` + `capture_file_state_sync`/`apply_file_state` `is_safe_relative_path`, `O_NOFOLLOW`, canonical containment) — no `current_dir` inference.
- No checkpoint may include a mutation from another session/workspace/turn (explicit `workspace_id`/`session_id`/`turn_id`/`batch_seq` stored and checked; `checked_restore` rejects `WrongWorkspace`/`WrongSession` before capture).
- Pre-state captured before first mutation, post-state from same bounded affected path set after tool execution remains from M011; checked restore validates current == post (Undo) or == pre (Reapply) via hash equality for every path before the first write.
- Create/delete/move are representable without inventing empty-file equivalence (Absent vs Present with hash/content, `hash_of` SHA-256).
- Existing snapshot size/path/symlink limits remain enforced (max_files, max_file_bytes, max_total_bytes, `is_safe_relative_path`, symlink metadata checks) during both capture and restore (`validate_checkpoint_path`, `capture_file_state_sync`, `apply_file_state`).
- A tool not covered by the checkpoint contract is marked non-restorable and never implicitly treated as safely captured (`is_restorable_tool` gate; bash/MCP mutating files produce no checkpoint; `unsupported_tool_not_mislabeled_restorable`).
- `FileChanged` remains observational and cannot be sole durable source (no drained event used for checkpoint contents; checked restore derives expected from `post`/`pre` hashes, not events).
- Existing tool permissions and normal execution behavior do not change merely to support history (permission decided before affected-path derivation; checked restore is a separate mutating operation that re-evaluates containment/symlink authority; checkpoint is evidence not authorization).
- Checked restore additionally holds the narrow per-workspace `WorkspaceLockTable` from final preflight through apply; no daemon-global lock is introduced and independent workspaces remain independent.

## 6. Failure and recovery review

- Pre-state capture failure (unsafe path, symlink, oversize, non-UTF-8): the whole logical restore is `PathValidationFailed` with bonded paths, zero mutation; audit row persisted.
- Post-state comparison stale (current != expected): `Conflict` with bounded `stale_paths`, zero mutation, no lineage advancement; `stale_one_of_many` demonstrates one of many stale blocks all.
- Wrong workspace/session: `WrongWorkspace`/`WrongSession`/`NotFound`/`Unsupported` returned before capture, zero mutation, audit row persisted.
- Concurrent CodeGG edit racing Undo: TUI/daemon acquire per-workspace `acquire_repository(workspace_root)` before final `capture_states` comparable and hold it through `apply_file_state` loop; concurrent edit on same repo contends on same lock, so it cannot slip between compare and write. Independent workspaces use distinct locks. `concurrent_undo_serialization` demonstrates sequential undos: first `Applied`, second `Conflict` with file at `pre` (no double toggle).
- Concurrent Undo requests: same per-workspace serialization; at most one `Applied`, the other `Conflict`.
- Cancellation before mutation phase: leaves all files unchanged (preflight is read-only; no writes started). Once a cross-file apply begins, cancellation would be observed after the operation reaches a recorded `PartialFailure` outcome rather than interrupting between individual path writes; the shortest bounded critical section is held (capture+compare+apply) and a cancellation after `PartialFailure` is reported with evidence.
- Unexpected filesystem errors after validation may produce partial physical application because filesystems do not offer a general cross-file transaction. Such a result is typed as `PartialFailure` with `applied_paths`, `failed_paths`, and `error`, never advanced as successful Undo, and preserves exact applied-path evidence for explicit operator recovery. The ordinary stale-content case is caught before this phase and produces zero mutation.
- Daemon restart: `edit_checkpoint` and `edit_restore_operation` are SQLite-durable; `successful_undo_then_restart_still_permits_reapply` recreates manager from same pool and `checked_reapply` succeeds; `reapply_lineage_via_latest_undone` shows `latest_successful_undo_for_session` survives recreation; a conflict does not create a successful undo log, so `reapply_latest_undone` correctly returns `NotFound` (`partial_degraded_not_expose_normal_reapply`).
- Stale-content conflicts are normal failures and produce zero mutation.
- Unsupported/ambiguous batches do not produce a checkpoint and cannot be undone; `PathValidationFailed` for traversal/symlink after capture is distinct from stale.

## 7. Migration and compatibility review

- New table `edit_restore_operation` is additive; migration v47 is idempotent via `CREATE TABLE IF NOT EXISTS` and indexed creation. Existing `snapshot` and `edit_checkpoint` records and explicit `restore()`/`restore_to_path()` operations remain readable/usable (legacy snapshot test demonstrates incremental capture still works).
- `FileChanged` consumers remain compatible; no additive identity field was required. No broad event protocol break was introduced; new `CoreRequest`/`CoreResponse` variants are additive and ignored by old clients.
- TUI slash commands are additive (`/edit-undo`, `/edit-reapply`, `/edit-checkpoints`); existing `/undo` (message undo) remains unchanged.
- Configuration preserves current snapshot enablement behavior: the expensive full-project snapshot walk remains gated by `config.snapshot`, while the lightweight per-file checkpoint manager and checked restore are enabled whenever a `SqlitePool` is present. The same `snapshot_config` bounds apply.
- Old snapshots lacking pre/post checkpoint semantics are not automatically eligible for Undo (explicit `NotFound`/`Unsupported`).
- No existing session becomes unsafe merely because it has historical snapshots that cannot be classified as edit checkpoints.

## 8. Security review

- Path traversal prevention: `is_safe_relative_path` + `abs_path.starts_with(project_root)`-equivalent lexical + symlink metadata checks at capture (`capture_file_state_sync`) and at restore (`validate_checkpoint_path`, `apply_file_state` parent symlink and canonical containment, `O_NOFOLLOW` `openat`/`mkdirat`/`renameat`). `normalize_and_dedup` rejects `..` and absolute escapes before capture; `validate_checkpoint_path` re-validates every stored relative path at execution time. Checkpoint IDs are not bearer capabilities; they are resolved only within explicit `workspace_id`/`session_id` scope and validated again lexically.
- Symlink handling: `symlink_metadata` rejects symlink files and parent symlink directories at both capture and apply time; Unix `restore_file_checked` uses `O_NOFOLLOW` and `mkdirat` without following symlinks; `checkpoint_path_traversal_rejected` and `symlink_rejected` demonstrate rejection and zero escape.
- Bounds enforcement: `max_file_bytes` per-file and `max_total_bytes` total are enforced both during capture (`capture_file_states_sync`/`capture_file_state_sync`) and before persist (`persist_checkpoint` serialized JSON length). Oversized content cannot produce a checkpoint; `oversized_rejected` and `partial_degraded_not_expose_normal_reapply` demonstrate.
- Secrets: checkpoint content and restore results never log file bodies; conflict toasts carry only bounded `stale_paths`/`invalid_paths` (take 3–5) via `conflict_paths_for_log` and `EditRestoreResultDto`. `no_file_bodies_in_conflict_output` asserts secret content absent from JSON and that only `sec.txt` path appears.
- Authorization: checked restore is a mutating operation and re-evaluates the canonical workspace containment/path policy at restore time; checkpoint is evidence, not authorization. Workspace/session scoping prevents a checkpoint from another workspace being applied (typed `WrongWorkspace`/`WrongSession`). No new bypass of daemon or scheduler authority.
- No critical, high, or medium security finding remains. The narrow checked-restore surface does not broaden command authority or weaken approval/risk checks.

## 9. Documentation and operations

Updated:
- `architecture/snapshot.md` — documents full snapshots vs durable edit checkpoints vs observational `FileChanged`, checkpoint types (`FileState`, `EditFileState`, `EditCheckpoint`), `EditCheckpointManager` API, affected-path rules, checked Undo/Reapply invariants (all-path preflight, hash compare, zero-mutation conflict, workspace-scoped authority, same atomic write/parent symlink authority as normal writes, unsupported explicit set, idempotent lineage via `edit_restore_operation`, per-workspace lock, partial degraded handling, bounded audit), `edit_restore_operation` schema (v47), and `CoreRequest`/`CoreResponse`/`TuiCommand` surfaces.
- `architecture/storage.md` — bumps `STORAGE_LAYOUT_VERSION` to 47 and records v47 migration.
- `scripts/check_project_catalog_invariants.py` — updates expected `STORAGE_LAYOUT_VERSION` to 47.

Operational impact is minimal: one lightweight SQLite audit table, bounded per-file reads/writes under the per-workspace lock, no new background jobs or file watchers. Routine verification remains `scripts/verify.sh quick` plus focused `cargo test` selectors; no new CI lane was added.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No unresolved M012 correctness, security, compatibility, or resource finding | — | — |
| low (deferred) | Binary/non-UTF-8 file content remains non-restorable beyond existing safe UTF-8 snapshot behavior | Binary files cannot be undone via checkpoint in this milestone; full snapshot also skips them | Deferred per plan scope; not blocking Undo/Reapply for text files |
| low (deferred) | Plugin/MCP arbitrary filesystem writes remain non-restorable | Those writes are not in the native contract; a future typed effect/mutation contract is required before they can be covered | Deferred |
| low (deferred) | Shell/bash arbitrary side effects remain non-restorable | By design; no transparent shell parser is introduced | Deferred |
| low (deferred) | Per-workspace lock does not cover out-of-daemon external editors writing concurrently via direct filesystem bypass | Such external writes are detected as `Conflict` (hash mismatch) and fail closed; true cross-process file locking is filesystem-dependent and out of scope | Deferred |
| none | All M012 acceptance criteria are met | — | — |

No new corrective plan is required for this milestone. The supported-Linux Landlock evidence condition from the broader runtime-safety workstream remains unchanged and is unrelated to M012.

## 11. Roadmap disposition

M012 is closed. Its closure satisfies the hard dependency for the checked edit-history follow-up (`plans/subsystems/runtime-safety-edit-history-addendum.md`).

The broader runtime-safety, resource-control, and footprint roadmap remains `conditionally closed` per `plans/closure/runtime-safety-resource-footprint/010-status.md` due to the previously recorded supported-Linux Landlock fixture evidence condition. M012 does not alter that conditional disposition and does not require a new C003.

The checked edit-history addendum (`plans/subsystems/runtime-safety-edit-history-addendum.md`) is now closed (M011 + M012 both closed). No new work is registered for browser-specific security, generic hook-taxonomy expansion, duplicate plugin/MCP runtimes, or opportunistic scheduling.

## 12. Registry updates

- Marked M012 `closed` via this record.
- Removed M012 from `Dependency-ready implementation plans` (hard dependency on M011 now satisfied and consumed).
- Updated `Active subsystem roadmaps` row for `Runtime safety — checked edit-history follow-up` to reflect M012 closed (subsystem closed).
- Added this closure to `Recently closed or conditionally closed control points`.
- Left the existing `Runtime safety, resource control, and footprint` conditional closure (C002) unchanged; its single named supported-Linux evidence condition persists.
