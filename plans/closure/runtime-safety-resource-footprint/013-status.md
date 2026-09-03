# Runtime Safety Milestone 013 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-safety-resource-footprint/013-cross-session-checkpoint-atomicity-and-hosted-closure-corrective-pass.md`

Source corrective roadmap:

- `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md`

Historical predecessors preserved:

- M011 implementation and closure: `plans/implementation/runtime-safety-resource-footprint/011-mutation-attribution-and-edit-checkpoints.md`, `plans/closure/runtime-safety-resource-footprint/011-status.md`
- M012 implementation and closure: `plans/implementation/runtime-safety-resource-footprint/012-checked-undo-reapply.md`, `plans/closure/runtime-safety-resource-footprint/012-status.md`

Repository baseline reviewed: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Implementation commit:

- `f314c38e689a940ff614fe2e08e0c21ee07b2191` — serialize eligible checkpointed native mutation intervals across sessions, fail closed on mixed unknown effects, and replace repeated SQL row tuples with typed rows.

Hosted acceptance evidence:

- Failed predecessor: `CI / verify` run `33683938442`, job `100426769862`, exact head `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`; Workspace Clippy failed on checkpoint row-type complexity before Workspace tests.
- Accepted replacement: `CI / verify` run `33712437859`, job `100514597927`, exact head `f314c38e689a940ff614fe2e08e0c21ee07b2191`; formatting, Workspace Clippy, and Workspace tests passed in 18m53s.

## 1. Executive finding

M013 is closed as the strict corrective disposition for checked edit history.
The native checkpoint interval now has one shared per-repository authority
across pre-state capture, native execution, post-state capture, and durable
persistence. The authority is the existing workspace-service
`WorkspaceLockTable`, retained by a daemon-owned workspace lease; no second
history journal and no daemon-global filesystem lock were introduced.

