# ADR-0043: Session-Owned Subagent Lifecycle Projection

Status: **accepted** — Option A (explicitly accepted by the user on 2026-10-19)

Date: 2026-10-19
Decision owners: project maintainers
Repository review baseline: `61cbe9222c442c443cc7dfb3bd3a5b7d99ecd649`

Related specifications: `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`, `plans/000-long-term-specification.md#25-tui-target-behavior`, `plans/000-long-term-specification.md#26-reliability-and-recovery`, `plans/000-long-term-specification.md#27-security-requirements`, `plans/001-terminology-and-domain-model.md`.
Affected roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md` (M003; M004 consumes outcome).
Related accepted ADRs: ADR-0031 (idle delegated continuation), ADR-0032 (agent workstream coordination), ADR-0041 (parent-mediated subagent steer). This ADR **does not** change authorization or steering authority.

## Context

The initial turn-local `agent_tree` was treated as the authoritative live subagent list. `ProjectionReducer` drops `SubagentStarted` when `active_turn` is absent, while `run_subagent_task_with_cancel` publishes after dequeuing, before cancellation is observed. A parent may complete/cancel while its child is still queued or running, and `MAX_CHILD_TIMEOUT` can finish the parent wait without terminating the worker. C006 documented this but retained the drop and escalated the identity question (`plans/closure/tui-subagent-and-job-visibility-convergence/007-status.md`). Durable scheduler-backed children additionally have AgentRun and Job records, while legacy worker-backed children may have only a daemon-local numeric `task_id`. A new turn replaces `active_turn`, so neither refreshing `recent_turns.front()` nor reading a volatile tab fixes lifecycle ownership.

A correct UI needs session-owned liveness and stable identity independent of whichever parent turn currently occupies the snapshot. This is a compatibility and ownership decision across core events, reducer, client, and storage.

## Decision drivers

Deterministic replay; explicit originating-session and originating-turn lineage; no decimal↔UUID joins; one logical subagent not counted twice because it has run and job records; old events remain readable; bounded memory; restart semantics do not show a dead worker as running; child controls remain subject to existing daemon authorization; terminal outcome after parent exit must update the right entity.

## Alternatives

**Option A (recommended): session-owned bounded subagent lifecycle projection.** Add a session-level collection keyed by a stable child instance identity (durable `run_id` when present and a separately minted unique instance id for legacy/non-durable execution), carrying owner session/parent-turn provenance, optional `job_id`, existing display/descriptor fields and explicit terminal state. Treat `active_turn.agent_tree` as a compatible conversational/tree view, not as the only place live child identity exists. Publish id/provenance at admission, not by guessing the current active turn later. Reducer upserts/terminates session-owned instances independent of `active_turn`. Rebuild durable children from AgentRunStore/JobStore on attach; reconcile or terminalize non-durable child instances on daemon restart when the actual worker no longer exists. Preserve legacy fields and add `serde(default)`/versioned transport compatibility. This remains projection-only: no second execution owner.

**Option B: derive all subagent rows only from AgentRunStore and scheduler jobs.** Uses existing durable ids and lower new-schema cost, but drops non-scheduler/legacy workers and cannot account honestly for started, still-running workers with no durable `AgentRun`. Acceptable only if an independently verified admission migration first guarantees *all* subagents are registered durably; that is not established at this baseline.

**Option C: extend `recent_turns` or hold the last `agent_tree` in the TUI.** Low code cost, but loses descendants on subsequent turns, snapshots/restart, and old-turn eviction; muddles completed-versus-running and leaks user-facing ghosts. Reject.

## Decision

Select **Option A**, accepted explicitly on 2026-10-19. M003 must confirm the admission-origin identity can be carried on every producer path without breaking transport or adding an unbounded store. The canonical correlation key must not be the text label or the prior broken decimal-vs-UUID `task_id` join. Define an explicit union/association model: `child_instance_id` unique per dispatch, `run_id`/`job_id` optional when durable, `session_id` mandatory, `originating_turn_id` optional but immutable once known. Do not equate a UUID run id to an ephemeral numeric id. If a producer lacks a durable id, mint one once **at dispatch**, record it on all lifecycle events, and never regenerate on replay. Exact wire field naming and retention cap may be settled in M003; the authority and identity semantics may not.

## Consequences and migration

Positive: running children remain visible after parent turnover; tab/selection action keys identify stable work; reconnection and snapshot restore have a defined lifecycle. Negative: additive DTO/event fields and more reducer state; bounded retention and terminal eviction/recovery logic; projection checkpoint/migration and frontend compatibility tests. Legacy event logs lacking instance id may be displayed as historical turn-local data when unambiguous, but MUST NOT be reclassified as a live durable child on restart. Schema additive/version negotiation first; no destructive migration of old conversations. Preserve the existing name-pool lease semantics and ADR-0041 Option A, including refusal when no controlling parent turn exists.

## Security and reliability

All new records are session-scoped; maintain authorized project/session stream filters and no cross-session fallback. Validate every event's claimed owner against trusted admission context; never accept an untrusted model-supplied session id as proof of authorization. Apply bounded aggregate size and string limits. Ignore stale terminal events for a different child generation. On cancel and crash, reconcile from stores/worker-lifetime evidence, rather than assuming a just-seen start means still running. Render missing/unavailable state distinctly from an authoritative empty list.

## Verification demanded before acceptance and closure

Before accepting: inventory all subagent spawn modes (scheduler-backed durable, legacy worker, group/parallel), event source lineage, restart behavior, producer assignment of ids, replay and transport surfaces, and caps. M003 must test started-after-parent-terminal, repeated numeric ids, timeout/cancel ordering, restart of an ephemeral child, resume from cursor, duplicates and cross-session spoof attempts; replay a recorded log twice to byte-equivalent bounded projections and prove no ghost running. Include socket TUI and desktop/WS compatibility assertions where DTOs change. Do not mark ADR accepted by implementing Option A silently.

## Supersession

None. Recheck ADR number 0042 against `main` and active branches before integration; numbers are never reused.
