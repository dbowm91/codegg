# Run Store

Persistent, filesystem-backed run index + artifact storage for
command executions (shell, git, test, python, native tools).

## Purpose

Records structured execution metadata (who ran what, how, what
happened) and their artifacts (stdout, stderr, diffs, projections)
with JSONL indexing, SHA-256 integrity, retention cleanup, and
in-memory test doubles.

## Where It Lives

| Layer | Path |
|-------|------|
| Trait + types + impls | `crates/codegg-core/src/run_store.rs` (~2503 lines) |
| Error variants | `crates/codegg-core/src/error.rs:390-411` (`RunStoreError`) |
| Root re-export | `src/lib.rs:11` — `pub use codegg_core::run_store;` |
| Ownership by `WorkspaceServices` | `crates/codegg-core/src/workspace_services.rs` |

Storage root: `<workspace>/.codegg/runs/`.

## How It Works

### Lifecycle

1. **`begin_run(RunDraft)`** — Creates a `RunId` (UUID v4), builds a
   `RunManifest` with `Running` status, writes `manifest.json`
   atomically (fsync before rename), appends to `index.jsonl`, returns
   `RunHandle`.
2. **`write_artifact(RunHandle, ArtifactInput)`** — Validates size ≤ 64
   MiB, computes SHA-256, writes artifact file atomically, updates
   `manifest.json` with new `ArtifactRecord`, returns `ArtifactRef`.
3. **`complete_run(RunHandle, RunCompletion)`** — Updates manifest with
   terminal status, permissions, sandbox, projection, changes, rerun
   descriptor, actual_backend/fallback. Rewrites index entry under the
   serialization lock. Triggers best-effort retention cleanup.

### Directory Layout

```
<root>/
  index.jsonl              # One IndexEntry per line
  2026-07-10/
    <run-id>/
      manifest.json        # RunManifest
      stdout.log           # ArtifactKind::Stdout
      stderr.log           # ArtifactKind::Stderr
      invocation.json      # ArtifactKind::CommandSource
      diff.patch           # ArtifactKind::UnifiedDiff
      projection.txt       # ArtifactKind::Projection
      ...
```

### Integrity model

`read_artifact` reads the file, recomputes SHA-256, and compares against
the `ArtifactRecord.sha256` stored in the **persisted** `manifest.json`
(not a cache copy). Mismatches return `RunStoreError::IntegrityViolation`.

### Retention

`FsRunStore::cleanup` runs after each `complete_run` with default limits:
1 GiB total, 1000 runs, 30-day max age (60 days for failed/timed-out),
pinned runs exempt. Uses `FsRunStore::plan_cleanup` for dry-run.

## Key Types & APIs

### Identifiers

| Type | File:Line | Notes |
|------|-----------|-------|
| `RunId` | :24 | UUID v4 newtype, `Display`, `Default` |
| `ArtifactId` | :55 | Same pattern |

### Enums

| Type | File:Line | Variants |
|------|-----------|----------|
| `RunKind` | :206 | `RawShell`, `ManagedProcess`, `Test`, `GitRead`, `GitMutation`, `Search`, `Python`, `NativeTool` (8) |
| `RunStatus` | :236 | `Running`, `Complete`, `Failed`, `TimedOut`, `Cancelled`, `Incomplete` (6) |
| `ArtifactKind` | :262 | `Stdout`, `Stderr`, `CombinedLog`, `CommandSource`, `TestReport`, `TestLog`, `UnifiedDiff`, `ChangedFiles`, `Projection`, `RtkProjection`, `StructuredJson`, `PolicyEvidence` (12) |
| `ContextPromotionState` | :694 | `LocalOnly`, `ProjectionIncluded`, `ArtifactRangeIncluded`, `Pinned`, `Excluded` (5) |
| `RunOwnership` | :94 | `Caller`, `DelegatedBackend`, `ChildOf(RunId)` (3) |
| `PlannedBackend` | :111 | `Unrouted`, `RawShell`, `TestRunner`, `PythonScript`, `NativeTool`, `ManagedArgv`, `Git`, `GitMutating` (8, last deprecated) |
| `ActualBackend` | :154 | Same as PlannedBackend + `Rejected { reason }` (9) |

### Record Types

