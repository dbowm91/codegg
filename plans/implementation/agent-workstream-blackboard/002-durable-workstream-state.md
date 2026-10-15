# Agent Workstream Blackboard M002 — Durable Bounded Workstream State and Read-Only Projection

Status: active — read-only groundwork; follows M001 closure

Planning baseline: de49884f3fdc6d528620f2a2645694b9b19fc087 (2026-10-15)
Source: plans/subsystems/agent-workstream-blackboard-roadmap.md M002
Accepted ADRs: ADR-0001, ADR-0002, ADR-0004; proposed ADR-0032 is design input only, NOT shared-write authority.
Canonical: plans/000-long-term-specification.md §§6, 16–17, 26–29; plans/001-terminology-and-domain-model.md §§7, 12–13.
Primary class: infrastructure; hard dependency: closed agent-run-worktree M009.

## Objective

Expose one authorized bounded workstream view over already durable AgentRunGroup, AgentRun, AgentTask, Job, Worktree and AgentRunResult records. Distinguish observed daemon facts from agent-authored intent. This milestone is a complete read-only consumer of canonical lifecycle state: no new run or worktree allocation, mutation claims, cross-agent tool, new scheduler or periodic polling.

## Baseline evidence

src/tool/task.rs exposes spawn_many/status_group; crates/codegg-core/src/agent_run_group.rs implements group membership/recomputation; crates/codegg-core/src/agent_run_control.rs and src/agent/run_control.rs persist and deliver control; crates/codegg-core/src/projection_replay/agent_runs.rs has bounded summaries; src/scheduler/executors.rs publishes run/worktree updates. Session projection is derived. Existing group status does not provide a consolidated task-ownership/peer-action view.

## Ownership and contracts

A workstream is a view of an existing group identity (AgentRunGroupId with explicit group owner). Do NOT add WorkstreamId unless a demonstrable lifecycle constraint prevents group identity reuse. Define a typed WorkstreamSnapshot with group ID, project/repository/workspace IDs, owner, monotonic revision/cursor, member IDs, roles/descriptions redacted as needed, phase/status, worktree identity, factual result/commit references, bounded blockers count, and source-class tags for derived vs declared fields.

Derive lifecycle, Git/worktree, and job facts from existing stores. Do not mirror those facts in another authoritative table. Where a monotonic cursor is needed, use the existing event journal/sequence or an additive minimal sequencing table with transactional event identity; document the consistency model (snapshot-at-read, stable revisions, changes-since). Never synthesize false global linearizability across separate stores.

## Work packages

A. Contract tests. Identify all durable group/run status transitions, provider notifications, cancelled and orphan worktree states, group owner kinds (Turn vs Run), nested child scopes and existing projection authorization. Define required versus optional snapshot fields, state precedence and loss-of-data behavior.

B. Pure projector and bounded store adapter. Implement a core-owned (UI-free, service-neutral) typed projection reducer and a daemon adapter that queries canonical stores, caps members at configured group bound (currently 16), byte-caps summaries and provides stable order. Avoid transcript or whole tool-result materialization. A workstream member may have no checkout or commit; represent absence accurately.

C. Authorization/transport. Implement narrowly scoped read request(s) or reuse native projection service (prefer smallest compatible extension). Use principal/project and group-owner authorization; identifiers alone convey no access. For single-user localhost deployment retain no additional setup. Frontends may observe but cannot mint ownership.

D. Replay and lifecycle. Snapshot/reconnect/restart and completion-before-subscription must converge; deduplicate by logical revision/event ID; never trigger model continuation as a side effect of read. Group termination must remain observable even if original turn ends. No separate always-running blackboard worker.

## Required tests

Two projects with colliding names cannot see each other's group state; same-project unauthorized principal fails. Root-turn-owned and nested-run-owned group trees stay distinct. Membership reordering and large descriptions cannot exceed declared bounds. Resume from sequence N returns only correct changes; nonterminal→terminal after reconnect converges without double delivery; duplicate events do not rewind revision. Missing job or worktree yields an explicit incomplete/fact-unavailable field, not invented status. No hidden reasoning, prompts, credentials, raw tool args, private paths or full logs leak.

Commands: focused cargo test -p codegg-core agent_run_group, agent_run, projection_replay; cargo test --lib agent::run_control; cargo test --test session_projection_consumer; cargo test --test scheduler_restart_recovery; bash scripts/check-core-boundary.sh; project authorization/projection guards relevant to changed files; cargo fmt --all -- --check; scripts/verify.sh quick and workspace Clippy. Record actual results.

## Migration/compatibility, acceptance and stop

Favor no migration for read-only derived state. If required, schema must be additive/backfill-safe with bounded indexes and nullable compatibility. Existing group APIs and default task spawn unchanged. Stop if this requires a second group/run status store, unbounded serialized snapshot, new busy-loop or cross-project information disclosure. Acceptance is an operationally consumable authenticated read surface with tested restart semantics; type definitions alone are insufficient.

## Closure

Write plans/closure/agent-workstream-blackboard/002-status.md with source/test/projection/security evidence and unblocking: M003 becomes ready only after strict M002 closure; M001 remains independently eligible.
