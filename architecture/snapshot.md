# Snapshot Module

The `snapshot` module provides file state capture and restore
functionality for safety during tool execution. It owns three
related but distinct capture contracts:

- **Full safety snapshots** — full-project walks for explicit operator
  restore (`SnapshotManager::capture`).
- **Durable edit checkpoints** — exact pre/post file states for the
  supported native file-mutating tool surface, scoped to
  workspace/session/turn/batch (`EditCheckpointManager`).
- **Observational `FileChanged` events** — UI/diff notification on
  `GlobalEventBus`; not a durable provenance boundary.

## Purpose

Capture the state of project files before modifications so they can
be restored if needed. Supports both full-project captures and
incremental per-file captures. Edit checkpoints provide the durable
provenance needed for checked Undo/Reapply without relying on an
unscoped global event stream.

## Where It Lives

- **Core types & manager**: `crates/codegg-core/src/snapshot/mod.rs`
- **Checkpoint types & manager**: `crates/codegg-core/src/snapshot/checkpoint.rs`
- **Affected-path extraction**: `crates/codegg-core/src/snapshot/affected_paths.rs`
- **Diff computation**: `crates/codegg-core/src/snapshot/diff.rs`
- **DB schema**: `crates/codegg-core/src/session/schema.rs`
  (migration v13 for `snapshot`, v46 for `edit_checkpoint`)
- **Python script snapshots**: `src/python_script/snapshot.rs`
  (separate, metadata-only)

The `python_script` module has its own lightweight
`WorkspaceSnapshot` that tracks file metadata (size, mtime) rather
than content. Do not confuse the two.

## How It Works

### Full Capture

`SnapshotManager::capture()` walks the project root using a
depth-first stack, collecting file contents up to configured limits.
Results are JSON-serialized into the `snapshot.data` column.

### Incremental Capture (legacy)

`SnapshotManager::capture_incremental()` accepts pre-collected
`(path, old_content)` pairs. It validates paths are safe (no `..`,
no absolute paths) and stores only valid entries. Returns `None` if
no files are valid. This path remains for compatibility but is no
longer the authority for durable edit history.

### Durable Edit Checkpoints (canonical)

`EditCheckpointManager` owns pre/post capture for supported native
mutators. The canonical boundary is `ToolBatchExecutor` in
`src/agent/tool_batch.rs`:

```
accepted native mutating batch
  -> derive bounded affected-path set
  -> capture pre-state for all paths
  -> execute normal authorized tools (parallel, serialized if overlapping)
  -> capture post-state for the same paths
  -> persist EditCheckpoint with workspace/session/turn/batch identity
```

`FileChanged` remains published by file tools for UI diff
notification, but durable checkpoint contents no longer depend on
drained global events. A foreign workspace `FileChanged` event cannot
enter another turn's checkpoint because the checkpoint's path set is
derived from accepted structured tool arguments, not from the
unscoped event stream.

For an eligible restorable batch, the daemon-retained workspace-service
lease supplies the canonical `WorkspaceLockTable`. The repository guard is
acquired before the first pre-state read and held through native execution,
post-state capture, and checkpoint persistence, then released by RAII. This
prevents independent sessions targeting one workspace from contributing to
one another's checkpoint interval while keeping unrelated workspaces
concurrent. A batch containing a supported native mutation plus an
unknown/potentially mutating call is non-restorable as a whole and persists no
native subset checkpoint. Affirmatively read-only calls may accompany native
mutations only when the existing effect classifier marks them read-only.

### Restore

`restore()` and `restore_to_path()` are available but **not
automatically called on tool failure**. They must be triggered
explicitly (e.g., via a `/restore` command or direct API call).

Restore uses an atomic write pattern (temp file + rename) and
validates path containment via `ensure_contained_parent()` which
rejects symlinks and canonicalizes paths.

## Key Types & APIs

### SnapshotOptions (`crates/codegg-core/src/snapshot/mod.rs:9`)

```rust
pub struct SnapshotOptions {
    pub max_files: usize,        // default: 5_000
    pub max_file_bytes: u64,     // default: 1_000_000 (1MB)
    pub max_total_bytes: u64,    // default: 20_000_000 (20MB)
}
```

Zero values are clamped to 1 with a warning.

### FileSnapshot (`:26`)

```rust
pub struct FileSnapshot {
    pub path: String,
    pub content: String,
    pub hash: String,       // SHA-256 hex
    pub timestamp: i64,
}
```

### Snapshot (`:34`)

