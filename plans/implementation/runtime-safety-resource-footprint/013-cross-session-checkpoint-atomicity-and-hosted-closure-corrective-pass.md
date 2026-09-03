# Runtime Safety Milestone 013 — Cross-Session Checkpoint Atomicity and Hosted Closure Corrective Pass

Status: active

Repository baseline: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Source corrective roadmap:

- `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md`

Original milestones and closure records corrected by this pass:

- M011: `plans/implementation/runtime-safety-resource-footprint/011-mutation-attribution-and-edit-checkpoints.md`
- M011 closure: `plans/closure/runtime-safety-resource-footprint/011-status.md`
- M012: `plans/implementation/runtime-safety-resource-footprint/012-checked-undo-reapply.md`
- M012 closure: `plans/closure/runtime-safety-resource-footprint/012-status.md`

Long-term requirements:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Applicable architecture:

- `architecture/snapshot.md`
- `architecture/tool.md`
- `architecture/workspace_services.md`
- `architecture/agent.md`

Primary class: corrective invariant / concurrency / closure

## 1. Objective

Correct the remaining edit-history attribution race so a durable checkpoint cannot incorporate another session's same-workspace mutation, make mixed native/unknown-side-effect batches fail closed for restore eligibility, and restore truthful exact-head hosted closure after the current `main` push failed Workspace Clippy.

This is a corrective pass over the existing checkpoint and checked-restore design. Preserve `EditCheckpointManager`, `SnapshotManager`, `ToolBatchExecutor`, `WorkspaceLockTable`, and checked Undo/Reapply ownership. Do not create a second history journal, event-sourcing layer, file watcher, or daemon-global filesystem lock.

## 2. Discovered defects

### 2.1 Cross-session same-path checkpoint contamination

M011 moved canonical checkpoint capture to `ToolBatchExecutor`, which correctly derives a bounded path set and captures pre/post state without trusting global `FileChanged` events. It only serializes overlapping calls *within the same tool batch* by lowering that batch's local semaphore to one.

Two different agent loops/sessions may still execute in the same workspace concurrently. The current implementation does not hold one shared mutation exclusivity boundary across:

```text
pre-state capture
  -> native mutation execution
  -> post-state capture
```

for the affected path set.

Therefore this sequence is possible:

```text
session A captures foo.rs = X
session B captures foo.rs = X
session A writes foo.rs = A
session B writes foo.rs = B
session A captures foo.rs = B
```

A's checkpoint may then persist `pre=X, post=B`, attributing B's mutation to A. Checked Undo can subsequently see current `B` as an exact post-state match and restore `X`, thereby reverting another session's work while behaving consistently with the corrupted checkpoint.

This violates the M011 invariant that a checkpoint must not include another session/workspace/turn's mutation.

### 2.2 Mixed restorable and unknown-mutating batches can be mislabeled

`extract_batch_affected_paths()` currently ignores non-restorable calls and returns the supported native subset. A batch containing a supported native file mutation plus `bash`, arbitrary MCP/plugin execution, Git mutation, or another unclassified side-effecting call can therefore record a checkpoint for only the native path set.

If the untracked call mutates one of those paths during the same batch interval, the resulting checkpoint can absorb the unknown side effect while still appearing restorable.

The safe default is not partial certainty. A logical batch is restorable only when every accepted call is either:

- part of the supported restorable mutation contract; or
- affirmatively classified read-only/non-filesystem-mutating by an existing authoritative effect classification.

Unknown or potentially mutating calls must make the entire batch non-restorable unless their effects are captured by a future typed effect contract.

### 2.3 Exact-head hosted CI is red

Push CI run `33683938442` for exact head `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce` failed at `Workspace Clippy` before workspace tests. Rust 1.98 reports six `clippy::type_complexity` errors in `crates/codegg-core/src/snapshot/checkpoint.rs` for repeated large SQL tuple row types.

