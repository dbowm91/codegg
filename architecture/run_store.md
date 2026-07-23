# Run Store

## Overview

**Location**: `crates/codegg-core/src/run_store.rs` (~1900 lines)

The run store provides durable, filesystem-backed persistence for structured command execution records (runs) and their associated artifacts. It replaces ad-hoc log files and JSONL indices with a typed, indexed store that supports ranging, retention, and cleanup.

## Key Responsibilities

- Persist run manifests (metadata, invocation, risk, permissions, sandbox, projection, changes, rerun descriptor)
- Persist artifacts (stdout, stderr, diffs, test reports, structured JSON, etc.)
- Provide JSONL index for fast listing and querying
- Enforce retention limits (bytes, count, age)
- Support ranged artifact reads for large outputs
- Provide in-memory implementation for tests

## Architecture

### RunStore Trait

```rust
#[async_trait]
pub trait RunStore: Send + Sync {
    async fn begin_run(&self, draft: RunDraft) -> Result<RunHandle, RunStoreError>;
    async fn write_artifact(&self, run: &RunHandle, artifact: ArtifactInput) -> Result<ArtifactRef, RunStoreError>;
    async fn complete_run(&self, run: RunHandle, completion: RunCompletion) -> Result<RunManifest, RunStoreError>;
    async fn get_run(&self, id: &RunId) -> Result<Option<RunManifest>, RunStoreError>;
    async fn read_artifact(&self, id: &ArtifactId, range: Option<ByteRange>) -> Result<ArtifactChunk, RunStoreError>;
    async fn list_runs(&self, query: RunQuery) -> Result<Vec<RunSummary>, RunStoreError>;
}
```

### Implementations

| Implementation | Location | Purpose |
|---------------|----------|---------|
| `FsRunStore` | `:478` | Filesystem-backed with JSONL index, atomic writes, path traversal protection |
| `MemRunStore` | `:968` | In-memory for tests, `parking_lot::RwLock` based |

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

### Key Constants

| Constant | Value |
|----------|-------|
| `SCHEMA_VERSION` | `1` |
| `MAX_ARTIFACT_BYTES` | 64 MiB |
| `DEFAULT_MAX_TOTAL_BYTES` | 1 GiB |
| `DEFAULT_MAX_RUN_COUNT` | 1000 |
| `DEFAULT_MAX_AGE_DAYS` | 30 |
| `DEFAULT_FAILED_EXTRA_DAYS` | 30 |

## Domain Types

### Identifiers

- `RunId(String)` — UUID v4, Display, Default, Serialize/Deserialize
- `ArtifactId(String)` — Same pattern as RunId

### Enums

- **`RunKind`** (8 variants): `RawShell`, `ManagedProcess`, `Test`, `GitRead`, `GitMutation`, `Search`, `Python`, `NativeTool`
- **`RunStatus`** (6 variants): `Running`, `Complete`, `Failed`, `TimedOut`, `Cancelled`, `Incomplete`
- **`ArtifactKind`** (12 variants): `Stdout`, `Stderr`, `CombinedLog`, `CommandSource`, `TestReport`, `TestLog`, `UnifiedDiff`, `ChangedFiles`, `Projection`, `RtkProjection`, `StructuredJson`, `PolicyEvidence`
- **`ContextPromotionState`** (5 variants): `LocalOnly`, `ProjectionIncluded`, `ArtifactRangeIncluded { artifact_id, start, end }`, `Pinned`, `Excluded`

### Record Types

- `RunInvocation` — command, argv, script_hash
- `BackendRecord` — family, detail
- `RiskRecord` — level, has_subprocess, has_git_mutation, has_destructive_mutation
- `PermissionDecisionRecord` — tool, path, decision
- `SandboxRecord` — os_isolation, network_isolation, read_roots, write_roots
- `ArtifactRecord` — artifact_id, kind, relative_path, mime_type, byte_length, sha256, truncated, redacted, safe_for_model
- `ProjectionRecord` — projector, exactness, omitted_ranges
- `ChangedPathRecord` — path, kind
- `RerunDescriptor` — argv, script_source_ref, backend_family, cwd, workspace_root, mode, config_profile, parent_run_id

### Composite Types