```rust
pub struct Snapshot {
    pub id: String,
    pub session_id: String,
    pub created_at: i64,
    pub label: Option<String>,
    pub data: String,       // JSON HashMap<String, FileSnapshot>
}
```

### SnapshotView (`:43`)

```rust
pub struct SnapshotView {
    pub id: String,
    pub session_id: String,
    pub files: HashMap<String, FileSnapshot>,
    pub created_at: i64,
    pub label: Option<String>,
}
```

### SnapshotManager (`:52`)

| Method | Line | Description |
|--------|------|-------------|
| `new(pool, root)` | :59 | Default options |
| `new_with_options(pool, root, opts)` | :67 | Custom options |
| `capture(session_id, label)` | :92 | Full project capture |
| `capture_incremental(sid, label, changes)` | :129 | Incremental capture (legacy) |
| `get(id)` | :194 | Fetch by ID |
| `list_for_session(sid)` | :218 | List all for session |
| `latest(sid)` | :241 | Latest for session |
| `restore(snapshot)` | :280 | Restore to project root |
| `restore_to_path(snapshot, target)` | :337 | Restore to custom path |
| `delete_snapshot(id)` | :392 | Delete by ID |
| `delete_all_for_session(sid)` | :401 | Delete all for session |

### EditCheckpoint (`checkpoint.rs`)

```rust
pub enum FileState {
    Absent,
    Present { hash: String, content: String },
}
pub struct EditFileState {
    pub path: String,
    pub pre: FileState,
    pub post: FileState,
}
pub struct EditCheckpoint {
    pub id: String,
    pub workspace_id: String,
    pub session_id: String,
    pub turn_id: Option<String>,
    pub batch_seq: i64,
    pub created_at: i64,
    pub files: Vec<EditFileState>,
}
```

`FileState::Absent` represents create/delete; `Present` carries
SHA-256 hash and bounded content. Existing snapshot size/path/symlink
limits are enforced.

### EditCheckpointManager (`checkpoint.rs`)

| Method | Description |
|--------|-------------|
| `new(pool, root)` / `new_with_options` | Same bounds as `SnapshotManager` |
| `capture_file_state_sync(path)` | Read single relative path -> `FileState` (validates safe path, symlink, size, UTF-8) |
| `capture_states(paths)` | Async bulk pre/post capture for a bounded path set |
| `persist_checkpoint(cp)` | Validate and insert `edit_checkpoint` row (rejects unsafe/oversized, enforces total bytes) |
| `get(id)` / `list_for_session` / `list_for_workspace` / `latest_for_session` | Durable retrieval; survives daemon restart |

### Affected-Path Extraction (`affected_paths.rs`)

Centralized derivation of the complete affected path set from accepted
structured tool arguments:

- `write`: one target path; pre may be absent/present; post present
- `edit`: one existing path
- `replace`: one existing path
- `multiedit`: one existing path
- `apply_patch update`: one existing path
- `apply_patch create`: one target path (absent -> present)
- `apply_patch delete`: one existing path (present -> absent)
- `apply_patch move`: both source and destination (including destination pre-state)

`extract_affected_paths(tool, input)` returns the raw paths for a
single tool call; `extract_batch_affected_paths` aggregates a batch;
`normalize_and_dedup` enforces safe relative containment, deduplicates,
and rejects `..`/absolute escapes. Malformed move/create/delete
arguments return `AffectedPathError` and mark the batch non-restorable
rather than producing an incomplete checkpoint.

`is_restorable_tool(name)` is the central eligibility predicate;
checkpoint eligibility derives from it so new native mutators cannot
silently bypass coverage.

### Diff Module (`crates/codegg-core/src/snapshot/diff.rs`)

```rust
pub struct FileDiff { pub path: String, pub hunks: Vec<DiffHunk> }
pub struct DiffHunk { pub old_start, new_start, lines }
pub struct DiffLine { pub kind: DiffKind, pub content: String }
pub enum DiffKind { Context, Added, Removed }
```

Functions:
- `diff_files(old, new, path)` (:29) — structured diff
- `format_unified_diff(old, new, old_path, new_path)` (:130) — unified format

Uses the `similar` crate for text diffing.

## Database Schema

