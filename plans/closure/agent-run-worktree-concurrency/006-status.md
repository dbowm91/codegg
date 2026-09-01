# Agent Run, Async Delegation, and Worktree Concurrency M006 — Closure Status

Status: closed

Closure date: 2026-09-01

Reviewed implementation commit: `7bc39c2845ca7d6cc8e56b9d051b080347961f16`

Implementation plan: `plans/implementation/agent-run-worktree-concurrency/006-projection-compatibility-and-closure.md`

Source roadmap: `plans/subsystems/agent-run-worktree-concurrency-roadmap.md`

Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`

## 1. Executive finding

M006 is fully implemented and closed. Durable agent-run, worktree, and
run-group state now travels through the existing bounded session projection
contract. Reconnect/resync snapshots are rebuilt from the authoritative stores,
incremental events use the same DTOs, and the canonical reducer remains pure.
The TUI can inspect concurrent child identity, status, worktree/branch, result
commit, validation, group state, and attention state.

No high- or medium-severity correctness, security, cancellation, restart,
worktree-leak, integration, contention, or compatibility finding remains in
this workstream.

## 2. Milestone and commit chain

| Milestone | Implementation commit | Reviewed closure evidence |
|---|---|---|
| M001 | `8cc449c25b0959bf849daff3a7ba676eecb2b3ba` | `plans/closure/agent-run-worktree-concurrency/001-status.md` |
| M002 | `36e19e6f93610029e608549e40846508d96f692f` | `plans/closure/agent-run-worktree-concurrency/002-status.md` |
| M003 | `0f3d75bf07f93cc375fed966cf324dff837187ad` | `plans/closure/agent-run-worktree-concurrency/003-status.md` |
| M004 | `37b9cc9c9442fbca20fa63072581b4be1067deaf` | `plans/closure/agent-run-worktree-concurrency/004-status.md` |
| M005 | `babff39770e7023918c3785feb63ff392a3c2732` | `plans/closure/agent-run-worktree-concurrency/005-status.md` |
| M006 | `7bc39c2845ca7d6cc8e56b9d051b080347961f16` | this record |

The M005 roadmap/registry close commit is `633f97a3644934059d5f7f8a8bb4d07d7fbbb7e6`.
All six milestones were reviewed against the same repository baseline.

## 3. Delivered scope and ownership map

| Concern | Authoritative owner | Projection/compatibility seam |
|---|---|---|
| task/run identity and lifecycle | `AgentRunStore` / `AgentRunService` | bounded `AgentRunSummary` |
| queue, attempt, and machine resources | `JobStore` / global `JobScheduler` | existing job projection |
| child execution | `SubagentJobExecutor` through scheduler | durable run events |
| mailbox, control, cancellation, restart | run mailbox/journal and `run_control` | control-status/attention events |
| managed worktree and cleanup | `WorktreeService` / lease records | `WorktreeSummaryProjection` |
| child result and integration | structured result store / typed Git integration | result commit/validation summary |
| groups and joins | `AgentRunGroupService` | `AgentRunGroupSummaryProjection` |
| frontend state | pure `ProjectionReducer` | session snapshot/replay/TUI |

The mental model is:

```text
spawn -> durable AgentRun -> scheduler
                       -> mailbox/journal
                       -> optional owned worktree
                       -> structured result
                       -> explicit integration
