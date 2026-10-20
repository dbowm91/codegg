# TUI Live Work Visibility M003 — Session-Owned Subagent Identity and Lifetime

Status: **closed** — closure: `plans/closure/tui-live-work-visibility-corrective/003-status.md`
Primary class: invariant
Repository review baseline: `ea5dab90424ba95312310ac7b1072be0f1323742` (`main`, 2026-10-20)
Source roadmap: `plans/subsystems/tui-live-work-visibility-corrective-roadmap.md` M003.
Requirements: `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`, `#26-reliability-and-recovery`, `#28-observability`; `plans/001-terminology-and-domain-model.md`.
Decision gate: `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` (proposed Option A). Related accepted ADR-0041 preserves **parent-mediated** steering.
Corrective origin: C006's **unfixed** residual in `plans/closure/tui-subagent-and-job-visibility-convergence/007-status.md`; prior C004/C005 field propagation tests did not exercise execution after a parent turn exited.

## 1. Objective, dependency and scope

Ensure every real subagent has session-owned identity and lifecycle state independent of `active_turn`, so it remains visible across parent timeout/completion, turn replacement and reconnection and terminates correctly after cancel/failure/restart. Do **not** implement durable DTO changes or redefine child identity until ADR-0043 is accepted; test-first source inventory and reproduction may be done before acceptance. M001 and M002 are soft integration dependencies for eventual UI proof.

## 2. Current evidence and constraints

`src/agent/worker.rs` publishes `AppEvent::SubagentStarted` at entry before checking cancellation. `specialized_runtime.rs` has `MAX_CHILD_TIMEOUT` that can outlive the parent's await. `crates/codegg-protocol/src/projection/reducer.rs::SubagentStarted` currently requires `snapshot.active_turn` and drops the event with `orphan_subagent_started`; progress/terminal arms similarly require a matching active tree node. `src/tui/app/render.rs::sync_subagents_from_snapshot` takes `active_turn` or one `recent_turns.front()`, losing older live nodes on next turn. Worker `task_id` is numeric; durable `AgentRunId` is a UUID, and the predecessor's attempted string join was impossible. The previous proposed workaround (keeping the most recent row) is not authoritative state.

## 3. Required invariant / non-goals

- Mint/assign each child instance's stable identity at trusted dispatch and carry once through start/progress/terminal/replay; never infer from name, task text, `active_turn` at publication or a decimal-vs-UUID conversion.
- Bind to exact canonical session, immutable originating-turn lineage, optional durable `run_id`/`job_id`; duplicate delivery must upsert one child. Avoid unbounded growth; distinguish ephemeral read-only historical events from authoritative live instances.
- A completed parent may still have a live child, but a crashed ephemeral worker must not appear live after restart. Cancellation and timeout have explicit outcomes; a missing terminal is not silently a success.
- Out: child admission/sandbox/worktree policy changes, scheduler replacement, durable display-name persistence, per-child direct TurnSteer (prohibited by ADR-0041), new model orchestration logic.

## 4. Ordered work packages

A. **Inventory and failing reproductions.** Enumerate worker, scheduler-backed `SubagentRun`, group/fan-out and other publish paths; record `session_id`, origin `turn_id`, numeric task id, run/job links and underlying cancellation token lifetime. Produce red tests for delayed start after cancellation, timeouts, new parent turn while child runs, two children sharing numeric id in different generations, delayed terminal after parent exit and daemon restart. Stop here if ADR remains proposed.

B. **After ADR acceptance: trusted identity.** Thread the selected child instance id from admission through `SubAgentRequest`, `AppEvent`, `CoreEvent`, `ProjectionEvent` and `AgentTreeNodeProjection` or additive session-owned DTO, with optional/defaulted fields for old event decoding. Do not inject synthetic origin turn ids at bridge time after the parent already exited.

C. **Session-owned reducer projection.** Add bounded session-scoped child upsert/progress/terminal logic independent of active turn, with correct owner and generation, deterministic duplicate/replay behavior, explicit terminal/recovery disposition and cap accounting. Keep existing per-turn `agent_tree` as compatible historical/conversational view. Define exact semantics of child completion that arrives after session turn transition, and missing old-wire correlation (reject or mark unavailable; do not attach to another child).

D. **Authoritative snapshot reconstruction.** Rebuild durable child records from AgentRunStore/scheduler, correlate via actual `run_id`/`job_id` links; reconcile ephemeral instances after daemon restart without ghost-running. Expose bounded missing/overflow diagnostics and retain enough terminal identity for tabs to detach safely. Do not store a shadow execution engine.

## 5. Failure, security, concurrency, recovery

Test cross-session and wrong-turn lifecycle events, current-turn vs old-turn race, parent cancellation + worker dequeue, worker finishing during new-turn start, child budget timeout, reconnect mid-stream, duplicate/adversarial/out-of-order events and session restart. Session scoping and trusted origin must be enforced in daemon publication and reduction; do not rely on user-visible label/name as access control. Preserve the parent execution ceiling and allow observers only read-only state. Count and tab status must converge to zero only after canonical terminal or explicitly reconciled interruption.

## 6. Compatibility, migration and tests

Propose additive `#[serde(default)]` fields and versioned controller snapshot compatibility, respecting `MAX_PROJECTION_SUBAGENTS` and independent session-level cap. Replay old logs twice to identical result; no cold-start resurrection of an orphan. If durable instance persistence requires new table/index, supply migration/schema bump, rollback and restart evidence before proceeding. Existing `/observe` and remote `/tui` clients must decode old/new payloads.

Focused tests: worker publish ordering, core app→event mapping, projection reducer, replay publisher and serde, integrated scheduler-backed plus legacy worker late-attach; with **real event provenance**, not manual injected `SubagentRow`. Use `cargo test -p codegg-protocol --lib projection --locked`, focused `cargo test -p codegg --lib subagent --locked`, transport guard scripts, ownership/scheduler guards, fmt, clippy and `scripts/verify.sh quick`.

## 7. Acceptance and stop / closure

Pass: a child started after its parent turn ended is still in the correct session's authoritative projection (or ends explicitly cancelled without ghost status); progress/terminal correctly updates the same child; late/reconnected TUI can render it; retry/numeric id reuse never misaddresses a row; restart never invents live status. Stop on unaccepted ADR, unclear immutable id source, privilege changes or migration not reviewed. Closure `plans/closure/tui-live-work-visibility-corrective/003-status.md` must include ADR acceptance link, exact event/identity contract, fail-before/pass-after races, replay/restart/transport evidence, project/session authorization negatives and exact commands/commit.
