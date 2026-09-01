# Agent Run, Async Delegation, and Worktree Concurrency Milestone 005 — Closure Status

Status: closed

Closure date: 2026-09-01

Reviewed implementation commit: `babff39770e7023918c3785feb63ff392a3c2732`

Implementation plan: `plans/implementation/agent-run-worktree-concurrency/005-run-groups-and-background-joins.md`

Source roadmap: `plans/subsystems/agent-run-worktree-concurrency-roadmap.md`

## 1. Executive finding

M005 is fully implemented and closed. CodeGG now has a bounded durable run-group
coordination service over existing `AgentRun` records. Groups do not admit work,
reserve scheduler capacity, or create a second executor. Members continue to use
the canonical scheduler and M004 worktree/result paths.

## 2. Delivered scope

- Added typed `AgentRunGroupId`, `RunJoinPolicy`, group status, bounded member
  summaries, and terminal notifications in `codegg-core`.
- Added in-memory and SQLite group stores with normalized membership, unique
  idempotency keys, notification claim state, and additive schema migration 41.
- Added deterministic `all`, `any_successful`, `first_completed`, and `detached`
  recomputation. `first_completed` uses persisted member order as its stable tie
  break; `any_successful`/`first_completed` persist explicit sibling-cancellation
  policy.
- Added ownership validation: members must be direct children of the owner and
  belong to the same root lineage. Group cancellation only requests cancellation
  for recorded members.
- Added bounded task-tool actions `spawn_many`, `create_group`, `status_group`,
  `wait_group`, and `cancel_group`. Partial fan-out acceptance is reported
  explicitly, and group waits return an active/timeout summary rather than
  treating a wait timeout as job failure.
- Connected terminal run control to group recomputation and bounded parent
  follow-up notifications. No per-group polling task or unowned background Tokio
  lifetime was introduced.
- Reused the existing scheduler-backed Tool Program background handle and
  notification contract. Tool Programs remain separate from agent-run groups;
  ordinary direct/parallel tools remain turn-local.
- Updated agent, scheduler, and Tool Program architecture documentation.

## 3. Acceptance evidence

| Acceptance area | Evidence | Result |
|---|---|---|
| Deterministic joins | Core tests cover all-member mixed outcomes, any-success, first-completed order, and detached completion | met |
| Bounds and idempotency | `MAX_GROUP_MEMBERS = 16`, bounded summaries, duplicate-member rejection, stable store idempotency tests | met |
| Ownership and cancellation | Direct-child/root-lineage validation and cancellation-only-of-members tests | met |
| Detached/restart behavior | SQLite group reload test reconstructs terminal state without respawning members | met |
| Parent notification | Group notification is claimed once and terminal group follow-up uses the existing M002 live channel | met |
| Model-facing fan-out | Task tool exposes bounded spawn-many/group control actions and explicit partial acceptance | met |
| Scheduler/background boundary | Existing Tool Program durable background handle/notification suite passes; groups never submit jobs directly | met |
| Isolation and structured results | Children are created through the existing M004 durable task path, preserving automatic worktree/result behavior | met |
| Authority separation | Architecture docs and implementation keep Tool Programs, direct batches, scheduler admission, and groups as separate authorities | met |

## 4. Verification executed

All results are local verification; no hosted CI claim is made.

- `rtk cargo test -p codegg-core agent_run_group -- --nocapture` — 5 passed.
- `rtk cargo test --lib task -- --nocapture` — 50 passed.
- `rtk cargo test --test scheduler_restart_recovery --test scheduler_cancellation -- --test-threads=1` — 25 passed.
- `rtk cargo test --test tool_program_background -- --test-threads=1` — 9 passed.
- `rtk cargo clippy -p codegg-core --all-targets --locked -- -D warnings` — passed.
- `rtk cargo fmt --all -- --check` — passed.
- `rtk scripts/verify.sh quick` — passed.
- `rtk bash scripts/check-core-boundary.sh` — passed.
- `rtk python3 scripts/check_scheduler_bypass.py` — passed.
- `rtk python3 scripts/check_execution_ownership.py` — passed.
- `rtk python3 scripts/check_daemon_cwd_usage.py` — passed.
- `rtk python3 scripts/check_git_forbidden_patterns.py` — passed.
- `rtk python3 scripts/check_project_agent_pwd_inference.py` — passed.
- `rtk git diff --check` — passed before implementation commit.

## 5. Failure, recovery, and security review

- Group state is derived from authoritative member records and persisted before
  cancellation signals or notifications. Terminal group state is monotonic.
- SQLite membership is normalized and foreign-keyed to agent runs. Duplicate
  creation is idempotent and conflicting reuse of a key is rejected.
- A group cannot adopt an unrelated run: every member must be a direct child of
  the owner in the same root lineage. Session/run actor checks protect control
  operations.
- Wait timeout is a bounded observation result; it does not mutate member state.
- Detached groups remain durable and scheduler-owned through the existing jobs
  and runs. Restart reconstructs state and never respawns a terminal member.
- Summaries contain statuses, bounded result references, and bounded failure
  metadata only; transcripts, hidden reasoning, credentials, and full artifacts
  are not aggregated.

## 6. Unresolved findings

None. The initial clippy findings in the new error type were corrected before
closure; final core clippy and quick verification were green.

## 7. Dependency audit and registry disposition

M006 — projection, compatibility simplification, and strict closure — had its
only declared blocker, M001–M005 completion, resolved by this closure. It is
moved from `blocked` to `ready` in both the subsystem roadmap and
`plans/registry.md`. No other registered plan was blocked on M005.

The implementation plan is marked `implemented`, the roadmap M005 row is
`closed`, and this closure record is the controlling completion evidence.