```

Projection code performs no I/O and never submits work, allocates a worktree,
sends control, or performs integration.

## 4. Acceptance evidence

- Additive, serde-bounded run/worktree/group DTOs and events are defined in
  `codegg-protocol`; old snapshots default the new collections to empty and
  older clients ignore the additive fields/events.
- `crates/codegg-core/src/projection_replay/agent_runs.rs` is a pure adapter
  from authoritative records. It omits prompts, mailboxes, transcripts,
  hidden reasoning, credentials, full paths, and artifact bodies.
- The daemon projection subscribe path reconstructs durable runs, worktrees,
  and groups from stores. Scheduler terminal transitions, control operations,
  and group operations publish the same summaries for incremental replay.
- The lower-level event-log publication adapter and safe-publication checks
  handle the new events, so reconnect, replay, and resync use one seam.
- The TUI sidebar shows bounded concurrent runs, stable typed identity, agent,
  branch/worktree, result commit, status, and attention-required state.
- `spawn_many`, group joins, `wait`, and push notifications are preferred in
  the task-tool contract; `status`/`get` remain documented compatibility paths.
- The scheduler is the only daemon machine-capacity authority. The pool’s
  semaphore remains only for standalone/legacy or semantic descendant limits;
  scheduled child execution does not acquire it.

## 5. Failure, restart, contention, and security review

The predecessor M001–M005 records provide the production-shaped evidence for
typed identity, persist-before-signal control, first-terminal-wins behavior,
restart reconciliation, worktree isolation/retention, child commits and typed
integration conflicts, group joins, and detached/background completion. M006
preserves those contracts and adds projection-only observation. Projection lag,
duplicate events, reconnect, resync, and unknown durable IDs do not invoke
control or execution.

Dirty/conflicted worktrees remain represented as retained attention state;
projection does not make them eligible for cleanup. Concurrent mutating runs
continue to use separate managed worktree leases, while read-only runs avoid
unnecessary allocation. Run/worktree/group summaries are bounded and no secret
or hidden-reasoning disclosure path was introduced.

## 6. Legacy compatibility inventory

| Legacy surface | Disposition | Evidence/criterion |
|---|---|---|
| numeric `TaskStore` IDs and reads/writes | retained compatibility adapter | older task callers and standalone constructors remain supported; typed IDs are preferred |
| `SubagentStarted/Progress/Completed/Failed` events | retained compatibility events | older clients/active-turn consumers remain readable; durable events are the new source for durable runs |
| `SubAgentPool` direct runtime | retained narrow adapter | standalone/stdio and child-runtime semantics; no daemon scheduler bypass |
| pool machine-capacity semaphore | retained only outside scheduled daemon admission | scheduled path skips it; global scheduler owns machine resources |
| `task get` | retained stable alias | compatibility inspection/control path with explicit migration guidance to typed IDs/wait/push |
| duplicate daemon admission or dual durable authority | none found | scheduler-bypass and execution-ownership guards pass; no safe deletion was indicated by naming-only search |

No historical legacy task was fabricated into a typed run or worktree. Removal
of the retained aliases is deferred until the negotiated older-client and
standalone migration window is intentionally ended.

## 7. Exact verification

All commands were run locally on the reviewed implementation tree and were
prefixed with the repository RTK wrapper.

- `rtk cargo test -p codegg-protocol --locked -- --test-threads=1` — 163 passed.
- `rtk cargo test --lib agent --locked -- --test-threads=1` — 358 passed.
- `rtk cargo test --lib scheduler --locked -- --test-threads=1` — 75 passed.
- `rtk cargo test --test session_projection_consumer --test scheduler_restart_recovery --test scheduler_contention --test scheduler_cancellation --test worktree --locked -- --test-threads=1` — 61 passed.
- `rtk cargo test -p codegg-protocol projection::reducer::tests::durable_run_projection_replays_and_old_snapshots_default_new_fields -- --nocapture` — 1 passed.
- `rtk scripts/verify.sh quick` — passed on the exact post-cleanup tree.
- `rtk cargo fmt --all -- --check` — passed.
- `rtk git diff --check` — passed before implementation commit.
- `rtk bash scripts/check-core-boundary.sh` — passed.
- `rtk python3 scripts/check_scheduler_bypass.py` — passed.
- `rtk python3 scripts/check_execution_ownership.py` — passed.
- `rtk python3 scripts/check_daemon_cwd_usage.py` — passed.
- `rtk python3 scripts/check_sandbox_contract.py` — passed.
- `rtk python3 scripts/check_git_forbidden_patterns.py` — passed, 0 findings.
- `rtk python3 scripts/check_identity_path_usage.py` — passed.
- `rtk python3 scripts/check_tui_project_authority.py` — passed.
- `rtk python3 scripts/check_tool_broker_boundary.py` — passed.
- `rtk bash scripts/check_projection_disclosure.sh` — passed.

The exact required broad posture remains the existing quick verification; no
new CI lane, release gate, benchmark, coverage, or scanner was added.

## 8. Dependency audit and registry disposition

The only M006 blocker was M001–M005 completion; all predecessor closure records
were accepted before implementation. The agent-run/worktree roadmap now has no
remaining milestone after M006. The repository registry was audited for future
plans depending on this workstream: no registered future plan remains blocked
on M006, so no unrelated plan status required promotion. Historical references
in older closure records were left unchanged for traceability.

The implementation plan, roadmap, registry, architecture docs, and this record
are updated together. Final recommendation: **closed**.
