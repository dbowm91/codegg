# Runtime Safety — Checked Edit History Corrective Addendum

Status: closed — M013 closed

Repository baseline reviewed: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Parent roadmap/addendum:

- `plans/subsystems/runtime-safety-resource-footprint-roadmap.md`
- `plans/subsystems/runtime-safety-edit-history-addendum.md`

Historical milestones preserved:

- M011 closed: `plans/closure/runtime-safety-resource-footprint/011-status.md`
- M012 closed: `plans/closure/runtime-safety-resource-footprint/012-status.md`

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

## 1. Corrective purpose

The original edit-history work established the correct architecture: exact pre/post checkpoints owned by the snapshot domain, durable capture at the native tool-batch boundary, and checked Undo/Reapply that validates current content before mutation. A post-closure review found one remaining concurrency defect and one restore-eligibility defect that the original test matrix did not exercise, plus a failed exact-head hosted Clippy run after the M012 closure commit.

This addendum does not reopen the architecture or erase M011/M012 history. It adds one corrective milestone that must close before the checked edit-history line is again treated as strictly closed.

## 2. Discovered corrective findings

### Cross-session same-path attribution

M011 serializes duplicate/overlapping paths inside one tool batch but does not establish shared exclusivity across independent session/turn batches in the same workspace from pre-capture through post-capture. Another session may therefore mutate the same path inside a checkpoint interval and become incorporated into the wrong checkpoint.

Because M012 correctly trusts the durable checkpoint's exact post-state, a provenance-corrupted checkpoint can later undo another session's change without triggering a stale-content conflict.

### Mixed unknown side effects

A batch containing a supported native mutation plus an untracked potentially mutating call currently extracts and checkpoints the native subset. This can misrepresent the logical batch as restorable even though a Bash/MCP/plugin/Git side effect may have changed the same path.

### Exact hosted closure

Exact head `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce` failed normal CI run `33683938442`, job `100426769862`, at Workspace Clippy due to `type_complexity` findings in `snapshot/checkpoint.rs`. Workspace tests were skipped. Local closure evidence remains historical but is not sufficient for strict current-head closure.

## 3. Invariants

- one checkpoint owns one coherent mutation interval;
- concurrent sessions cannot contribute mutations to each other's checkpoints;
- mixed unknown-mutating batches are non-restorable unless all side effects are covered by an explicit effect contract;
- checked Undo/Reapply remains compare-before-mutate and fail-closed;
- existing snapshot storage and restore ownership remain canonical;
- no daemon-global mutation lock is introduced;
- independent workspaces remain concurrent;
- permissions, cancellation, path containment, symlink protection, and size bounds do not weaken;
- CI remains the existing minimal single `CI / verify` workflow.

## 4. Corrective milestone

### M013 — Cross-session checkpoint atomicity and hosted closure corrective pass

Status: closed — see `plans/closure/runtime-safety-resource-footprint/013-status.md`

Plan:

- `plans/implementation/runtime-safety-resource-footprint/013-cross-session-checkpoint-atomicity-and-hosted-closure-corrective-pass.md`

Class: corrective invariant / concurrency / closure

Dependencies:

- hard: none beyond historical M011/M012 implementation already on `main`;
- interface: existing workspace mutation/lock authority and `ToolBatchExecutor` checkpoint seam.

Exit conditions:

- same-workspace same-path mutations from independent sessions serialize into coherent ordered checkpoints;
- the exclusivity boundary covers pre-capture through post-capture/persistence;
- mixed supported mutation + unknown/potentially mutating side-effect batches produce no misleading restorable checkpoint;
- existing M012 Undo/Reapply tests remain green;
- Rust 1.98 Workspace Clippy passes without weakening `-D warnings`;
- focused tests and `scripts/verify.sh quick` pass;
- a normal hosted `CI / verify` run is green on the exact accepted final candidate through Workspace tests;
- closure record is written at `plans/closure/runtime-safety-resource-footprint/013-status.md`.

## 5. Why the corrective milestone is independently ready

The defects are within existing ownership boundaries. The repository already has:

- explicit workspace/session/turn identity;
- `ToolBatchExecutor` as the canonical native mutation/checkpoint seam;
- `WorkspaceLockTable` and workspace-service mutation serialization primitives;
- durable `EditCheckpointManager` storage;
- checked restore and full regression suites;
- a minimal hosted CI lane that reproduces the current Clippy failure.

No new scheduler, runtime, storage subsystem, or protocol design is required.

## 6. Verification posture

Use deterministic same-path cross-session fixtures, mixed-batch negative tests, existing checkpoint/checked-restore integration suites, Workspace Clippy, `scripts/verify.sh quick`, and one ordinary hosted push run on the final SHA.

Do not add CI lanes, stress matrices, coverage gates, file watchers, or broad performance infrastructure.

## 7. Deferred work remains deferred

This corrective pass does not add rollback for arbitrary:

- shell/terminal effects;
- plugin/MCP filesystem effects;
- Git history mutation;
- binary/non-UTF-8 content;
- external human/process writes outside CodeGG's mutation authority.

Those remain explicitly non-restorable or conflict-detected according to existing architecture.

## 8. Closure disposition

M013 is the accepted strict corrective disposition for the checked edit-history
line. The original M011/M012 records remain valid historical implementation
evidence and are not rewritten. No further corrective milestone is registered
behind this addendum.