- **`RunManifest`** (:264) — Full run descriptor with all 15 fields
- **`RunSummary`** (:298) — Lightweight listing (run_id, kind, status, started_at, completed_at, command)
- **`RunDraft`** (:311) — Input for `begin_run`
- **`RunHandle`** (:323) — Returned by `begin_run` (run_id, run_dir, started_at)
- **`RunCompletion`** (:330) — Input for `complete_run`
- **`RunQuery`** (:343) — Filter for `list_runs` (kind, status, session_id, since, until, limit)
- **`ArtifactInput`** (:355) — Input for `write_artifact`
- **`ArtifactRef`** (:363) — Returned by `write_artifact`
- **`ArtifactChunk`** (:371) — Returned by `read_artifact` (supports ranged reads)
- **`ByteRange`** (:379) — start, end
- **`RetentionConfig`** (:387) — max_total_bytes, max_run_count, max_age_days, preserve_failed_longer, failed_extra_days
- **`CleanupPlan`** (:408) — runs_to_delete, bytes_to_free, pinned_runs_skipped
- **`IndexEntry`** (:417) — JSONL index record (10 fields including pinned, date_dir)
- **`RunOwnership`** (:75) — Enum (`Caller`, `DelegatedBackend`, `ChildOf(RunId)`) describing who owns the canonical run record for a command execution.

### View Models (Phase 08)

- **`RunCellView`** (:462) — Compact summary for TUI cells (from_manifest()). Capability flags: `can_rollback` (disabled; no rollback backend exists), `can_rerun` (requires `rerun` descriptor with complete argv), `can_promote` (requires artifacts, projection, completed/failed status, and at least one safe_for_model artifact), `can_view_artifact` (disabled; no ranged reader available yet).
- **`RunDetailView`** (:530) — Full detail for overlay (from_manifest())
- Sub-views: `RunInvocationView`, `RunPermissionView`, `RunPolicyView`, `RunArtifactView`, `RunProjectionView`, `RunChangeView`

## Integration Points

### Tool Integration

| Location | How Used |
|----------|----------|
| `src/tool/mod.rs:242` | `ToolRegistryOptions.run_store: Option<Arc<dyn RunStore>>` |
| `src/tool/mod.rs:263-266` | `BashTool` receives `run_store` via `with_run_store()` |
| `src/tool/mod.rs:341-342` | `PythonScriptTool` receives `run_store` via `with_run_store()` |
| `src/tool/mod.rs:340-341` | `TestTool` receives `run_store` via `with_run_store()` |
| `src/tool/factory.rs:45-52` | Factory creates `FsRunStore` at `.codegg/runs/` and passes to tools |
| `src/tool/bash.rs:664-760` | BashTool persists runs with the correct `RunKind` based on routing decision (GitRead, NativeTool, Search, GitMutation, ManagedProcess, Test, Python, RawShell). Skips persistence for TestRunner and PythonScript backends that own their own records. |
| `src/python_script/tool.rs:143-257` | PythonScriptTool persists `Python` runs with diff/sandbox/changes |
| `src/test_runner/runner.rs:238-239` | TestRunner persists `Test` runs via `persist_to_run_store()` after each test run |

### TUI Integration

| Location | How Used |
|----------|----------|
| `src/tui/app/mod.rs:681` | `App.run_store: Option<Arc<dyn RunStore>>` |
| `src/tui/app/mod.rs:872-877` | App initializes `FsRunStore` at `.codegg/runs/` |
| `src/tui/app/mod.rs:3510` | `TuiMsg::OpenRunDetail` loads manifest, creates `RunDetailDialog` |
| `src/tui/components/dialogs/run_detail.rs` | `RunDetailDialog` — 7-tab detail view |
| `src/tui/components/messages.rs` | `MsgPart::RunCell` — compact run cell rendering |

### Protocol Events (Phase 08)

Added to `CoreEvent` in `crates/codegg-protocol/src/core.rs`:

- `RunStarted`, `RunProgress`, `RunArtifactCreated`, `RunProjectionReady`
- `RunCompleted`, `RunDenied`, `RunPinned`, `ContextPromotionChanged`, `RunRerunLinked`

### Protocol Conversions

In `src/protocol_conversions.rs`:

