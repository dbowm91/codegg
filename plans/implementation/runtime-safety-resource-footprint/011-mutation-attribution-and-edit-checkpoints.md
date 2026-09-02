# Runtime Safety Milestone 011 — Mutation Attribution and Durable Edit Checkpoints

Status: closed (closure: `plans/closure/runtime-safety-resource-footprint/011-status.md`)

Repository baseline: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source roadmap:

- `plans/subsystems/runtime-safety-edit-history-addendum.md#6-dependency-graph`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None. Preserve explicit workspace authority and existing snapshot ownership.

Primary class: invariant

## 1. Objective

Make CodeGG's existing snapshot/edit-history capture correctly attributable to the exact workspace/session/turn/tool batch and complete for the supported native file-mutating tool surface, producing durable pre/post edit checkpoints that can safely support later checked Undo/Reapply.

## 2. Why this milestone is ready

- `SnapshotManager` already owns durable snapshot content and restore safety.
- `ToolBatchExecutor` already identifies native file-modifying tools and has exact session/turn/workspace execution context.
- file tools already expose structured arguments sufficient to derive affected paths for the normal native mutation surface.
- prior runtime-correctness work established explicit workspace-bound agent construction.

No new mutation runtime or event-sourcing subsystem is required.

## 3. Current implementation evidence

At baseline:

- `AgentLoop::file_change_rx` subscribes to `GlobalEventBus`.
- `drain_file_change_events()` accepts any received `AppEvent::FileChanged` and returns `(path, old_content)` without session/workspace filtering because the event has no such identity.
- `ToolBatchExecutor` clears the receiver, captures a full snapshot before recognized mutating batches, executes tools, then calls incremental snapshot capture based on drained file-change events.
- `write`, `edit`, and `replace` emit prior-content `FileChanged` events.
- `multiedit` directly mutates a file without the same prior-content event path.
- `apply_patch` directly handles update/create/delete/move and likewise lacks uniform event-based pre-state coverage.
- `SnapshotManager::capture_incremental()` already stores bounded old content and validates safe relative paths.

This means the current event path is useful for UI diff notification but is not a sufficient durable provenance boundary for a concurrent singleton daemon.

## 4. Invariants that must not regress

- All durable checkpoint paths are resolved relative to one explicit execution workspace.
- No checkpoint may include a mutation from another session/workspace/turn.
- Pre-state is captured before the first relevant mutation in the batch.
- Post-state is captured from the same bounded affected path set after tool execution.
- Create/delete/move state is representable without inventing empty-file equivalence for absence.
- Existing snapshot size/path/symlink limits remain enforced.
- A tool not covered by the checkpoint contract is marked non-restorable; it is never implicitly treated as safely captured.
- `FileChanged` remains observational and cannot be the sole durable source of mutation attribution.
- Existing tool permissions and normal execution behavior do not change merely to support history.

## 5. Scope

### In scope

- define a bounded affected-path extraction contract for supported native file mutators;
- define/persist an edit checkpoint identity with workspace/session/turn/batch provenance;
- represent file states as present/absent with hash and content where supported;
- capture pre-state before execution and post-state after execution from `ToolBatchExecutor` or an equivalent canonical mutation boundary;
- cover `write`, `edit`, `replace`, `multiedit`, and `apply_patch` update/create/delete/move;
- remove durable incremental-history dependence on unscoped global `FileChanged` draining;
- retain `FileChanged` as a UI/event signal where useful;
- add two-workspace and concurrent-batch regression tests;
- document which native mutations are restorable and which are not.

### Explicitly out of scope

- Undo/Reapply command or TUI implementation (M012);
- terminal/bash arbitrary side-effect capture;
- plugin/MCP arbitrary filesystem writes;
- Git mutation rollback;
- binary content support beyond existing safe snapshot behavior;
- changing unrelated global event-bus consumers.

## 6. Required production changes

### Core/domain

Extend the existing snapshot domain with the minimum typed structures required for edit checkpoints rather than creating a second history subsystem. A representative model is:

```text
EditCheckpoint
  id
  workspace_id
  session_id
  turn_id
  batch_sequence or invocation identity
  created_at
  files: Vec<EditFileState>

EditFileState
  path
  pre: FileState
  post: FileState

FileState
  Absent
  Present { hash, content }
```

Use established typed identity types where available. Do not use process CWD, display labels, or path strings as workspace identity.

