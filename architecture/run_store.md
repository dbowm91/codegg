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
| Trait + types + impls | `crates/codegg-core/src/run_store.rs` (2714 lines) |
| Error variants | `crates/codegg-core/src/error.rs:447-468` (`RunStoreError`, 7 variants) |
| Root re-export | `src/lib.rs:22` — `run_store` in the `pub use codegg_core::{...}` re-export list |
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
| `RunId` | :26 | UUID v4 newtype, `Display`, `Default` |
| `ArtifactId` | :57 | Same pattern |

### Enums

| Type | File:Line | Variants |
|------|-----------|----------|
| `RunKind` | :212 | `RawShell`, `ManagedProcess`, `Test`, `GitRead`, `GitMutation`, `Search`, `Python`, `NativeTool` (8) |
| `RunStatus` | :242 | `Running`, `Complete`, `Failed`, `TimedOut`, `Cancelled`, `Incomplete` (6) |
| `ArtifactKind` | :268 | `Stdout`, `Stderr`, `CombinedLog`, `CommandSource`, `TestReport`, `TestLog`, `UnifiedDiff`, `ChangedFiles`, `Projection`, `RtkProjection`, `StructuredJson`, `PolicyEvidence` (12) |
| `ContextPromotionState` | :785 | `LocalOnly`, `ProjectionIncluded`, `ArtifactRangeIncluded`, `Pinned`, `Excluded` (5) |
| `RunOwnership` | :96 | `Caller`, `DelegatedBackend`, `ChildOf(RunId)` (3) |
| `PlannedBackend` | :114 | `Unrouted`, `RawShell`, `TestRunner`, `PythonScript`, `NativeTool`, `ManagedArgv`, `Git`, `GitMutating` (8, last deprecated) |
| `ActualBackend` | :157 | `RawShell`, `TestRunner`, `PythonScript`, `NativeTool`, `ManagedArgv`, `Git`, `GitMutating`, `Eggwork`, `Rejected { reason }` (9) |

### Record Types

| Type | File:Line | Purpose |
|------|-----------|---------|
| `RunInvocation` | :286 | command, argv, script_hash |
| `BackendRecord` | :297 | family, detail |
| `RiskRecord` | :306 | level, has_subprocess, has_git_mutation, has_destructive_mutation |
| `PermissionDecisionRecord` | :316 | tool, path, decision |
| `SandboxRecord` | :326 | os_isolation, network_isolation, read_roots, write_roots |
| `ArtifactRecord` | :410 | artifact_id, kind, relative_path, mime_type, byte_length, sha256, truncated, redacted, created_at, safe_for_model |
| `ProjectionRecord` | :426 | projector, exactness, omitted_ranges, projection_id, source_spans, redaction_records, rtk_metadata, estimated_output_tokens, promotion_decision, input_digests |
| `ChangedPathRecord` | :496 | path, kind |
| `RerunDescriptor` | :504 | argv (AuditSafeArgv), script_source_ref, backend_family, cwd, workspace_root, mode, config_profile, parent_run_id |
| `FallbackRecord` | :199 | planned, actual, reason |
| `RunAssetProvenance` | :591 | generation, fingerprint, activated_skill_digests |

### Composite Types

| Type | File:Line | Purpose |
|------|-----------|---------|
| `RunManifest` | :532 | Full run descriptor, 26 fields: `schema_version`, `run_id`, `session_id`, `parent_run_id`, `kind`, `invocation`, `started_at`, `completed_at`, `status`, `workspace_root`, `cwd`, `backend`, `risk`, `permissions`, `sandbox`, `artifacts`, `projection`, `changes`, `rerun`, `planned_backend`, `actual_backend`, `fallback`, `remote_resources`, `ownership`, `asset_provenance`, `source_subject` |
| `RunSummary` | :626 | Lightweight listing for `list_runs` |
| `RunDraft` | :639 | Input for `begin_run` |
| `RunHandle` | :660 | Returned by `begin_run` (run_id, run_dir, started_at) |
| `RunCompletion` | :667 | Input for `complete_run` |
| `RunQuery` | :694 | Filter for `list_runs` |
| `ArtifactInput` | :706 | Input for `write_artifact` |
| `ArtifactRef` | :714 | Returned by `write_artifact` |
| `ArtifactChunk` | :722 | Returned by `read_artifact` (supports ranged reads) |
| `ByteRange` | :730 | start, end |
| `RetentionConfig` | :738 | max_total_bytes, max_run_count, max_age_days, preserve_failed_longer, failed_extra_days |
| `CleanupPlan` | :759 | runs_to_delete, bytes_to_free, pinned_runs_skipped |
| `IndexEntry` | :768 | JSONL index record |

