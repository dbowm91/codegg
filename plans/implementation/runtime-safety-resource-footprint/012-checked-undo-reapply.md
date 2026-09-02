# Runtime Safety Milestone 012 — Checked Undo and Reapply

Status: blocked

Repository baseline: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source roadmap:

- `plans/subsystems/runtime-safety-edit-history-addendum.md#6-dependency-graph`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None. Preserve ordinary workspace mutation authority and snapshot safety semantics.

Primary class: capability

Hard dependency:

- `plans/implementation/runtime-safety-resource-footprint/011-mutation-attribution-and-edit-checkpoints.md` must be closed with complete pre/post checkpoint semantics.

## 1. Objective

Expose safe Undo/Reapply for supported CodeGG-native edit checkpoints using all-path precondition verification and existing snapshot/path authority, with no silent overwrite of later human, external, or concurrent changes.

## 2. Why this milestone is blocked

The user-facing capability is only correct after M011 establishes:

- exact workspace/session/turn checkpoint identity;
- complete supported native-mutator path coverage;
- explicit absent/present pre/post state;
- deterministic ordering for overlapping mutations;
- durable restart-safe checkpoint persistence.

Implementing UI/commands before those invariants close would recreate an unsafe partial-history layer.

## 3. Current implementation evidence

At the reviewed baseline `SnapshotManager` already has `restore()` and `restore_to_path()` with path-containment and atomic-write mechanics, but these are explicit restore APIs, not compare-and-swap edit-history operations. They do not establish that current files still equal the post-state produced by the edit being undone.

The TUI already has diff/file-change presentation seams, and session/projection infrastructure can carry bounded command results. The missing behavior is checked restoration and a clear user surface.

## 4. Invariants that must not regress

- Undo never overwrites a path whose current state differs from the checkpoint's recorded post-state.
- Reapply never overwrites a path whose current state differs from the checkpoint's recorded pre-state.
- All paths in a checkpoint are validated before the first restore mutation begins.
- A stale/conflicting path fails the logical operation as a whole.
- Restore uses the same explicit workspace root, containment, symlink, permission, and mutation authority as normal file writes.
- Unsupported side effects are not represented as undone/reapplied.
- Undo/Reapply is scoped to the intended session/workspace history and cannot target another workspace by checkpoint ID alone.
- A successful Undo produces enough durable state to support a corresponding Reapply without relying on in-memory UI state.

## 5. Scope

### In scope

- checked restore API over M011 edit checkpoints;
- exact state comparison for present/absent files using hashes and safe content checks;
- all-path preflight before restore;
- logical operation serialization against concurrent workspace mutation;
- Undo/Reapply command/protocol/TUI surface consistent with existing command conventions;
- bounded list/status information sufficient to identify the latest eligible checkpoint/turn;
- durable operation result/audit metadata sufficient for restart and reapply;
- clear reporting of conflicts and unsupported mutations.

### Explicitly out of scope

- arbitrary shell/terminal rollback;
- Git commit/branch reset or history rewriting;
- plugin/MCP side-effect rollback unless a later typed mutation contract explicitly makes them eligible;
- semantic three-way merge when current content diverged;
- automatic rollback on ordinary tool failure;
- multi-workspace atomic transactions;
- replacing explicit snapshot restore commands if they have a distinct operator use case.

## 6. Required production changes

### Core/domain

Add a checked restore operation to the snapshot/edit-checkpoint domain rather than altering ordinary `restore()` into a silently stricter API if compatibility would be unclear.

Representative result types should distinguish:

- applied;
- conflict/stale path set;
- unsupported/non-restorable checkpoint;
- not found/wrong workspace;
- permission/path validation failure;
- partial I/O failure after validation.

The precondition comparison must support absent/present state and must compare all checkpoint paths before writes/deletes begin.

### Runtime and contention

Acquire the narrow existing workspace mutation/exclusivity boundary before final validation and hold it through restore application so another CodeGG mutation cannot race between the compare and write phases.

