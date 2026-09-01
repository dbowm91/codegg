# M001 — Durable AgentTask/AgentRun Foundation Closure

Status: closed

Closure date: 2026-09-01

Reviewed implementation head: `8cc449c25b0959bf849daff3a7ba676eecb2b3ba`

Implementation plan: `plans/implementation/agent-run-worktree-concurrency/001-durable-agent-run-foundation.md`

Source roadmap: `plans/subsystems/agent-run-worktree-concurrency-roadmap.md`

## Sources and baseline

- Repository baseline: `b08d33b7e52bde1bde1ddcddeeee3c7c157a4103`.
- Primary implementation commit: `8cc449c25b0959bf849da3ba7a676eecb2b3ba`.
- Governing requirements: the M001 implementation plan, the agent-run/worktree roadmap, the durable hierarchy/scheduler/recovery sections of `plans/000-long-term-specification.md`, and ADR-0001/ADR-0002.

The implementation was reviewed against the existing daemon scheduler, typed identity, migration, TaskTool compatibility, and SubAgentPool seams. The legacy `task` table and numeric task handles remain compatibility state; new daemon delegations use the canonical typed records below.

## Executive finding

M001 is closed. Daemon delegated execution now creates or resolves one typed durable task/run identity before scheduler submission, links the run to the scheduler job and attempt, and records bounded provenance and terminal state. `JobRecord`/`JobAttempt` remain the scheduler queue and attempt authority. The SubAgentPool remains a child-runtime/semantic-admission adapter, while scheduled work does not acquire its duplicate machine-capacity semaphore.

## Requirement-to-evidence matrix

| Acceptance criterion | Evidence | Result |
|---|---|---|
| 1. Stable task/run records before execution | `codegg_core::agent_run::{AgentTaskRecord,AgentRunRecord}`; TaskTool `create_or_get` precedes `JobSubmissionService::submit` | met |
| 2. One scheduler job/attempt lineage | `JobPayload::SubagentRun`; TaskTool attaches `JobId`; `SubagentJobExecutor` attaches `AttemptId` before child execution | met |
| 3. Deterministic duplicate submission | unique delegation key in migration 37; in-memory and SQLite duplicate tests; scheduler submission idempotency tests | met |
| 4. Scheduler-only machine-capacity admission | scheduled worker mode skips pool semaphore; scheduler contention suite remains green; execution-ownership and scheduler-bypass guards pass | met |
| 5. Bounded provenance | normalized session/project/repository/workspace/lineage fields plus bounded agent/model/digest/authority/budget/failure/result fields | met |
| 6. Exactly one terminal run state | transition validation, first-terminal-wins store behavior, queued-cancellation test, executor terminal mapping | met |
| 7. Restart does not replay completed child work | non-idempotent delegated jobs use no retry; scheduler startup reconciliation terminalizes stale durable runs; scheduler restart-recovery suite passes | met |
| 8. Existing TaskTool behavior remains usable | numeric `TaskStore` alias is retained; daemon spawn response includes typed Task/Run IDs plus compatibility task/job handles; typed and numeric `get` paths are supported | met |
| 9. No daemon process/worker bypass | `check_scheduler_bypass.py`, `check_execution_ownership.py`, and core-boundary guard pass | met |
| 10. Focused and broad verification green | verification list below; quick verification passed | met |

## Schema and migration

Migration 37 adds canonical `agent_task` and `agent_run` tables with stable string primary keys, unique delegation identity, root/parent/session/workspace indexes, scheduler job/attempt links, authority and budget metadata, cancellation intent, terminal/result/failure fields, and lifecycle timestamps. `STORAGE_LAYOUT_VERSION` is 37. The migration is additive and idempotent. Legacy `task` rows are not fabricated into typed history and remain readable through the compatibility store.

## Production implementation

