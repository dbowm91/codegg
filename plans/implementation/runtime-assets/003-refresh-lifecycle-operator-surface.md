# Runtime Assets Milestone 003 — Refresh Lifecycle and Operator Surface

Status: implemented — see `plans/closure/runtime-assets/003-status.md`

Repository baseline: `5974976` (`feat(project-catalog): add bounded discovery reconciliation`), with Runtime Assets Milestones 001–002 closed.

Implementation commit: `972c286`

Source roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-3--refresh-lifecycle-and-operator-surface`

Related closure evidence:

- `plans/closure/runtime-assets/002-status.md`
- `plans/closure/project-catalog/002-status.md`

Primary class: capability

## 1. Objective

Add a daemon-owned, project/workspace-scoped runtime-asset refresh lifecycle and operator surface. Refresh must build a candidate `ProjectAssetSnapshot` outside the publication lock, validate it, and atomically publish a new generation only when valid. Session/project lifecycle triggers and manual refresh must converge through the same coordinator.

## 2. Dependencies and boundaries

Hard dependencies are Runtime Assets Milestones 001–002, both closed. The interface dependency on the closed snapshot builder is satisfied by `AssetContext`, `ProjectAssetSnapshot`, `ProjectAssetSnapshotBuilder`, and the context-aware agent/skill/instruction constructors documented in `plans/closure/runtime-assets/002-status.md`.

The plan may consume the core-neutral discovery coordinator from Project Catalog M2 for selected project/workspace context, but it MUST NOT implement catalog protocol migration, project activation leases, remote execution, or a file watcher as correctness infrastructure.

## 3. Scope

### In scope

- A per-project/workspace refresh coordinator with generation numbers, last-valid snapshot retention, and single-flight/coalesced requests.
- Refresh triggers on project activation and session create/open/attach/rebind seams, before the next turn runtime is constructed.
- Manual refresh requests and focused `/reload` aliases through the existing native command/protocol seams.
- Bounded refresh reports for added, removed, changed, shadowed, invalid, retained, and diagnostic asset entries.
- Cancellation and failure behavior that preserves the previous valid snapshot.
- Restart reconstruction from explicit context and the latest durable generation/fingerprint metadata.
- Protocol DTOs/events and capability advertisement sized for summaries; asset bodies/resources remain behind bounded local handles.
- Architecture, operational, and regression documentation.

### Explicitly out of scope

- Mutating an in-flight turn or changing an already captured snapshot.
- Executing skill scripts or foreign harness metadata.
- Real-time watchers, distributed manifests, remote transport, or synchronization writes.
- Multi-project TUI tab authority or Project Catalog Milestone 4 server migration.
- Unbounded eager reads of skill resources.

## 4. Required production changes

### Refresh domain and coordinator

Introduce a project/workspace-scoped refresh service around the existing snapshot builder. It must:

1. capture an immutable explicit `AssetContext` at request start;
2. coalesce same-scope requests and admit global work through the daemon scheduler/resource boundary where required;
3. build and validate outside the publication lock;
4. assign a monotonic generation only at successful publication;
5. retain the last valid generation when cancellation, invalid input, or filesystem failure occurs;
6. return bounded reports without including asset bodies or secret-bearing paths;
7. make restart reconstruction deterministic from context, config revision, source inventory, and stored fingerprint metadata.

The publication state must be `Arc`-based so active turns retain their snapshot without observing later swaps.

### Lifecycle integration

Wire refresh into the narrowest existing project/session lifecycle seams. The trigger must complete or fail actionably before a new turn runtime captures assets. Repeated lifecycle signals should coalesce and must not create duplicate generations. Existing deprecated cwd-based constructors remain compatibility boundaries only; daemon paths use explicit context.

### Protocol and operator surface

Add bounded request/response/event types for refresh, status, generation, diagnostics, and capability support. Manual refresh must distinguish successful publication, retained prior generation, cancellation, and invalid refresh. `/reload` may be an alias, but it must route to the same service and not become a second refresh implementation.

### Security and resource limits

Reuse the asset registry and instruction resolver bounds. Validate project/workspace roots before refresh, preserve foreign directories as read-only, reject traversal/symlink escapes, avoid logging contents, and ensure refresh never invokes tools, providers, LSP, builds, or bundled scripts.

## 5. Storage, migration, and compatibility

Prefer additive metadata for generation, fingerprint, last-success timestamp, and bounded diagnostics. If SQLite changes are needed, use an idempotent migration with explicit limits and retention. Snapshot bodies remain reconstructible; do not persist unbounded Markdown or skill contents.

Protocol additions must be capability-gated and preserve existing clients. Large reports require pagination or bounded summaries. Legacy refresh-free clients continue to use the last published snapshot or receive an actionable capability response.

## 6. Verification plan

Required focused evidence:

- refresh generation increments only after valid publication;
- invalid/cancelled/failed refresh retains the last valid snapshot;
- concurrent same-scope requests coalesce to one publication;
- two projects remain isolated under simultaneous refresh;
- project/session lifecycle triggers refresh before a new turn runtime;
- manual refresh and `/reload` return the same report semantics;
- restart reconstructs the same effective fingerprint;
- protocol summaries are bounded and omit asset bodies/secrets;
- no refresh path executes a script or starts an external subsystem;
- resource, symlink, malformed-frontmatter, and path-traversal adversarial fixtures pass.

Run at minimum:

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test --test asset_snapshot
rtk cargo test -p codegg --lib agent::
rtk cargo test -p codegg --lib tui::commands::agents
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk bash scripts/check-core-boundary.sh
rtk git diff --check
```

Use the repository's capped workspace command before closure and report unrelated socket/provider failures separately.

## 7. Acceptance and closure

The milestone is ready to close when lifecycle-triggered and manual refresh both publish validated immutable generations, preserve the last valid generation on failure/cancellation, coalesce same-scope work, expose bounded operator diagnostics, and demonstrate in-flight snapshot pinning. The closure record must include exact commits, requirement evidence, migration/compatibility results, security/contention/restart tests, static-guard output, docs, known limitations, and the follow-up disposition for Runtime Assets Milestone 004.