### Storage and migrations

Prefer extending the current snapshot storage/schema if it can represent checkpoint metadata cleanly. If overloading the existing `snapshot.data` payload would create ambiguous semantics between full snapshots and edit checkpoints, add a narrow typed discriminator/metadata field or a dedicated checkpoint table that reuses the same file-state serialization and size limits.

Do not duplicate file bodies in multiple stores without measured need. A schema migration must remain backward compatible with existing snapshots.

### Runtime and concurrency

Move canonical restorable mutation capture into `ToolBatchExecutor` or a directly adjacent service where accepted tool calls, execution context, and batch boundaries are known.

For each batch containing supported file mutations:

1. derive and normalize the complete affected path set before execution;
2. reject/mark non-restorable any path set that cannot be safely derived;
3. capture pre-state for all paths;
4. run normal tool permission/execution logic unchanged;
5. capture post-state for all paths that were actually part of the checkpoint scope;
6. persist the checkpoint only when the resulting state meaningfully represents the mutation batch;
7. associate the checkpoint with exact turn/session/workspace identity.

Parallel tool execution must not allow two mutating calls against overlapping paths to produce ambiguous pre/post ordering. Reuse existing batch serialization or workspace/path exclusivity where present; otherwise serialize overlapping restorable mutations narrowly rather than adding daemon-global locking.

### Native tool affected-path rules

The implementation must explicitly handle:

- `write`: one target path; pre may be absent/present; post present;
- `edit`: one existing path;
- `replace`: one existing path;
- `multiedit`: one existing path;
- `apply_patch update`: one existing path;
- `apply_patch create`: one target path, normally absent -> present;
- `apply_patch delete`: one existing path, present -> absent;
- `apply_patch move`: both source and destination paths, including destination pre-state if the current tool permits replacement.

If actual tool semantics differ, encode those semantics rather than forcing this list mechanically.

### Event compatibility

Do not require adding session/workspace identity to every `FileChanged` consumer if durable history no longer depends on it. However, if the event continues to describe agent-owned mutations to concurrent frontends, consider adding optional scoped identity in a backward-compatible way and update routing accordingly.

### Documentation and static guards

Update `architecture/snapshot.md` and `architecture/tool.md` to distinguish:

- full safety snapshots;
- durable restorable edit checkpoints;
- observational `FileChanged` events.

Prefer tests/type boundaries over a regex guard listing mutating tool names in multiple places. If a central mutation metadata table/catalog exists, make checkpoint eligibility derive from it so new native mutators cannot silently bypass coverage.

## 7. Ordered work packages

### Work package A — Canonical checkpoint model and persistence

Intent: establish one durable representation for exact pre/post edit state.

Required changes:

- add typed checkpoint/file-state structures;
- add compatible persistence/retrieval APIs;
- support absent/present states and hashes;
- enforce existing snapshot bounds/path safety.

Acceptance evidence:

- persistence round-trip tests for create/update/delete/move states;
- legacy snapshot records remain readable;
- oversized/unsafe paths fail predictably.

### Work package B — Affected-path extraction and pre-state capture

Intent: make capture complete before mutation begins.

Required changes:

- centralize supported native-mutator affected-path extraction;
- derive paths from accepted structured arguments;
- capture pre-state before dispatch;
- mark unsupported/ambiguous calls non-restorable rather than guessing.

Acceptance evidence:

- table-driven tests cover all supported tool/mode combinations;
- malformed move/create/delete arguments cannot produce incomplete checkpoints;
- two workspaces with the same relative filename remain isolated.

### Work package C — Post-state capture and event decoupling

Intent: remove unscoped event draining as durable history authority.

Required changes:

- capture post-state from the same path set after execution;
- persist exact batch/turn identity;
- stop using drained global `FileChanged` events to decide durable checkpoint contents;
- retain/update observational events for TUI diff behavior as needed.

Acceptance evidence:

- a foreign workspace `FileChanged` event cannot enter another turn's checkpoint;
- `multiedit` and every `apply_patch` mode produce correct checkpoint state;
- failed mutations do not fabricate successful post-state.

### Work package D — Concurrency, compatibility, and docs

Intent: close overlap/race hazards without broad refactoring.

Required changes:

- serialize or reject overlapping restorable mutations within a batch as needed;
- document non-restorable mutation classes;
- update snapshot/tool architecture docs and focused tests.

Acceptance evidence:

- overlapping-path concurrency test has deterministic outcome;
- independent-path/workspace concurrency remains parallel where safe;
- existing diff UI/event tests continue to pass.

## 8. Failure, cancellation, restart, and contention semantics

If pre-state capture fails, the tool may continue only if existing policy permits ordinary execution, but that batch/path must be explicitly non-restorable; never store a partial checkpoint and call it complete.

If execution partially fails after mutating some paths, capture actual post-state for the original bounded path set and mark checkpoint/result status so later tooling can reason about what occurred. Do not automatically roll back in this milestone.

Cancellation after mutation likewise records observable post-state where possible; cancellation is not equivalent to rollback.

Daemon restart reads persisted checkpoints from storage. No broadcast receiver state is required.

Overlapping mutating calls must have a deterministic serialization/rejection rule so checkpoint A's post-state cannot ambiguously become checkpoint B's pre-state under concurrent writes.

## 9. Compatibility and migration

Existing `snapshot` records and explicit restore operations must remain readable/usable. New checkpoint metadata should be additive.

`FileChanged` consumers should remain compatible unless an additive identity field is introduced. Avoid a broad event protocol break solely to fix internal checkpoint attribution.

Configuration should preserve current snapshot enablement behavior where possible, but if lightweight edit checkpointing is separated from expensive full-project snapshots, document the default explicitly and avoid silently enabling large captures.

## 10. Required tests

### Focused unit tests

- `FileState` absent/present serialization and hashing;
- affected-path extraction per native mutator/mode;
- path normalization/containment and duplicate path deduplication;
- checkpoint persistence round trip.

### Integration tests

- write/edit/replace/multiedit checkpoints;
- apply_patch create/update/delete/move checkpoints;
- failed/partial mutation behavior;
- two sessions/workspaces emitting concurrent observational events cannot cross-contaminate checkpoint state.

### Restart and recovery tests

- persisted checkpoint reload after recreating manager/service;
- legacy snapshot migration/read compatibility.

### Contention and cancellation tests

- overlapping file mutations in one batch;
- independent files can retain allowed concurrency;
- cancellation after pre-capture/before mutation and after mutation.

### Security and negative tests

- symlink/path escape rejection;
- oversized/non-supported content handling;
- unsupported plugin/MCP/shell mutations are not mislabeled restorable.

## 11. Required verification commands

```bash
cargo test -p codegg-core snapshot
cargo test --test snapshot
cargo test tool_batch
cargo test apply_patch
cargo test multiedit
scripts/verify.sh quick
```

Adjust selectors to actual test organization. Keep verification focused and do not add new CI lanes.

## 12. Documentation updates

- `architecture/snapshot.md`
- `architecture/tool.md`
- `architecture/agent.md` if it describes incremental event-drain capture
- closure record: `plans/closure/runtime-safety-resource-footprint/011-status.md`

## 13. Acceptance criteria

- Durable edit checkpoints are explicitly workspace/session/turn/batch scoped.
- Durable capture no longer trusts unscoped global `FileChanged` draining as its source of truth.
- Every supported native file mutator has complete affected-path pre/post coverage.
- Create/delete/move are represented correctly with absent/present file states.
- Unsupported/ambiguous mutations are explicitly non-restorable.
- Concurrent workspaces cannot contaminate each other's checkpoints.
- Existing snapshot safety constraints and ordinary tool permissions remain intact.
- Focused tests and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Stop and report when:

- safe affected-path derivation requires parsing arbitrary shell/plugin/MCP behavior;
- correct ordering would require a daemon-global filesystem lock;
- snapshot storage cannot support bounded edit checkpoints without a broader storage redesign;
- a public protocol break appears necessary solely for internal provenance;
- implementation starts building Undo/Reapply UI before checkpoint correctness is closed.

## 15. Closure evidence required

- implementation commit(s);
- schema/migration evidence if any;
- per-tool/mode affected-path and pre/post-state test matrix;
- cross-workspace contamination regression evidence;
- overlapping mutation/cancellation evidence;
- proof that durable checkpoint contents no longer depend on unscoped `FileChanged` draining;
- exact verification commands and outcomes;
- unresolved findings and explicit non-restorable classes.

## 16. Handoff notes

Do not create a new `TurnMutationJournal` or parallel history database by default. Reuse and clarify the existing snapshot domain. M012 must remain blocked until this plan has a closure record demonstrating complete checkpoint semantics.