Do not add a daemon-global lock. Independent workspaces remain independent.

For multi-path restore, prepare every operation first. Apply with the strongest practical all-or-nothing semantics available from the filesystem. Because cross-file atomicity is not generally available, if an unexpected I/O error occurs after writes begin:

- stop further writes when safe;
- record exactly which paths were applied;
- return a typed degraded/partial failure;
- do not claim successful Undo;
- preserve enough evidence for explicit operator recovery.

The ordinary stale-content case must be caught before this phase and produce zero mutations.

### Reapply lineage

A successful Undo must create or update durable lineage so Reapply knows the exact inverse transition and expected current states. Avoid an in-memory stack as authority.

Repeated Undo/Reapply requests must be idempotent or return a typed conflict when current state no longer matches the expected side of the checkpoint.

### Protocol and frontend

Follow existing frontend-neutral command/projection patterns. Expose only bounded metadata such as checkpoint/turn ID, timestamp, path count, restorable status, and conflict paths.

The TUI/command surface should support at minimum:

- undo latest eligible edit checkpoint/turn for the current workspace/session;
- reapply the latest successfully undone checkpoint;
- clear explanation when no eligible checkpoint exists;
- conflict explanation naming bounded affected paths without dumping file bodies.

Do not couple the core API to TUI state.

### Security and authorization

Checked restore is a mutating operation and must pass the same canonical authorization/path policy as equivalent file edits. A checkpoint is evidence, not authorization.

Checkpoint IDs are not bearer capabilities. Resolve them only within explicit workspace/session scope and validate every stored relative path again at execution time.

### Documentation

Update `architecture/snapshot.md`, relevant TUI/command docs, and user-facing history semantics. Document the exact unsupported classes and the fail-closed conflict rule.

## 7. Ordered work packages

### Work package A — Checked restore core

Intent: establish compare-before-mutate semantics independent of UI.

Required changes:

- implement pre/post side selection for Undo/Reapply;
- validate all current states against expected states;
- return bounded typed conflicts;
- apply present/absent state changes through existing safe filesystem helpers.

Acceptance evidence:

- any single stale path prevents every path in the logical restore from changing;
- create/delete/move inverse cases behave correctly;
- checkpoint from another workspace/session is rejected.

### Work package B — Contention and durable inverse lineage

Intent: make checked restore stable across concurrency and restart.

Required changes:

- hold narrow workspace mutation serialization from final compare through apply;
- persist successful Undo/Reapply lineage/state;
- define idempotent/repeated request behavior;
- record partial I/O failure truthfully.

Acceptance evidence:

- concurrent CodeGG edit racing Undo cannot bypass precondition checking;
- restart after successful Undo still permits the intended Reapply;
- duplicate requests do not double-apply changes.

### Work package C — Command/protocol/TUI surface

Intent: expose the capability without making the frontend authoritative.

Required changes:

- add bounded request/response DTOs or reuse an existing command protocol seam;
- add current-session/workspace commands and TUI feedback;
- surface conflict/non-restorable/no-history outcomes clearly.

Acceptance evidence:

- end-to-end TUI/command integration test invokes core checked restore;
- frontend never writes files directly for Undo/Reapply;
- remote/frontend-neutral request path remains scoped and bounded.

### Work package D — Documentation and compatibility closure

Intent: document what “Undo” means precisely.

Required changes:

- update architecture/user docs;
- retain explicit snapshot restore behavior unless intentionally migrated;
- capture supported/unsupported side-effect matrix.

Acceptance evidence:

- docs state that later external changes block Undo rather than being overwritten;
- tests preserve existing explicit snapshot APIs.

## 8. Failure, cancellation, restart, and contention semantics

Cancellation before the mutation phase leaves all files unchanged. Once a cross-file apply begins, cancellation should not intentionally interrupt between individual path writes if doing so would worsen partial-state risk; use the shortest bounded critical section practical and report cancellation after the operation reaches a stable recorded outcome.