```sql
CREATE TABLE IF NOT EXISTS snapshot (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    label TEXT,
    data TEXT NOT NULL,
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS snapshot_session_idx ON snapshot(session_id);

CREATE TABLE IF NOT EXISTS edit_checkpoint (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    session_id TEXT NOT NULL,
    turn_id TEXT,
    batch_seq INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    data TEXT NOT NULL, -- JSON Vec<EditFileState>
    FOREIGN KEY (session_id) REFERENCES session(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_edit_checkpoint_workspace ON edit_checkpoint(workspace_id);
CREATE INDEX IF NOT EXISTS idx_edit_checkpoint_session ON edit_checkpoint(session_id, created_at DESC);
```

Defined in migrations v13 and v46 (`session/schema.rs`). Both tables
coexist; `edit_checkpoint` reuses the same file-state serialization
and size limits as `snapshot` but adds explicit
`workspace_id`/`turn_id`/`batch_seq` provenance. Legacy `snapshot`
records remain readable after the v46 migration.

## Security

### Path Traversal Prevention

- `is_safe_relative_path()` rejects `..`, root dir, Windows
  prefixes, and empty paths.
- `ensure_contained_parent()` validates parent directories
  after `mkdir` but before writing, rejecting symlinks and checking
  canonical containment. This shrinks the TOCTOU window.
- `capture_incremental()` and `capture_states()` reject absolute paths
  and unsafe relative paths before storing.

### Atomic Write Pattern

`restore()` and `restore_to_path()` write to a `.tmp` file first,
then atomically rename. This prevents partial writes on interruption.

### Content Bounds

`SnapshotOptions` limits (`max_files`, `max_file_bytes`,
`max_total_bytes`) are enforced for both `snapshot` and
`edit_checkpoint`. Oversized or non-UTF-8 content causes the batch to
be marked non-restorable rather than storing a partial checkpoint.
Symlinks are rejected.

## Configuration Surface

```json
{
  "snapshot": true,
  "snapshot_config": {
    "max_files": 5000,
    "max_file_bytes": 1000000,
    "max_total_bytes": 20000000
  }
}
```

`snapshot: true` gates the expensive full-project walk. Edit
checkpoints are lightweight per-file captures distinct from the full
walk and are enabled whenever a pool is present so mutation
attribution remains correct even when full snapshots are disabled.
The same `snapshot_config` bounds apply to both.

## Invariants & Gotchas

- **No automatic rollback**: Snapshots are captured for safety but
  `restore()` is not called automatically on tool failure.
- **Durable checkpoints are workspace/session/turn/batch scoped**:
  Paths are resolved relative to one explicit execution workspace;
  no checkpoint may include a mutation from another session/workspace/turn.
  Pre-state is captured before the first mutation; post-state from the
  same bounded set after execution.
- **Create/delete/move are Absent/Present**: Not empty-file equivalence.
- **Observational vs durable**: `FileChanged` is retained for TUI diff
  notification but is not the durable provenance source. Overlap within a
  batch serializes (effective_max = 1) so checkpoint A's post cannot
  ambiguously become checkpoint B's pre.
- **Non-restorable is explicit**: Tools not in the checkpoint contract
  (bash/shell, plugin/MCP writes, git mutations, binary content beyond
  safe snapshot behavior, oversized) are never implicitly treated as
  safely captured. Malformed move/create/delete args cannot produce
  incomplete checkpoints.
- Excluded directories: `.git`, `node_modules`, `target`, `.codegg`.
- Binary files (non-UTF-8) are rejected for checkpoint pre/post (batch
  non-restorable) and silently skipped during full capture.
- Empty files are captured with a hash of the empty string.
- `capture()` uses `spawn_blocking` for the filesystem walk.
- `restore()` also uses `spawn_blocking` with a oneshot channel to
  bridge sync/async.
- Both tables FK cascade on session delete; `edit_checkpoint` is indexed
  by workspace_id for scoped retrieval.

### Checked Undo/Reapply (M012)

Checked restore operates over durable `edit_checkpoint` records with
compare-before-mutate semantics:

```
Undo(checkpoint): verify every current path == post, then atomically restore pre
Reapply(checkpoint): verify every current path == pre, then atomically restore post
```

- **All-path preflight**: every path in the checkpoint is validated
  (`is_safe_relative_path`, symlink, size, UTF-8) and compared against the
  expected side (hash equality) before the first write. Any stale/conflicting
  path fails the whole logical operation with zero mutation and a bounded
  `stale_paths` set (no file bodies leaked).