### View Models

| Type | File:Line | Purpose |
|------|-----------|---------|
| `RunCellView` | :808 | Compact TUI cell; `from_manifest()` computes capability flags |
| `RunDetailView` | :967 | Full detail overlay; `from_manifest()` |
| `RunInvocationView` | :980 | Command, argv, cwd, backend |
| `RunPermissionView` | :991 | Tool, path, decision |
| `RunPolicyView` | :998 | Risk + sandbox |
| `RunArtifactView` | :1018 | Metadata only (no raw bytes) |
| `RunProjectionView` | :1030 | projector, exactness, omitted_ranges |
| `RunChangeView` | :1037 | path, kind |

### Trait

```rust
// run_store.rs:1138
#[async_trait]
pub trait RunStore: Send + Sync {
    async fn begin_run(&self, draft: RunDraft) -> Result<RunHandle, RunStoreError>;
    async fn write_artifact(&self, run: &RunHandle, artifact: ArtifactInput)
        -> Result<ArtifactRef, RunStoreError>;
    async fn complete_run(&self, run: RunHandle, completion: RunCompletion)
        -> Result<RunManifest, RunStoreError>;
    async fn get_run(&self, id: &RunId) -> Result<Option<RunManifest>, RunStoreError>;
    /// Project the scheduler-sealed JobAttempt subject after executor cleanup.
    /// Scheduler-owned RunStore records are projections; this API never captures source state.
    async fn set_run_source_subject(
        &self,
        id: &RunId,
        subject: &crate::jobs::ExecutionSubjectProvenance,
    ) -> Result<(), RunStoreError>;
    async fn read_artifact(&self, id: &ArtifactId, range: Option<ByteRange>)
        -> Result<ArtifactChunk, RunStoreError>;
    async fn list_runs(&self, query: RunQuery) -> Result<Vec<RunSummary>, RunStoreError>;
}
```