The M012 closure record reports local verification as green but was committed before this exact hosted result existed. The historical closure record must remain intact, but the subsystem cannot be considered strictly closed until this corrective milestone records a green exact candidate.

## 3. Why original verification missed these defects

- M011 had a two-workspace isolation test and a same-workspace concurrent test using *different files*; neither exercises two sessions mutating the same path across independent tool batches.
- the overlapping-path test covered duplicate paths inside one batch and therefore validated only local semaphore serialization.
- mixed-batch tests explicitly demonstrated extraction of the native subset but did not assert the stronger logical-batch restore eligibility rule.
- M012's checked-restore tests correctly validate stale content and all-path preconditions, but a checked restore cannot detect bad provenance if the checkpoint's recorded post-state already includes another session's write.
- local checks did not reproduce the exact hosted Rust 1.98 Workspace Clippy `type_complexity` findings before the closure commit was written.

## 4. Invariants that must not regress

- checkpoint pre/post state is attributable to exactly one workspace/session/turn/batch.
- no checkpoint may include a mutation from another concurrent session.
- compare-before-mutate checked Undo/Reapply semantics from M012 remain unchanged.
- `FileChanged` remains observational and is not restored as durable authority.
- independent workspaces remain concurrent.
- independent read-only operations must not be serialized merely because checkpointing exists.
- do not add a daemon-global mutation lock.
- shell/plugin/MCP/Git arbitrary effects remain non-restorable unless a future explicit effect contract owns them.
- existing path containment, symlink protection, snapshot bounds, tool permissions, cancellation, and scheduler authority remain intact.
- exact-hosted verification remains the repository's existing single CI lane; do not add a new workflow or matrix.

## 5. Required production changes

### 5.1 Shared mutation transaction boundary

Inspect all existing native file mutation and workspace lock acquisition paths before changing locking. The corrective implementation must establish a single shared exclusivity boundary for each checkpointed mutation interval:

```text
acquire existing/narrow mutation authority
  -> capture all pre-states
  -> execute supported native restorable mutations
  -> capture all post-states
  -> persist checkpoint
release authority
```

Prefer composing with the existing `WorkspaceLockTable` / workspace-services mutation authority rather than introducing an unrelated lock registry.

If the native tool path already acquires the same non-reentrant repository lock internally, do not nest-acquire and deadlock. Refactor the smallest seam so checkpoint capture and mutation execute under one shared transaction/guard, or introduce a path-scoped coordinator only if repository-level reuse would create unacceptable serialization and the existing owner cannot express the needed critical section.

The implementation must remain scoped to one canonical workspace/repository. It must not serialize unrelated workspaces.

A repository-level lock is acceptable for this corrective pass if it is the existing canonical mutation authority and measured/code-review evidence shows it does not create a new daemon-global bottleneck. Do not build a complex lock hierarchy solely to preserve theoretical parallel writes.

### 5.2 Inter-batch overlap correctness

Two sessions or turns that target the same path in the same workspace must produce a deterministic serial history. For example:

```text
X -> A -> B
```

must result in two valid ordered checkpoints:

```text
A: pre X, post A
B: pre A, post B
```

or the opposite ordering if B legitimately acquires authority first. The forbidden outcome is either checkpoint recording another session's post-state without having owned that mutation interval.

The lock/transaction must cover the final pre-state read through the final post-state read. Acquiring only around tool execution while leaving capture outside the guard is insufficient.

### 5.3 Mixed-batch restore eligibility

Introduce one explicit logical-batch eligibility decision adjacent to affected-path extraction. Avoid duplicating lists of mutating tool names across modules.

Required behavior:

- supported restorable native mutations only: checkpoint normally;
- supported restorable mutations plus affirmatively read-only tools: checkpoint may proceed;
- supported restorable mutations plus unknown/potentially mutating `bash`, terminal, Git mutation, arbitrary plugin/MCP call, or equivalent side effect: execute normally under existing permission policy but mark the whole batch non-restorable and persist no misleading checkpoint;
- purely non-restorable batch: no checkpoint;
- malformed/unsafe affected paths: preserve current fail-closed non-restorable behavior.