- **Workspace-scoped authority**: checkpoint IDs are not bearer capabilities.
  The daemon resolves `workspace_id`/`session_id` explicitly via
  `WorkspaceRegistry` and validates that the stored `workspace_id` matches the
  requested workspace; `WrongWorkspace`/`WrongSession` are typed failures.
  Every stored relative path is re-validated at restore time and resolved
  against the explicit `workspace_root` (no implicit `current_dir`).
- **Same mutation safety as normal writes**: `apply_file_state` reuses the
  `restore_file_checked` atomic temp-file+rename path, `O_NOFOLLOW`,
  canonical containment, and parent symlink checks. `Absent` deletes the file
  only after the same containment/parent checks.
- **Unsupported is explicit**: shell/bash, plugin/MCP, git, binary/oversized,
  and malformed move args never produce a checkpoint and are reported as
  non-restorable rather than partially undone. Old snapshots lacking pre/post
  are not treated as edit checkpoints.
- **Idempotent lineage**: after a successful `Undo`, the file system is at
  `pre`, so a duplicate `Undo` sees `current != post` and returns
  `Conflict` without double-applying. `Reapply` is the inverse and succeeds
  only when `current == pre`. The operation is logged durably in
  `edit_restore_operation` so `Reapply` of the latest undone checkpoint
  survives daemon restart without in-memory stack authority.
- **Contention**: the daemon acquires the narrow per-workspace
  `WorkspaceLockTable::acquire_repository(workspace_root)` before the final
  capture/compare and holds it through apply, so a concurrent CodeGG edit
  cannot race between compare and write. No daemon-global lock is used;
  independent workspaces remain independent.
- **Partial I/O after validation**: cross-file atomicity is not available
  from the filesystem. If an unexpected I/O error occurs after writes begin,
  the operation stops, returns `PartialFailure` with exactly `applied_paths`
  and `failed_paths`, does not advance logical lineage, and preserves audit
  evidence for operator recovery. The ordinary stale case is caught before
  this phase and produces zero mutations.
- **Bounded audit**: `edit_restore_operation` stores `checkpoint_id`,
  `workspace_id`, `session_id`, `direction`, `result`, bounded
  `conflict_paths`/`applied_paths`/`failed_paths` (JSON arrays), and
  `error_message`. `CheckpointSummary` exposed over the protocol carries
  only `id`, `workspace_id`, `session_id`, `turn_id`, `batch_seq`,
  `created_at`, `file_count`, `paths`, `restorable` — no file bodies.
- **Protocol surface**: `CoreRequest::EditCheckpointList`,
  `EditCheckpointGet`, `EditCheckpointUndo`, `EditCheckpointUndoLatest`,
  `EditCheckpointReapply`, `EditCheckpointReapplyLatest` and
  `CoreResponse::EditCheckpointList`, `EditCheckpointDetail`,
  `EditCheckpointUndoResult`, `EditCheckpointReapplyResult`
  (`EditRestoreResultDto` tagged `kind` with `applied`/`conflict`/
  `not_found`/`wrong_workspace`/`wrong_session`/`path_validation_failed`/
  `partial`/`unsupported`). The TUI exposes `/edit-undo [id]`,
  `/edit-reapply [id]`, `/edit-checkpoints` via the same core service
  (no direct file writes from the frontend).

## Testing

```bash
cargo test -p codegg-core -- snapshot
cargo test --test snapshot
cargo test --test edit_checkpoint_integration
cargo test --test checked_restore_integration
```

Integration coverage includes write/edit/replace/multiedit and every
`apply_patch` mode, failed/partial mutation, two-workspace isolation
( same relative filename in different workspace roots remains isolated),
concurrent-batch isolation, foreign `FileChanged` contamination
regression, overlapping-path serialization, symlink/oversize rejection,
and restart reload (recreating manager from same pool still reads
persisted checkpoints).

Checked-restore coverage (M012) includes create/update/delete/move
Undo/Reapply matrix, stale one-of-many prevents all mutation,
human/external edit conflict, wrong-workspace/session rejection,
duplicate/idempotent behavior, path-traversal/symlink rejection,
no-file-bodies in conflict output, workspace isolation, restart
durability (undo -> restart -> reapply), latest-undone lineage,
partial degraded handling, and concurrent undo serialization via the
per-workspace lock. TUI slash commands `/edit-undo`, `/edit-reapply`,
`/edit-checkpoints` drive the same core service (verified by
`checked_restore_integration` plus `cargo test tui`).

## Related Docs

- [agent.md](agent.md) — integration with agent loop and ToolBatchExecutor
- [tool.md](tool.md) — file-modifying tools and mutation surface