`set_run_source_subject` has a **default no-op body** in the trait, so an
implementation that does not project the subject still compiles. Both shipped
implementations override it: `FsRunStore` (`:1740`) and `MemRunStore` (`:2100`)
refuse an unsealed subject (`RunStoreError::IntegrityViolation`) and refuse a
revision change against a subject already captured in the manifest. The
RunStore never captures source state itself (see "Execution subject
projection" below); the only production caller is the scheduler
(`src/scheduler/scheduler.rs:1598`).

### Implementations

| Impl | File:Line | Backend |
|------|-----------|---------|
| `FsRunStore` | :1229 | Filesystem with JSONL index, `tokio::sync::Mutex<()>` serialization |
| `MemRunStore` | :1900 | In-memory `parking_lot::RwLock<HashMap>` |

### Constants

| Constant | Value | File:Line |
|----------|-------|-----------|
| `SCHEMA_VERSION` | `1` | :14 |
| `MAX_ARTIFACT_BYTES` | 64 MiB | :17 |
| `DEFAULT_MAX_TOTAL_BYTES` | 1 GiB | :18 |
| `DEFAULT_MAX_RUN_COUNT` | 1000 | :19 |
| `DEFAULT_MAX_AGE_DAYS` | 30 | :20 |
| `DEFAULT_FAILED_EXTRA_DAYS` | 30 | :21 |

## Integration Points

### Tool integration

| Location | How Used |
|----------|----------|
| `src/tool/mod.rs:437` | `ToolRegistryOptions.run_store: Option<Arc<dyn RunStore>>` (struct at `:376`) |
| `src/tool/factory.rs:162` | Creates `FsRunStore` at `execution.workspace_root/.codegg/runs`, passes to tools |
| `src/tool/bash.rs:1029` (`persist_caller_run` impl `src/tool/bash/output.rs:149`) | Persists runs with the correct `RunKind` derived from the command intent; delegation is decided by `persistence_decision` |
| `src/python_script/tool.rs:60-216` (`build_python_run_draft`, `begin_python_run`, artifact/complete helpers) | Persists `RunKind::Python` runs with diff/sandbox/changes |
| `src/test_runner/runner.rs` | Begins the `Test` RunStore record before process launch and completes it after supervision |

### TUI integration

| Location | How Used |
|----------|----------|
| `src/tui/app/mod.rs:360` | `App.run_store: Option<Arc<dyn RunStore>>` |
| `src/tui/app/mod.rs:925-930` | Initializes `FsRunStore` at `<project_dir>/.codegg/runs` |
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
| Native search tools | No run_store integration. The native `git` tool **does** persist: `src/git_run_store.rs` writes `RunKind::GitMutation` / `RunKind::GitRead` records through `persist_mutation` / `persist_recovery`. |
| Git/Python/shell/worktree rerun | Intentionally unsupported until each class has an explicit reconstruction and credential contract |
| Rollback/revert | No rollback infrastructure (`can_rollback = false`) |
| Artifact viewer | Run detail shows metadata, not full content (`can_view_artifact = false`) |

## Testing

```bash
cargo test -p codegg-core run_store        # 18 unit tests
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

## Source verification

### Pass of 2026-10-23

Re-verified against `crates/codegg-core/src/run_store.rs` (2714 lines),
`crates/codegg-core/src/error.rs`, `src/lib.rs`, `src/tool/{mod,factory,bash}.rs`,
`src/tool/bash/output.rs`, `src/python_script/tool.rs`, `src/git_run_store.rs`,
`src/test_runner/runner.rs`, and `src/tui/app/mod.rs`. No build or test run
was performed; every count is a source-level measurement.

- **Corrected a wrong count.** `ActualBackend` has **9** variants, not 10 —
  `Unrouted` was removed. It is still *not* "`PlannedBackend` + `Rejected`":
  `ActualBackend` adds `Eggwork` and `Rejected { reason }` to the planned set.
  `PlannedBackend` still has 8 (including the deprecated `GitMutating`).
- **Corrected a wrong field count.** `RunManifest` has **26** fields, not 25:
  the list omitted `remote_resources` (between `fallback` and `ownership`).
- **Corrected a stale gap claim.** "Native git/search tools — no run_store
  integration" was no longer true for the native `git` tool:
  `src/git_run_store.rs` persists `RunKind::GitMutation` / `RunKind::GitRead`
  records via `persist_mutation` (`:40`) and `persist_recovery` (`:248`). The
  row now says search tools have no run_store integration.
- **Corrected a stale test count.** `run_store.rs` holds **18** `#[test]` /
  `#[tokio::test]` functions, not 19.
- **Re-anchored every `File:Line` ref in the type, enum, record, composite,
  view-model, impl and constant tables** to the current file (the file grew
  from 2612 to 2714 lines; the drift is not uniform — it runs from +1 near the
  top to +84 near the bottom, because `ContextPromotionState`,
  `ArtifactRecord`, `ProjectionRecord` and the view models were extended
  in place). The `RunStore` trait anchor moved `:1054` → `:1138`,
  `FsRunStore` `:1145` → `:1229`, `MemRunStore` `:1809` → `:1900`.
- **Completed the trait snippet.** The 2026-10-06 snippet omitted
  `set_run_source_subject`, the trait's seventh method (`:1153`), which
  projects the scheduler-sealed `ExecutionSubjectProvenance`. It is documented
  now, with its default no-op body and both overriding implementations.
- **Re-anchored the integration-point refs**: `ToolRegistryOptions.run_store`
  `tool/mod.rs:337` → `:437` (struct `:276` → `:376`), `FsRunStore`
  construction `tool/factory.rs:139-143` → `:162`, bash persistence
  `bash.rs:651-664` → `:1029` (impl still `bash/output.rs:149`), python
  `tool.rs:57-227` → `:60-216`, `App.run_store` `tui/app/mod.rs:246` → `:360`
  and init `:533-539` → `:925-930`.
- **Re-anchored the surrounding refs**: `RunStoreError` `error.rs:410-431` →
  `:447-468`, root re-export `src/lib.rs:12` → `:22`.
- **Corrected a heading level.** "Execution subject projection" was an `#`
  (h1) heading inside the body; it is now `##`, matching the rest of the
  document.
- **Confirmed correct as written**: `RunKind` 8, `RunStatus` 6,
  `ArtifactKind` 12, `ContextPromotionState` 5, `PlannedBackend` 8,
  `RunOwnership` 3, `RunStoreError` 7, the field lists of every record and
  view-model type, the six retention constants, the `CoreEvent` run variants
  in `crates/codegg-protocol/src/core.rs` (`RunStarted`, `RunProgress`,
  `RunArtifactCreated`, `RunProjectionReady`, `RunCompleted`, `RunDenied`,
  `RunPinned`, `ContextPromotionChanged`, `RunRerunLinked`), the
  `RunDetailDialog` 7-tab view, and the storage root
  `<workspace>/.codegg/runs/`.

### Pass of 2026-10-06

Verified against `crates/codegg-core/src/run_store.rs` (2612
lines), `crates/codegg-core/src/error.rs`, `src/lib.rs`,
`src/tool/{mod,factory,bash}.rs`, `src/tool/bash/output.rs`,
`src/python_script/tool.rs`, and `src/tui/app/mod.rs`. Confirmed and
extended the prior review's finding that the view-model refs were
systematically stale: they were off by 15-22 lines, and the *entire*
type table was stale too. Corrected all 34 type/enum/impl/constant refs
(`RunCellView` :718→:724 through `RunChangeView` :930→:953, `RunStore` trait
:1028→:1054, `FsRunStore` :1110→:1145, `MemRunStore` :1730→:1809,
`RunOwnership` :94→:95, and all 20 record/composite types). Corrected
`RunStoreError` ref `error.rs:390-411` → `:410-431` and recorded its 7
variants; root re-export `src/lib.rs:11` → `:12`. Corrected
`ActualBackend`: it is NOT "PlannedBackend + `Rejected`" — it has 10
variants, adding `Eggwork`; listed all 10. Replaced `RunManifest`'s
"~22 fields" with the verified 25. Corrected the integration-point refs:
`ToolRegistryOptions.run_store` `tool/mod.rs:242`→`:330`,
`FsRunStore` construction `tool/factory.rs:45-52`→`:139-143`,
`App.run_store` `tui/app/mod.rs:681`→`:246` and init `:872-877`→`:533-539`,
bash persistence `:664-760`→`:651-664` (impl is in `bash/output.rs:149`),
python `:143-257`→`:57-227`. Verified accurate: `RunKind` 8, `RunStatus` 6,
`ArtifactKind` 12, `ContextPromotionState` 5, `PlannedBackend` 8,
`RunOwnership` 3, and all six retention constants.

The 2026-10-06 `ActualBackend` (10) and `RunManifest` (25) figures were wrong
and are corrected in the 2026-10-23 pass above; every line ref in that list has
since moved again.
## Execution subject projection

Run manifests may carry the optional attempt-scoped `source_subject`
provenance envelope. Scheduler-owned producers copy it from JobAttempt; the
RunStore never captures or reconstructs source state itself. When both the
draft and completion carry a subject, completion must preserve the captured
revision. Historical manifests without the field remain valid and unavailable
for exact-subject evidence.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"), which rewrote `src/tool/mod.rs`.
Corrected: `ToolRegistryOptions.run_store` `330`->`337` and the struct anchor
`:272`->`:276`, both re-checked against the new file. The optional
`RunStore` shape and its default-`None` behaviour are unchanged.

Re-verified 2026-10-23: the subject is projected through the trait's
`set_run_source_subject` method (default no-op; overridden by `FsRunStore`
`:1740` and `MemRunStore` `:2100`, which refuse an unsealed subject and a
revision change). The only production caller is the scheduler
(`src/scheduler/scheduler.rs:1598`). `ToolRegistryOptions.run_store` is now
`src/tool/mod.rs:437` (struct `:376`). The optional `RunStore` shape and its
default-`None` behaviour are unchanged.