If an existing central effect/risk classification can provide the read-only/mutating distinction safely, reuse it. Otherwise add the smallest typed classification next to the existing affected-path/restorable contract; do not infer safety from tool names scattered through call sites.

### 5.4 Clippy row-type cleanup

Replace the repeated large SQL tuples in `crates/codegg-core/src/snapshot/checkpoint.rs` with a maintainable typed row representation or narrowly shared alias.

Preferred order:

1. private `sqlx::FromRow` structs when row semantics are meaningful;
2. a private type alias if that is materially simpler and remains readable;
3. a targeted `#[allow(clippy::type_complexity)]` only when the typed alternatives make the code worse and the closure record explains why.

Do not globally weaken `-D warnings` or modify CI to ignore the failure.

## 6. Storage, protocol, and migration

No storage-layout change should be necessary for the primary correction. Existing `edit_checkpoint` and `edit_restore_operation` records remain compatible.

Do not rewrite historical checkpoints or pretend old records have stronger provenance than they do. Existing records remain inspectable; safe restore continues to rely on content preconditions.

No protocol change should be needed. If operator diagnostics need to identify a batch as non-restorable, prefer existing bounded metadata/logging rather than exposing file bodies or adding a broad DTO redesign.

## 7. Ordered work packages

### WP A — Reproduce the inter-session race

Before production changes, add a deterministic regression fixture with two independent session/loop contexts targeting the same relative path in one workspace.

The fixture must demonstrate that without shared exclusivity a contaminated pre/post sequence is possible or directly exercise the production boundary after correction.

Acceptance evidence:

- two concurrent session mutations to one path serialize into coherent ordered checkpoints;
- checkpoint session/turn identities correspond to the mutation interval they owned;
- no test relies on sleeps alone when a barrier/channel can make ordering deterministic.

### WP B — Establish shared checkpoint mutation authority

Thread or reuse the canonical workspace mutation guard so pre-capture, native execution, post-capture, and checkpoint persistence are one critical interval for restorable batches.

Acceptance evidence:

- same-path cross-session test passes repeatedly;
- different workspaces remain parallel;
- no nested-lock deadlock in native mutation, Git/worktree, Bash translation, or Undo/Reapply paths;
- cancellation/error releases the guard through RAII.

### WP C — Fail closed on mixed unknown side effects

Implement logical-batch eligibility before checkpoint pre-capture.

Acceptance evidence:

- `write + bash` executes according to existing permissions but yields no restorable checkpoint;
- `write + arbitrary MCP/plugin tool` yields no restorable checkpoint;
- `write + read` remains restorable if `read` is explicitly classified read-only;
- supported native-only batches retain current checkpoint behavior.

### WP D — Restore exact-head hosted closure

Fix the current Clippy findings without weakening CI.

Acceptance evidence:

- local `cargo clippy --workspace --all-targets --locked -- -D warnings` passes on the final candidate;
- normal push CI runs on the exact final candidate and reaches/passes Workspace tests;
- closure record cites the failed predecessor run `33683938442` and the accepted replacement run/job.

## 8. Failure, cancellation, restart, and contention semantics

- cancellation before mutation releases the guard and persists no false checkpoint;
- cancellation/partial failure after mutation still captures actual post-state under the same guard where possible and follows existing checkpoint status semantics;
- capture failure makes the batch non-restorable but does not silently claim success;
- another session cannot enter a conflicting checkpointed mutation interval until the current guard is released;
- daemon restart continues to read existing checkpoints and restore logs unchanged;
- stale Undo/Reapply conflict behavior remains exactly as M012 defines it.

## 9. Security and authorization

