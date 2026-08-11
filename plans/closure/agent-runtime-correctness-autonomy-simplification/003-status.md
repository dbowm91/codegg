# Agent Runtime Correctness, Autonomy, and Simplification M003 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/agent-runtime-correctness-autonomy-simplification/003-workspace-bound-agent-loop-construction.md`
Source subsystem roadmap: `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md#7-ordered-milestones`
Repository baseline reviewed: `8c2638db`
Implementation commits: `8c2638db — bind agent loop construction to workspace identity`; `f71303de — fix workspace snapshot regression fixture`

## 1. Executive finding

M003 is strictly closed. Production `AgentLoop` construction now receives the
workspace root and session identity before initializing snapshots or other
workspace-sensitive state. The daemon turn build input carries the immutable
`ExecutionContext`; no production setter can replace the workspace root after
construction.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Explicit workspace identity before loop initialization | `AgentLoop::new` requires `workspace_root` and `session_id`; `AgentLoopBuildInput` requires `Arc<ExecutionContext>` | pass |
| Snapshot root follows owning workspace | Snapshot manager receives the constructor root; explicit-root capture test captures only workspace B | pass |
| No production workspace-root setter | `set_workspace_root` and `runtime_factory` removed | pass |
| Path/mutation checks use explicit root | `is_path_within_workspace` and `is_workspace_file_mutation` receive the loop root; explicit-root and CWD-independent tests pass | pass |
| Multi-workspace state is not ambient-CWD driven | Root is stored as immutable `PathBuf`; tool execution context and subagent/security-review paths derive from it | pass |
| Standalone/test construction is explicit | CLI/exec/subagent callers pass roots; harness callers pass deliberate fixture roots | pass |
| No protocol or storage migration | No protocol/schema files changed; snapshot identifiers and storage API remain unchanged | pass |

## 3. Production implementation evidence

Before M003, `DefaultTurnRuntime` populated a many-field build input, delegated
through `AgentLoopFactory` to `runtime_factory::build_agent_loop`, constructed
the loop with `workspace_root = None`, and then called `set_workspace_root`.
Snapshots were already initialized from process CWD at that point.

After M003, `DefaultTurnRuntime` passes the daemon `ExecutionContext` to one
`build_agent_loop` function. That function constructs permission state and calls
`AgentLoop::new` with the execution root and session ID. Snapshot construction,
tool execution CWD, workspace identity derivation, and subagent fallback
requests all use the captured root. The obsolete `runtime_factory` module and
factory trait were deleted.

Removed production ambient-CWD/setter dependencies:

- snapshot initialization no longer calls `std::env::current_dir()`;
- workspace mutation approval no longer calls `std::env::current_dir()`;
- `set_workspace_root()` was deleted;
- `runtime_factory::build_agent_loop` was deleted;
- `AgentLoopFactory`/`DefaultAgentLoopFactory` indirection was replaced by one
  typed construction function.

## 4. Verification executed

Local verification:

- `rtk cargo fmt --all` — passed.
- `rtk cargo check --lib` — passed.
- `rtk cargo check --tests` — passed.
- `rtk cargo test --lib agent::r#loop::tests::workspace_file_mutation` — passed.
- `rtk cargo test --test snapshot` — passed.
- `rtk cargo test --test agent_loop_harness -- --test-threads=1` — passed.
- `rtk scripts/verify.sh quick` — passed through its bounded quick checks.
- `rtk python3 scripts/check_daemon_cwd_usage.py` — passed.
- `rtk python3 scripts/check_project_agent_pwd_inference.py` — passed.
- `rtk python3 scripts/check_discovery_invariants.py` — passed.
- `rtk python3 scripts/check_execution_ownership.py` — passed.

One unrelated repository guard remains inconsistent with the current baseline:
`check_project_catalog_invariants.py` expects storage layout version 33 while
the repository declares version 35. It is not affected by M003 and is recorded
as a low-severity maintenance finding below.

## 5. Invariant review

Workspace authority is immutable after construction. Relative paths resolve
under the captured root, missing mutation targets validate their explicit
parent, and tool execution CWD uses the same root. No global mutable workspace
state or process-CWD synchronization was introduced.

## 6. Failure and recovery review

Subagent execution now rejects requests without an explicit workspace root
instead of silently inheriting ambient process state. Existing snapshot async,
cancellation, and restore behavior is unchanged apart from receiving the
correct root.

## 7. Migration and compatibility review

No schema, protocol, or snapshot-record migration is required. Internal
standalone and test callers were updated to pass explicit fixture roots.
`set_session_id` remains as a narrow compatibility label override for existing
harness callers; it cannot alter workspace authority. Normal daemon callers
use the constructor session ID.

## 8. Security review

The change removes process-CWD influence from workspace mutation auto-approval
and snapshot capture. Traversal and symlink/canonicalization behavior remains
in the existing path checks; paths outside the explicit root are not treated as
workspace mutations.

## 9. Documentation and operations

`architecture/agent.md` and `architecture/core.md` now document construction
ownership and the direct typed build function. No new guard was added because
the existing daemon-CWD and project-agent guards cover the relevant authority
surface.

## 10. Unresolved findings (severity: critical/high/medium/low)

- Low: `scripts/check_project_catalog_invariants.py` has a stale expected
  `STORAGE_LAYOUT_VERSION` of 33; the current repository is at 35. This is
  unrelated to M003 and should be reconciled by the project-catalog owner.
- None for M003 production correctness, security, migration, or compatibility.

## 11. Roadmap disposition

M003 is closed. M004 remains ready and is independent. M005 remains blocked on
M004; M006 remains blocked on M005; and M009 remains blocked on M001–M008.
M003 is only a soft dependency for M005, so closing it does not make any
blocked plan dependency-ready.

## 12. Registry updates

The registry removes M003 from dependency-ready work, records it under recently
closed work with commit `8c2638db`, and advances the active workstream pointer
to M004/M007–M008. The blocked-work audit found no plan whose remaining hard
dependencies were all satisfied by this closure; no plan was unblocked.
