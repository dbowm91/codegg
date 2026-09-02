# Runtime Safety — Checked Edit History Addendum

Status: ready

Repository baseline reviewed: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source subsystem roadmap:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`
- `architecture/snapshot.md`
- `architecture/tool.md`
- `architecture/agent.md`

Applicable ADRs:

- None required for the scoped work. Existing workspace authority and normal mutation permissions remain authoritative.

## 1. Purpose and ownership boundary

CodeGG already captures file snapshots around model-driven file mutation and already has restore APIs. This addendum completes that existing safety mechanism rather than introducing a parallel mutation journal.

The work owns:

- correct workspace/session/turn attribution of native file mutations;
- complete pre/post state capture for the native file-edit tool surface;
- durable bounded edit checkpoints built on the existing snapshot subsystem;
- compare-and-swap style checked restore semantics;
- user-facing Undo/Reapply for supported CodeGG-native file edits.

It does not own terminal/shell side effects, arbitrary plugin or MCP filesystem behavior, Git history rewriting, or worktree orchestration.

## 2. Invariants

- Mutation capture is scoped by explicit workspace/session/turn identity, never by an unscoped process-global event stream.
- A native mutating tool counted as restorable must have complete pre/post state coverage for every affected path.
- Undo verifies that every current path still equals the expected recorded post-state before changing any path.
- Reapply verifies that every current path equals the expected recorded pre-state before changing any path.
- Multi-file restore is all-or-nothing at the logical operation level: any stale/conflicting path prevents the restore from starting.
- Existing path-containment, symlink, atomic-write, and workspace authority checks remain in force.
- Human/external edits after the recorded mutation are never silently overwritten.
- Unsupported mutations are explicitly reported as non-restorable rather than partially included in a turn-level undo.
- The existing snapshot subsystem remains the durable content owner; no second content-history database is introduced without evidence that the snapshot schema cannot support the requirement.

## 3. Explicit non-goals

- undoing arbitrary shell/terminal commands;
- undoing Git commits, branch operations, package-manager side effects, database changes, network changes, or external services;
- intercepting arbitrary plugin/MCP writes in the first pass;
- making binary/non-UTF-8 content restorable if the snapshot subsystem does not safely support it;
- replacing Git as long-term version control;
- adding a general event-sourcing framework;
- adding heavyweight file watchers as the correctness mechanism.

## 4. Current-state evidence

At the reviewed baseline:

- `SnapshotManager` already supports full and incremental capture, durable SQLite storage, listing, fetch, and restore.
- restore already uses bounded path validation and atomic temp-file/rename behavior.
- `AgentLoop` takes a full pre-change snapshot for recognized file-mutating batches and then calls incremental capture from drained `FileChanged` events.
- `write`, `edit`, and `replace` publish `FileChanged` with prior content.
- `multiedit` and `apply_patch` are recognized as file-mutating but do not use the same `FileChanged` publication path for all modes.
- `FileChanged` carries path/action/old content but no session, turn, project, or workspace identity.
- each agent loop subscribes to the global event bus and drains file-change events for incremental snapshot capture, creating a cross-session attribution risk in the singleton multi-project daemon.

The principal gap is therefore capture correctness and checked restore semantics, not absence of snapshot storage.

## 5. Target architecture

The canonical restorable mutation path should be owned around `ToolBatchExecutor`, which already knows the accepted tool calls and the explicit execution identity:

```text
accepted native mutating tool batch
  -> derive bounded affected-path set
  -> capture pre-state from SnapshotManager
  -> execute normal authorized tools
  -> capture post-state for the same paths
  -> persist edit checkpoint with workspace/session/turn/batch identity
```

A file state must distinguish at least:

- `Absent`;
- `Present { hash, content }`.

This is required for create/delete/move semantics.

`FileChanged` may remain a UI/event notification, but durable edit-history correctness must not depend on a receiver consuming unscoped global events.

Checked restore operates as:

```text
Undo(checkpoint):
  validate authority + containment
  verify every current path == checkpoint.post
  if all match, atomically restore checkpoint.pre states

Reapply(checkpoint):
  validate authority + containment
  verify every current path == checkpoint.pre
  if all match, atomically restore checkpoint.post states
```

For a multi-path operation, validation of all paths occurs before the first mutation.

## 6. Dependency graph

```text
M011 mutation attribution + checkpoint correctness
        |
        v
M012 checked Undo/Reapply capability
```

### M011 — Mutation attribution and durable edit checkpoint correctness

Status: ready

Plan:

- `plans/implementation/runtime-safety-resource-footprint/011-mutation-attribution-and-edit-checkpoints.md`

Class: invariant/infrastructure

Exit conditions:

- no durable incremental capture depends on unscoped `FileChanged` draining;
- recognized native mutators have complete affected-path coverage or are explicitly marked non-restorable;
- create/delete/update/move are represented with pre/post state;
- checkpoints carry explicit workspace/session/turn/batch identity;
- concurrent projects cannot cross-contaminate edit checkpoints.

### M012 — Checked Undo/Reapply

Status: blocked on M011

Plan:

- `plans/implementation/runtime-safety-resource-footprint/012-checked-undo-reapply.md`

Class: capability

Exit conditions:

- supported checkpoints can be undone/reapplied from the normal frontend/command surface;
- stale current content blocks the whole operation before mutation;
- restore respects normal workspace permission/path authority;
- unsupported side effects are clearly identified and never falsely reported as reverted.

## 7. Security, concurrency, restart, and compatibility

Checkpoint content remains subject to existing snapshot limits and secret-handling expectations. Do not log captured file bodies.

Concurrent edits must be detected through content hashes immediately before restore. A restore operation should hold the narrowest existing workspace mutation serialization/exclusivity boundary available so validation and application are not separated by an avoidable race.

Daemon restart must preserve persisted checkpoints; no in-memory event receiver is needed to reconstruct history.

Existing snapshot restore APIs may remain for explicit operator restore, but Undo/Reapply must use the stricter checked path. Avoid silently changing broad restore semantics if existing users depend on unconditional explicit restore.

## 8. Verification posture

Use focused snapshot/tool-batch tests, two-workspace concurrency fixtures, create/delete/move/update fixtures, stale-content negative tests, and one repository quick verification pass per coherent milestone. No new CI lane is required.

## 9. Deferred work

- restorable plugin/MCP filesystem operations through a future typed effect/mutation contract;
- terminal/shell side-effect rollback;
- binary file history;
- Git-aware semantic conflict resolution beyond exact hash/state checks;
- cross-daemon/distributed edit-history replication.