Stale-content conflicts are normal failures and produce zero mutation.

Unexpected filesystem errors after validation may produce partial physical application because filesystems do not offer a general cross-file transaction. Such a result must be typed as partial/degraded and persist exact applied-path evidence; it must never advance the logical Undo/Reapply stack as if successful.

Daemon restart reconstructs eligibility and inverse lineage from durable checkpoint/operation state.

## 9. Compatibility and migration

Keep existing snapshot capture/restore records readable. Undo/Reapply should be additive.

If M011 introduces a new checkpoint table/schema, this milestone consumes it without another redesign. Old snapshots lacking pre/post checkpoint semantics are not automatically eligible for Undo.

No existing session should become unsafe merely because it has historical snapshots that cannot be classified as edit checkpoints.

## 10. Required tests

### Focused unit tests

- compare expected/current present/absent states;
- conflict aggregation before mutation;
- inverse mapping for create/update/delete/move;
- wrong-workspace/session rejection;
- duplicate/idempotent request behavior.

### Integration tests

- undo and reapply supported single-file edits;
- multi-file move or batch edit;
- stale one-of-many path prevents all mutation;
- human/external edit between tool completion and Undo causes conflict;
- command/protocol/TUI path reaches the same core service.

### Restart and recovery tests

- successful Undo -> restart -> Reapply;
- restart with a pending/partial degraded result does not falsely expose a normal Reapply.

### Contention and cancellation tests

- concurrent CodeGG edit versus Undo;
- concurrent Undo requests;
- cancellation before critical mutation phase.

### Security and negative tests

- checkpoint path traversal/symlink manipulation after capture;
- checkpoint ID from another workspace;
- unsupported shell/plugin/MCP mutations remain non-restorable;
- no file bodies leaked in normal conflict UI/log output.

## 11. Required verification commands

```bash
cargo test -p codegg-core snapshot
cargo test --test snapshot
cargo test undo
cargo test reapply
cargo test tui
scripts/verify.sh quick
```

Use actual focused selectors after implementation. No additional CI lanes are required.

## 12. Documentation updates

- `architecture/snapshot.md`
- relevant command/TUI architecture docs
- user-facing documentation for Undo/Reapply behavior
- closure record: `plans/closure/runtime-safety-resource-footprint/012-status.md`

## 13. Acceptance criteria

- Undo/Reapply uses M011 durable edit checkpoints, not a new parallel journal.
- Every path is compared against the expected checkpoint side before any logical restore mutation.
- Any ordinary stale/conflicting path causes zero mutation for the whole operation.
- Workspace/path permission checks are re-evaluated at restore time.
- Undo/Reapply state survives daemon restart.
- Unsupported side effects are clearly excluded.
- Frontends invoke the core checked restore service and do not own mutation semantics.
- Focused tests and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Stop and report when:

- M011 is not strictly closed;
- implementation requires silently treating old full snapshots as edit checkpoints;
- a requested rollback class cannot be expressed as bounded file pre/post state;
- correctness would require a daemon-global lock or a new general transaction engine;
- the frontend would need to bypass canonical file mutation authority;
- semantic merge of divergent files becomes necessary to satisfy acceptance.

## 15. Closure evidence required

- M011 closure dependency reference;
- implementation commit(s);
- stale-conflict zero-mutation evidence;
- create/update/delete/move Undo/Reapply matrix;
- workspace-isolation and path-security evidence;
- restart/idempotency evidence;
- command/protocol/TUI end-to-end evidence;
- exact verification commands/outcomes;
- explicit unsupported-side-effect inventory;
- any partial-I/O limitation and severity assessment.

## 16. Handoff notes

Do not weaken the fail-closed rule for convenience. The primary value of this capability is that Undo will not erase later work. Exact state conflict is preferable to a clever automatic merge in this milestone.