Logical batches now fail closed when a supported native mutation is mixed with
an unknown or potentially mutating call. Affirmatively read-only companions
remain eligible through the existing effect classifier. Existing checked
Undo/Reapply compare-before-mutate behavior and checkpoint storage remain
unchanged.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Same-workspace same-path sessions cannot contaminate checkpoints | `same_path_batches_serialize_capture_mutate_capture_and_persist` | pass |
| Guard covers the complete checkpoint transaction | Production `ToolBatchExecutor` guard is acquired before `capture_states`, retained through tool execution and post capture, and dropped after persistence | pass |
| Canonical lock authority is shared across detached turns | `CoreDaemon` retains `WorkspaceServicesLease`; `TurnRunInput`, `AgentLoop`, `TaskTool`, and child requests propagate its lock table | pass |
| Independent workspaces remain concurrent | `WorkspaceLockTable` is owned by each workspace service; `two_workspaces_isolated_same_relative_path` and workspace isolation tests pass | pass |
| Pure read-only batches are not serialized by checkpointing | Checkpoint eligibility requires at least one restorable mutation; read-only-only batches never acquire the guard | pass |
| Mixed native plus unknown side effects fail closed | Core affected-path tests, `unsupported_tool_not_mislabeled_restorable`, and tool-batch classification coverage for Bash, MCP, and plugin names | pass |
| Native plus authoritative read-only remains eligible | `mixed_affirmative_read_only_call_remains_restorable` and integration `read_mixed` coverage | pass |
| Existing M011 checkpoint behavior remains intact | 22 `edit_checkpoint_integration` tests, including create/update/delete/move/apply-patch and persistence cases | pass |
| Existing M012 checked restore remains intact | 23 `checked_restore_integration` tests, including stale, conflict, undo, reapply, restart, and workspace isolation | pass |
| Rust 1.98 Workspace Clippy row findings are corrected | Private `sqlx::FromRow` representations in `snapshot/checkpoint.rs`; `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| Cancellation/error releases authority | Owned repository guard is RAII; all early returns and task cancellation drop the guard. No new manual unlock path exists | pass |
| No storage/protocol redesign | Existing `edit_checkpoint` and `edit_restore_operation` records remain compatible; no migration or public DTO change | pass |

## 3. Production implementation evidence

- `crates/codegg-core/src/snapshot/affected_paths.rs` now centralizes logical
  batch eligibility. Supported native mutation paths are extracted only when
  every other accepted call is affirmatively read-only; unknown effects return
  `None` for the complete batch.
- `src/agent/tool_batch.rs` derives the complete accepted batch, normalizes its
  bounded paths, acquires the workspace-service repository guard, captures
  pre-state, executes the batch, captures post-state, and persists the
  checkpoint before releasing the guard. Missing checkpoint identity or lock
  authority is fail-closed and does not create a partial checkpoint.
- `src/core/daemon.rs` acquires and retains the workspace service lease for a
  detached turn. `AgentLoop`, the turn runtime, and child-agent/task seams
  preserve the same lock-table identity where a child shares the workspace.
- `crates/codegg-core/src/snapshot/checkpoint.rs` uses private typed SQL row
  structs for checkpoint and restore-operation queries. The storage layout and
  historical records are unchanged.
- Architecture documentation now describes transaction scope, logical-batch
  eligibility, lease retention, and repository-level serialization in
  `architecture/snapshot.md`, `architecture/tool.md`, and
  `architecture/workspace_services.md`.

## 4. Why the prior test matrix missed the defect

M011 tested same-workspace concurrent batches on different files and duplicate
paths within one batch. Those cases did not place independent session loops on
the same path across the full pre/mutate/post interval. The new deterministic
fixture holds the first session's guard after pre-capture, starts the second
session while the guard is held, then verifies the ordered `initial -> first ->
second` checkpoint states and durable records. Mixed-batch extraction also
previously demonstrated native-subset extraction without testing the stronger
logical-batch restore rule; M013 adds the fail-closed cases.

## 5. Verification executed

### Local commands

```bash
cargo fmt --check --all                         # via scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test -p codegg-core -- snapshot
RUSTFLAGS='-L native=/usr/local/lib -L native=/usr/local/opt/libiconv/lib' cargo test --test edit_checkpoint_integration
RUSTFLAGS='-L native=/usr/local/lib -L native=/usr/local/opt/libiconv/lib' cargo test --test checked_restore_integration
RUSTFLAGS='-L native=/usr/local/lib -L native=/usr/local/opt/libiconv/lib' cargo test --lib agent::tool_batch::invocation_tests
RUSTFLAGS='-L native=/usr/local/lib -L native=/usr/local/opt/libiconv/lib' cargo test --test workspace_services_isolation workspace_lock_table_serializes_acquire_repository
scripts/verify.sh quick
git diff --check
```

### Results

- `cargo fmt --check --all`: passed through the quick tier.
- Workspace Clippy with `-D warnings`: passed with no issues.
- Core snapshot tests: 23 passed, 519 filtered out.
- Checkpoint integration: 22 passed, including the new same-path transaction regression.
- Checked restore integration: 23 passed.
- Tool-batch classification unit tests: 3 passed.
- Existing workspace-lock isolation test: 1 passed.
- `scripts/verify.sh quick`: passed generated-agent validation, core boundary,
  sandbox contract, execution ownership, formatting, and locked all-target
  compilation.
- All additional required static guards passed, including daemon-CWD,
  scheduler bypass, tool-broker boundary, provider-connection compatibility,
  projection transport, and WebSocket bounds guards.
- `git diff --check`: passed.
- The local executable tests required the explicit native library search paths
  above because this host's default x86_64 Rust link path selected arm64
  MacPorts `liblzma`/iconv libraries. The focused tests passed with the
  matching `/usr/local` paths; hosted Ubuntu verification is the authoritative
  exact-candidate result.
- Hosted run `33712437859` / job `100514597927` passed through Workspace tests
  on the exact implementation commit. Its Node.js 20 action warning is an
  existing CI-environment annotation and did not affect the result.

## 6. Failure, recovery, and contention review

- Permission checks complete before the checkpoint guard is acquired, so a
  pending question cannot hold workspace mutation authority.
- A capture failure makes the batch non-restorable and does not fabricate a
  checkpoint. A persist failure leaves no claimed durable success; RAII still
  releases the guard.
- Cancellation, panic, and ordinary early-return paths release the owned guard.
- Repository-level contention serializes same-workspace same-repository
  checkpointed mutation intervals. Different workspace service lock tables
  remain independent. A pure read-only batch does not enter this path.
- Existing checked Undo/Reapply locking, stale-content conflict behavior,
  restart reconstruction, path containment, symlink protection, permission
  checks, and bounded conflict reporting were not redesigned.
- Daemon restart continues to read existing checkpoint and restore-operation
  records. M013 does not rewrite historical checkpoints or claim stronger
  provenance for them.

## 7. Compatibility and migration

No storage migration, protocol change, or new history subsystem is required.
Existing `edit_checkpoint` and `edit_restore_operation` rows remain readable.
Older checkpoints retain their existing provenance limitations and continue to
be protected by checked restore's content preconditions. Existing callers that
construct loops without a workspace-service lease fail closed for new durable
checkpoint capture rather than using an unscoped lock.

## 8. Security and authorization

M013 changes serialization and provenance, not permission policy. Existing
permission checks remain authoritative and run before lock acquisition.
Unknown Bash, Git mutation, plugin, MCP, and other potentially mutating calls
execute only if existing permissions allow them, but make the complete logical
batch non-restorable. No file bodies, command output, or secrets are emitted
in logs, protocol diagnostics, or this closure record.

## 9. Documentation and planning updates

Updated:

- `architecture/snapshot.md`
- `architecture/tool.md`
- `architecture/workspace_services.md`
- the M013 implementation plan
- `plans/subsystems/runtime-safety-edit-history-corrective-addendum.md`
- `plans/registry.md`
- this closure record

M011 and M012 closure records were not rewritten. M013 is the new strict
correctness disposition for the checked edit-history follow-up only. The main
runtime-safety roadmap remains conditionally closed for its unrelated,
previously recorded supported-Linux Landlock fixture evidence.

## 10. Unresolved findings by severity

| Severity | Finding | Disposition |
|---|---|---|
| — | No unresolved M013-scope correctness, security, compatibility, or hosted-verification finding | Closed by focused and exact-hosted evidence |
| Environmental note | Local root executable tests need `/usr/local` native library paths on this mixed-architecture host | Not a production or hosted issue; focused tests passed with the matching paths |
| Existing CI annotation | Node.js 20 action deprecation warning on `actions/checkout@v4` | Informational only; CI passed and M013 does not change workflow policy |

## 11. Roadmap disposition and future-plan audit

M013 is strictly closed. The checked edit-history corrective addendum is
closed, and the implementation plan is marked implemented. The main
runtime-safety/resource-footprint roadmap remains conditionally closed only
for its pre-existing supported-Linux Landlock evidence condition; M013 did not
touch that sandbox path.

The complete `plans/` dependency/reference audit found no later registered
plan whose blocker is runtime-safety checked-edit-history M013. Therefore no
future plan was unblocked and no unrelated plan status was changed. The M013
entry was removed from the dependency-ready table and added to the recently
closed control points.

## 12. Registry updates

- The runtime-safety checked-edit-history corrective follow-up moved from
  active to closed in `plans/registry.md`.
- M013 was removed from the dependency-ready table and added to recently
  closed control points with exact implementation and hosted evidence.
- The corrective addendum records M013 as closed.
- The implementation plan records accepted implementation/closure status.
- Historical M011/M012 implementation and closure records remain intact.
- No future plan was promoted or otherwise changed because the dependency audit
  found no registered plan blocked on M013.