- Added the core durable store contract and in-memory/SQLite implementations with transition validation, relation checks, bounded fields, idempotent create/get, job/attempt attachment, cancellation intent, terminalization, and session/root lookup.
- Added `SubagentRun` as an additive scheduler payload and wired the daemon default executor to the durable run store.
- Added lifecycle transitions in `SubagentJobExecutor`, including scheduler cancellation propagation and first-terminal-wins terminal recording.
- Wired `CoreRuntimeDeps`, `TurnRunInput`, and `SessionToolContext` so production TaskTool instances receive the daemon-owned store and explicit project/workspace/session context.
- Kept the legacy TaskStore and numeric task alias for compatibility; daemon spawn now returns typed IDs immediately and typed TaskTool `get` resolves durable state first.
- Preserved semantic descendant limits while removing pool machine-capacity rejection from scheduler-owned worker requests.
- Added scheduler-side queued-cancel and startup-recovery reconciliation for durable runs.

## Verification evidence

All commands were run from the reviewed implementation tree with bounded test execution where applicable:

- `scripts/verify.sh quick` — passed.
- `cargo check --all-targets` — passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `cargo test -p codegg-core agent_run -- --test-threads=1` — 4 passed.
- `cargo test -p codegg-core migration -- --test-threads=1` — 6 passed.
- `cargo test --lib agent::worker -- --test-threads=1` — 3 passed.
- `cargo test --lib scheduler -- --test-threads=1` — 74 passed.
- `cargo test --test subagent -- --test-threads=1` — 22 passed.
- `cargo test --test scheduler_authority_matrix -- --test-threads=1` — 13 passed.
- `cargo test --test scheduler_restart_recovery -- --test-threads=1` — 15 passed.
- `cargo test --test scheduler_cancellation -- --test-threads=1` — 10 passed.
- `cargo test --test scheduler_contention -- --test-threads=1` — 14 passed.
- `cargo test --test scheduler_submission_idempotency -- --test-threads=1` — 11 passed.
- `bash scripts/check-core-boundary.sh` — passed.
- `python3 scripts/check_scheduler_bypass.py` — passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- `python3 scripts/generate_builtin_agents.py --check` — passed as part of quick verification.

## Invariant, security, and compatibility review

- Scheduler ownership remains the only daemon machine-capacity authority; no new process spawn site was added.
- Durable identity is generated typed identity, not a path, display title, numeric hash, or mutable prompt.
- Parent task relation checks require an existing task in the same session/workspace; parent run relation checks require the declared parent task/run pairing in memory-backed conformance paths.
- Authority is represented by a bounded digest of the already-resolved denied-tool/path ceiling. Secrets, hidden reasoning, full permission bodies, and unbounded output are not persisted.
- SQLite duplicate-key insertion retries the canonical lookup after a unique delegation race; terminal updates are guarded and re-read.
- Existing numeric TaskStore records remain readable and standalone execution remains explicit; no protocol field was removed.

## Failure and recovery semantics

Submission failure terminalizes the durable run as failed. Cancellation before admission cancels the scheduler job and durable run without starting the child. Running cancellation is transported by the scheduler token and mapped by the executor. Late completion cannot overwrite a terminal durable run. Startup recovery runs scheduler generation recovery first, then reconciles terminal `SubagentRun` jobs to cancelled/interrupted durable outcomes. Delegated jobs are non-idempotent/no-retry, so stale in-flight child work is not silently replayed.

## Unresolved findings

No high- or medium-severity finding remains in M001 scope. Low-severity follow-up is intentionally deferred to later roadmap milestones: durable mailbox/journal control (M002), worktree ownership and mutation isolation (M003/M004), run groups (M005), and final projection/compatibility simplification (M006). Nested durable parent IDs are represented in the core relation model; the current compatibility worker does not yet expose mailbox/worktree control, as those capabilities are explicitly out of scope for M001.

## Roadmap disposition and downstream unblocking

M001 is marked closed in the implementation plan and source roadmap. The dependency audit found that M002 and M003 now have all declared hard dependencies satisfied, so both are moved from `blocked` to `ready`. M004, M005, and M006 remain blocked on their additional mailbox/worktree/run-group dependencies. The subsystem roadmap remains active with M002 and M003 as the next parallel-ready milestones.

## Registry updates

- Added this closure record to `plans/closure/agent-run-worktree-concurrency/001-status.md`.
- Updated `plans/registry.md` to mark M001 closed and list M002/M003 as ready.
- Updated `plans/subsystems/agent-run-worktree-concurrency-roadmap.md` to mark M001 closed, M002/M003 ready, and retain the later dependency gates.
- Updated the implementation plan status to `implemented`.