- `run_started_event()`, `run_progress_event()`, `run_completed_event()`
- `run_artifact_created_event()`, `run_denied_event()`

## Not Yet Integrated

| Gap | Details |
|-----|---------|
| Native git/search tools | No run_store integration |
| Full rerun from manifest | RerunDescriptor defined but re-execution not wired |
| Rollback/revert | No rollback infrastructure |
| Artifact viewer | Run detail shows artifact metadata, not full content |

## Python Scheduler Ownership (M001)

All production model-facing Python execution is now scheduler-owned. The `PythonJobExecutor` begins a `RunKind::Python` record **before** process launch, writes artifacts (stdout, stderr, unified diff) after execution, and completes the run with terminal status and sandbox evidence.

### Lifecycle

1. **`begin_python_run`** — Creates a `RunDraft` and calls `store.begin_run()`. The run is visible as "active" immediately. Called by `PythonJobExecutor` before subprocess launch.
2. **`write_python_run_artifacts`** — Writes `ArtifactKind::Stdout`, `ArtifactKind::Stderr`, and `ArtifactKind::UnifiedDiff` to the active run handle.
3. **`complete_python_run`** — Calls `store.complete_run()` with terminal `RunStatus`, `SandboxRecord`, and `ChangedPathRecord`s.

### Integration points

| Location | How Used |
|----------|----------|
| `src/python_script/tool.rs` | `begin_python_run`, `write_python_run_artifacts`, `complete_python_run` — split lifecycle for executor |
| `src/python_script/tool.rs` | `persist_python_run` — legacy combined helper (delegates to split functions) |
| `src/scheduler/executors.rs` | `PythonJobExecutor::execute` — calls begin before execution, write+complete after |
| `src/python_script/source_store.rs` | Content-addressed source persistence; orphan cleanup via scheduler reconcile |

### RunStore artifact expansion

Python run artifacts use real `ArtifactRef` handles (`run://{run_id}/stdout`, `run://{run_id}/stderr`), not pseudo-labels. The `project_python_run` function references these handles for model-facing projection.

## Tests

13 unit tests in `run_store.rs` covering: ID generation, serde roundtrip, begin/write/complete flow, get/list, ranged reads, integrity violation, artifact too large, rerun descriptor safety, concurrent writes, path traversal, list with limit, cleanup plan, FsRunStore (atomic begin, artifact write, index update).

Run with: `cargo test -p codegg-core run_store`. With repeated-run regression coverage (`fs_store_complete_updates_index_repeated`), the full RunStore suite is 19 tests. Always run with `--test-threads=1` (resource-capped) to avoid spurious hangs under concurrent load.

## Invariants

### Authoritative checksum source

The SHA-256 that `read_artifact` validates against is the `sha256` field
of the **`ArtifactRecord`** stored in the *artifact store*:

- **`MemRunStore`**: `artifacts: parking_lot::RwLock<HashMap<ArtifactId, MemArtifactEntry>>` where `MemArtifactEntry = (RunId, Vec<u8>, ArtifactRecord)`. `read_artifact` reads the bytes, recomputes SHA-256, and compares against `record.sha256`.
- **`FsRunStore`**: `ArtifactRecord.relative_path` points at a file under `<date>/<run-id>/`. `read_artifact` reads the file, recomputes SHA-256, and compares against `ArtifactRecord.sha256` from the persisted `manifest.json`.

The manifest also carries a copy of each `ArtifactRecord` (in `RunManifest.artifacts`) for serialization convenience, but the manifest copy is **never** the integrity source. Tests that want to verify integrity MUST mutate either the bytes on disk (for `FsRunStore`) or the `MemArtifactEntry` record (for `MemRunStore`).

### `tokio::sync::Mutex` reentrancy rule

`FsRunStore.lock: tokio::sync::Mutex<()>` is **not reentrant**. The
single allowed lock-holding pattern is: acquire the lock once, then
call `rewrite_index_locked` (the `_locked` suffix signals "caller must
hold `self.lock` for the duration"). Never wrap the locked variant in
code that calls `self.lock.lock().await` again; doing so will deadlock
the current task permanently. The history commit
`ba66c7d4a4f448abcadc789c8790ec3ecad54e94` documents the
`fs_store_complete_updates_index` hang as the originating defect.