| Type | File:Line | Purpose |
|------|-----------|---------|
| `RunInvocation` | :281 | command, argv, script_hash |
| `BackendRecord` | :292 | family, detail |
| `RiskRecord` | :301 | level, has_subprocess, has_git_mutation, has_destructive_mutation |
| `PermissionDecisionRecord` | :311 | tool, path, decision |
| `SandboxRecord` | :321 | os_isolation, network_isolation, read_roots, write_roots |
| `ArtifactRecord` | :333 | artifact_id, kind, relative_path, mime_type, byte_length, sha256, truncated, redacted, created_at, safe_for_model |
| `ProjectionRecord` | :349 | projector, exactness, omitted_ranges, projection_id, source_spans, redaction_records, rtk_metadata, estimated_output_tokens, promotion_decision, input_digests |
| `ChangedPathRecord` | :419 | path, kind |
| `RerunDescriptor` | :427 | argv (AuditSafeArgv), script_source_ref, backend_family, cwd, workspace_root, mode, config_profile, parent_run_id |
| `FallbackRecord` | :194 | planned, actual, reason |
| `RunAssetProvenance` | :506 | generation, fingerprint, activated_skill_digests |

### Composite Types

| Type | File:Line | Purpose |
|------|-----------|---------|
| `RunManifest` | :456 | Full run descriptor (~22 fields) |
| `RunSummary` | :541 | Lightweight listing for `list_runs` |
| `RunDraft` | :555 | Input for `begin_run` |
| `RunHandle` | :576 | Returned by `begin_run` (run_id, run_dir, started_at) |
| `RunCompletion` | :583 | Input for `complete_run` |
| `RunQuery` | :604 | Filter for `list_runs` |
| `ArtifactInput` | :615 | Input for `write_artifact` |
| `ArtifactRef` | :623 | Returned by `write_artifact` |
| `ArtifactChunk` | :631 | Returned by `read_artifact` (supports ranged reads) |
| `ByteRange` | :639 | start, end |
| `RetentionConfig` | :647 | max_total_bytes, max_run_count, max_age_days, preserve_failed_longer, failed_extra_days |
| `CleanupPlan` | :668 | runs_to_delete, bytes_to_free, pinned_runs_skipped |
| `IndexEntry` | :677 | JSONL index record |

### View Models

| Type | File:Line | Purpose |
|------|-----------|---------|
| `RunCellView` | :718 | Compact TUI cell; `from_manifest()` computes capability flags |
| `RunDetailView` | :861 | Full detail overlay; `from_manifest()` |
| `RunInvocationView` | :873 | Command, argv, cwd, backend |
| `RunPermissionView` | :884 | Tool, path, decision |
| `RunPolicyView` | :891 | Risk + sandbox |
| `RunArtifactView` | :911 | Metadata only (no raw bytes) |
| `RunProjectionView` | :923 | projector, exactness, omitted_ranges |
| `RunChangeView` | :930 | path, kind |

### Trait

```rust
// run_store.rs:1028
#[async_trait]
pub trait RunStore: Send + Sync {
    async fn begin_run(&self, draft: RunDraft) -> Result<RunHandle, RunStoreError>;
    async fn write_artifact(&self, run: &RunHandle, artifact: ArtifactInput)
        -> Result<ArtifactRef, RunStoreError>;
    async fn complete_run(&self, run: RunHandle, completion: RunCompletion)
        -> Result<RunManifest, RunStoreError>;
    async fn get_run(&self, id: &RunId) -> Result<Option<RunManifest>, RunStoreError>;
    async fn read_artifact(&self, id: &ArtifactId, range: Option<ByteRange>)
        -> Result<ArtifactChunk, RunStoreError>;
    async fn list_runs(&self, query: RunQuery) -> Result<Vec<RunSummary>, RunStoreError>;
}
```

### Implementations

| Impl | File:Line | Backend |
|------|-----------|---------|
| `FsRunStore` | :1110 | Filesystem with JSONL index, `tokio::sync::Mutex<()>` serialization |
| `MemRunStore` | :1730 | In-memory `parking_lot::RwLock<HashMap>` |

### Constants

| Constant | Value | File:Line |
|----------|-------|-----------|
| `SCHEMA_VERSION` | `1` | :13 |
| `MAX_ARTIFACT_BYTES` | 64 MiB | :16 |
| `DEFAULT_MAX_TOTAL_BYTES` | 1 GiB | :17 |
| `DEFAULT_MAX_RUN_COUNT` | 1000 | :18 |
| `DEFAULT_MAX_AGE_DAYS` | 30 | :19 |
| `DEFAULT_FAILED_EXTRA_DAYS` | 30 | :20 |

## Integration Points

### Tool integration

| Location | How Used |
|----------|----------|
| `src/tool/mod.rs:242` | `ToolRegistryOptions.run_store: Option<Arc<dyn RunStore>>` |
| `src/tool/factory.rs:45-52` | Creates `FsRunStore` at `.codegg/runs/`, passes to tools |
| `src/tool/bash.rs:664-760` | Persists runs with correct `RunKind` per routing decision |
| `src/python_script/tool.rs:143-257` | Persists `Python` runs with diff/sandbox/changes |
| `src/test_runner/runner.rs:238-239` | Persists `Test` runs after each run |