This milestone changes serialization/provenance, not authorization. Existing permission checks still decide whether a tool may execute.

Do not acquire a restore/checkpoint lock before a user permission prompt in a way that blocks unrelated workspace activity for the prompt duration. Derive/validate permission first where current architecture permits, then acquire mutation authority immediately before pre-state capture and hold it through the mutation interval.

No captured file content may be added to logs, CI output, protocol diagnostics, or closure evidence.

## 10. Required tests

Focused tests must include:

- cross-session same-workspace same-path serialization;
- opposite acquisition ordering remains coherent;
- same workspace different paths behavior documented and tested according to the selected lock granularity;
- different workspaces retain concurrency;
- mixed `write + bash` non-restorable;
- mixed native mutation + arbitrary MCP/plugin non-restorable;
- native mutation + confirmed read-only tool remains restorable where supported;
- cancellation/error releases mutation guard;
- existing M011 create/update/delete/move checkpoint tests;
- existing M012 stale/conflict/undo/reapply tests.

## 11. Required verification

Use the repository's existing minimal posture plus exact hosted evidence:

```bash
cargo fmt --check --all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg-core -- snapshot
cargo test --test edit_checkpoint_integration
cargo test --test checked_restore_integration
scripts/verify.sh quick
git diff --check
```

Run any existing workspace-lock isolation test affected by the chosen seam.

After the final corrective commit is pushed, inspect the ordinary `CI / verify` run for that exact SHA. Do not add or manually expand CI merely to obtain closure evidence.

## 12. Documentation updates

Update as needed:

- `architecture/snapshot.md` — inter-session transaction/serialization semantics and mixed-batch eligibility;
- `architecture/tool.md` — restorable logical-batch classification;
- `architecture/workspace_services.md` — only if the shared lock/transaction contract changes;
- corrective roadmap and registry;
- new closure record `plans/closure/runtime-safety-resource-footprint/013-status.md`.

Do not rewrite M011/M012 closure records. M013 explains the later-discovered defects and supersedes only their strict subsystem disposition.

## 13. Acceptance criteria

- two concurrent sessions cannot create a checkpoint whose pre/post state includes the other's same-path mutation;
- the shared guard covers pre-capture through post-capture/persistence for checkpointed mutations;
- mixed unknown-mutating batches are not labeled restorable;
- no daemon-global lock or second history subsystem is added;
- existing checked Undo/Reapply behavior remains intact;
- current Rust 1.98 Workspace Clippy failures are corrected without weakening `-D warnings`;
- focused tests and `scripts/verify.sh quick` pass;
- the exact final SHA receives a green normal hosted `CI / verify` run through Workspace tests;
- M013 closure records the failed predecessor run and accepted replacement evidence.

## 14. Stop conditions

Stop and report rather than improvise when:

- the only available design requires a daemon-global lock;
- reusing `WorkspaceLockTable` creates unavoidable reentrant deadlocks and no narrow existing transaction seam can be exposed cleanly;
- correct same-path attribution would require intercepting arbitrary external filesystem writers;
- mixed-batch correctness would require parsing arbitrary shell/MCP/plugin side effects rather than failing closed;
- a storage/protocol redesign becomes necessary;
- implementation begins broad scheduler, worktree, or plugin-runtime refactoring unrelated to checkpoint atomicity.

## 15. Closure evidence required

The M013 closure record must include:

- implementation commit(s);
- explicit reference to M011/M012 historical closure records;
- explanation of why the prior test matrix missed inter-session same-path overlap;
- deterministic cross-session same-path regression evidence;
- mixed-batch non-restorable regression evidence;
- lock ownership/deadlock review and cancellation behavior;
- exact local verification commands/outcomes;
- failed hosted predecessor `33683938442` / job `100426769862`;
- exact replacement hosted run/job and final candidate SHA;
- unresolved findings by severity;
- recommendation: strict closed, conditionally closed, or another corrective pass required.
