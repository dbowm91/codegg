# Agent Runtime, Model Adaptation, and ACP Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/003-bounded-nested-agent-delegation.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-003--bounded-nested-agent-delegation`

Repository baseline reviewed: `b893462`

Implementation commits:

- `b893462` — `feat(agent): enable bounded nested delegation`

## 1. Executive finding

Milestone 003 is closed. The existing shared `SubAgentPool` remains the only
descendant admission authority, and eligible child loops now receive that
same pool and functional `task` runtime. Delegation is bounded by depth,
target policy, direct-child count, active descendants, concurrency, cumulative
child tool-call reservation, wall-clock timeout, and stable request identity.
The implementation remains a compatibility bridge; durable `AgentRun`
storage/restart recovery and mutation worktree isolation remain out of scope.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Shared child spawner is installed | `src/agent/worker.rs`; child-loop construction calls `set_subagent_pool` and registers `TaskTool` only for explicit `task = "allow"` | pass | No recursive pool is created |
| Explicit, target-bounded delegation | `SubagentConfig`, built-in `general.toml`, target allow/deny checks | pass | Prompt text alone cannot enable delegation |
| Depth/fan-out/concurrency bounds | Pool admission checks and semaphore; `test_max_depth_returns_error_before_queueing`; existing concurrency tests | pass | Direct-child and active-descendant limits are shared pool state |
| Idempotency | SHA-256 delegation key and duplicate-task admission; `test_duplicate_delegation_identity_is_rejected_before_queueing` | pass | Stable fields are used without inventing durable `AgentRunId` |
| Parent authority/path ceiling | inherited denied tools and lexical descendant path-scope intersection in `TaskTool` | pass | Mutation-capable parallel worktree policy remains deferred |
| Model inheritance and timeout | resolved parent model passed to child; configurable wall-clock timeout | pass | Provider token accounting remains a bounded seam |
| Cancellation and cleanup | pool cancellation select, semaphore cancellation, shutdown join/abort fallback, active-count RAII | pass | Full durable restart recovery is deferred |
| Production-shaped execution | `cargo test --test subagent` (22 passed) and `cargo test --test agent_loop_harness` (40 passed) | pass | Child-loop construction is covered by the production worker path |

## 3. Production implementation evidence

`SubagentConfig` now carries the global delegation kill switch, target
allow/deny lists, and bounded fan-out/active/tool/time limits. `SubAgentPool`
tracks accepted identities and shared reservations. Child execution installs
the pool, carries the inherited model and denied-tool ceiling, registers a
functional task tool only for explicitly delegating agents, and applies the
inherited allowed-path scope. Built-in general agents explicitly advertise
delegation through the generated asset path. Configuration and architecture
documentation describe the contract.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test -p codegg-config subagent_delegation_bounds_deserialize -- --nocapture
cargo test --test subagent
cargo test --test agent_loop_harness
scripts/verify.sh quick
python3 scripts/generate_builtin_agents.py --check
python3 scripts/check_builtin_agents.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_project_agent_pwd_inference.py
python3 scripts/check_discovery_invariants.py
```

### Results

All listed commands passed. `subagent` reported 22 passed tests and
`agent_loop_harness` reported 40 passed tests. Quick verification passed.
`check_project_catalog_invariants.py` was also run but reports the repository
baseline mismatch that `STORAGE_LAYOUT_VERSION` is 35 while that guard still
expects 33; no project-catalog code was changed by this milestone.

## 5. Invariant review

- Admission remains shared through one pool and its request queue.
- Child authority is narrowed through inherited denied tools and paths.
- Depth, fan-out, active work, concurrency, tool-call, and time limits are
  checked before or during execution.
- Duplicate stable delegation identities do not enqueue a second child.
- `task` is not exposed when the loop lacks a functional spawner or when the
  resolved agent has not explicitly allowed delegation.

## 6. Failure and recovery review

Queue rejection occurs before worker execution and rolls back direct-child
and tool-call reservations. Semaphore cancellation and pool shutdown return
through the existing response path; active counts use an RAII guard. A
wall-clock timeout produces one failed terminal result. Durable daemon
restart reconciliation and final AgentRun recovery are explicitly deferred
to later roadmap work.

## 7. Migration and compatibility review

The existing `SubAgentRequest` and task/session display identifiers remain
compatible. New configuration fields are additive and absent fields retain
the existing first-level pool defaults. No storage migration or protocol
breaking change was introduced.

## 8. Security review

Delegation is disabled globally when configured, targets can be allowlisted or
denied, child tool authority inherits parent denials, and child paths cannot
escape an inherited non-empty path scope. Mutation-capable parallel children
remain without automatic worktrees and therefore are not presented as a
completed isolation capability.

## 9. Documentation and operations

Updated `architecture/config.md`, built-in agent configuration, generated
assets, and the implementation/roadmap registry. The quick verification
entrypoint and relevant static ownership guards remain green.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Project-catalog invariant guard expects storage layout 33 while code is at 35 | Unrelated guard cannot be green at this baseline | Reconcile the guard in the project-catalog/release verification workstream |
| low | Durable AgentRun restart recovery and mutation worktree isolation are not implemented | Final long-term hierarchy remains incomplete | Owned by later roadmap milestones; not a M003 blocker |

## 11. Roadmap disposition

Milestone closed and next dependencies may proceed. M004 specialized
security-review runtime and M005 specialized research runtime are promoted
from blocked to ready because M003 was their only hard dependency. M006 was
already independently ready; M008–M011 retain their stated predecessor
blocks.

## 12. Registry updates

- Marked M003 implemented/closed and added this closure record.
- Promoted M004 and M005 to `ready` in the same close change.
- Updated the subsystem roadmap and registry to show M001–M003 closed and
  M004/M005 ready.
- Recorded the unrelated project-catalog guard mismatch without changing its
  owning subsystem.