### TUI integration

| Location | How Used |
|----------|----------|
| `src/tui/app/mod.rs:681` | `App.run_store: Option<Arc<dyn RunStore>>` |
| `src/tui/app/mod.rs:872-877` | Initializes `FsRunStore` at `.codegg/runs/` |
| `src/tui/components/dialogs/run_detail.rs` | `RunDetailDialog` — 7-tab detail view |

### Durable rerun

Rerun is a fresh scheduler submission, never an in-place mutation of the
historical manifest. The current supported class is a completed, failed, or
timed-out `RunKind::Test` whose `RerunDescriptor` has a non-empty
`test_runner`/audit-safe argv and no script source reference. `RunCellView`
derives `can_rerun` from those reconstructability properties; the daemon
revalidates status, session authority, canonical workspace identity, and cwd
before submission.

The child job is `JobKind::Test`, `SafeRepeat`, and carries the parent
`RunId`. The scheduler supplies the leased workspace RunStore to the test
executor, which persists a fresh child manifest with `parent_run_id` and
`RunOwnership::ChildOf`. Completion publishes `RunRerunLinked`. Redacted or
credential-dependent argv is rejected with an actionable reacquisition code;
raw secrets are never restored from durable metadata. Unsupported Git,
Python, shell, and worktree-dependent runs remain ineligible until they have
their own explicit reconstruction and credential contracts.

### Protocol events

`CoreEvent` variants in `crates/codegg-protocol/src/core.rs`:
`RunStarted`, `RunProgress`, `RunArtifactCreated`, `RunProjectionReady`,
`RunCompleted`, `RunDenied`, `RunPinned`, `ContextPromotionChanged`,
`RunRerunLinked`.

## Tool Programs Linkage (M003)

RunStore and ToolProgramStore are **separate authorities**:

- **RunStore** owns execution artifacts for concrete command runs.
- **ToolProgramStore** owns lifecycle records for agent-submitted
  Tool Programs (state, manifest, source/IR refs, call ledger).

A `ToolProgramRecord` may link to a RunStore run via
`ProgramCallRecord.child_run_id`, but the two stores are not atomically
coupled.

## Invariants & Gotchas

### `tokio::sync::Mutex` reentrancy

`FsRunStore.lock` is **not reentrant**. The single allowed pattern is:
acquire the lock once, call `rewrite_index_locked` (the `_locked` suffix
means "caller must hold `self.lock`"). Calling `self.lock.lock().await`
again from the same task **deadlocks permanently**. The historical
`fs_store_complete_updates_index` hang was caused by this.

### Integrity source

The authoritative SHA-256 is the `sha256` field on the `ArtifactRecord`
stored in the artifact store (on-disk manifest for `FsRunStore`, in-memory
entry for `MemRunStore`). The `RunManifest.artifacts` copy is serialization
convenience only and is **never** the integrity source.

### `RunOwnership` guard

Tools that delegate to a structured backend (TestRunner, PythonScriptTool)
MUST set `RunOwnership::DelegatedBackend` and skip their own
`begin_run`/`write_artifact`/`complete_run` to avoid duplicate records.

### Durable writes

`write_file_durable` fsyncs before rename. `sync_parent_dir` best-effort
fsyncs the parent directory after rename. This ensures data reaches stable
storage before the rename is visible.

## Not Yet Integrated

| Gap | Details |
|-----|---------|
| Native git/search tools | No run_store integration |
| Full rerun from manifest | RerunDescriptor defined but re-execution not wired |
| Rollback/revert | No rollback infrastructure (`can_rollback = false`) |
| Artifact viewer | Run detail shows metadata, not full content (`can_view_artifact = false`) |

## Testing

```bash
cargo test -p codegg-core run_store        # 19 unit tests
```

Covers: ID generation, serde roundtrip, begin/write/complete flow,
get/list, ranged reads, integrity violation (mem + fs), artifact too
large, rerun descriptor safety, concurrent writes, path traversal,
list with limit, cleanup plan, FsRunStore atomic begin, artifact
write, index update, and the deadlock regression test
(`fs_store_complete_updates_index_repeated`).

Run with `--test-threads=1` to avoid spurious hangs under concurrent load.

## Related Docs

- [storage.md](storage.md) — Daemon catalog and legacy project store
- [snapshot.md](snapshot.md) — Pre-mutation snapshots
- `architecture/tool_programs.md` — Tool Program lifecycle
- `architecture/scheduler.md` — Scheduler admission and RunStore linkage
