# TUI Live Work Visibility — Runtime Integration Corrective Roadmap

Status: conditionally closed (2026-10-20; M001–M005 closed, M006 conditionally closed pending multi-worker timeout/restart and operator qualification)

Repository review baseline: `ea5dab90424ba95312310ac7b1072be0f1323742` on `main` (2026-10-20). Originating user reproduction: real sessions with waiting jobs and subagents show neither in `/sessions` nor `Ctrl+T` after commit `8fab1ba7d48d794b397ce4a0a207d5ae547ff976`.

Long-term: `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`, `plans/000-long-term-specification.md#25-tui-target-behavior`, `plans/000-long-term-specification.md#26-reliability-and-recovery`, `plans/000-long-term-specification.md#27-security-requirements`, `plans/000-long-term-specification.md#28-observability`, `plans/001-terminology-and-domain-model.md`; governance `plans/003-planning-process.md`.

Predecessors (retain their history; do not retroactively change closure claims): `plans/subsystems/tui-subagent-and-job-visibility-roadmap.md`; `plans/subsystems/tui-subagent-and-job-visibility-convergence-corrective-addendum.md`; `plans/closure/tui-subagent-and-job-visibility-convergence/001-status.md` through `007-status.md`. The C006 closure explicitly escalates a **still-broken orphan-subagent** path. Related accepted decisions: ADR-0031 (idle completion), ADR-0035/0036/0037/0038 (prompt/turn authority), ADR-0041 (human-to-subagent Option A). Proposed decision: `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` (do not implement M003's schema/identity changes until accepted).

## 1. Purpose and ownership

Deliver a truthful, interactive view of **real outstanding work** in the default daemon-backed TUI, not just render synthetic `SubagentRow` and `JobProjection` fixtures. The user must find ongoing work at `/sessions` and `Ctrl+T`, inspect it, and use authorized controls. The daemon/scheduler remains authority for execution and identity; the frontend owns only per-session subscription lifecycle, display derivation, navigation, and user intent dispatch.

Core dependencies: session projection/replay, session authorization, scheduler job store, AgentRunStore, tool-program lifecycle, runtime subagent worker, and existing TUI focus/render plumbing. No parallel work database, competing event bus, hidden timer-based truth source, or rewritten execution scheduler.

## 2. Work classification

- **Invariant:** a displayed live row has an authoritative session-bound identity and nonterminal state; an authoritative empty result differs from disconnected/unsubscribed/unauthorized/loading; no cross-session disclosure or stale-generation repaint; no synthetic progress or ghost-running records after restart.
- **Capability:** both user entry points show real in-flight subagents/jobs, remain current during execution and after parent-turn completion, and support inspection/allowed actions.
- **Infrastructure:** active TUI projection subscription and resume; job event/snapshot convergence; session-owned subagent lifecycle for outliving-turn children; stable cross-type work identity and de-duplication.
- **Polish:** compact readable status/diagnostics/help, minimal unnecessary redraws, consistent row actions.

## 3. Evidence and unclosed findings

1. Default `src/main.rs` calls `App::attach_core_client` / raw `subscribe()`, not a normal-session `ProjectionSubscribe`. `src/tui/commands/observe.rs` subscribes only for explicit observation; `handle_remote_event` installs snapshots for remote `/tui` attach, but ordinary `/core` TUI lacks a corresponding production installation. Search production calls before editing; a reference found elsewhere may change the diagnosis. `App::apply_core_event` can consume `ProjectionStreamEvent`, but only after a subscription exists.
2. `App::task_rows()` maps `active_snapshot().jobs` and `ui_state.subagents.rows`; absent projection silently appears empty. `/sessions` delegates to the same builder. The synthetic tests `tests/tui_sessions_pending_sections.rs` and `tests/tui_main_buffer.rs` manually install seeded snapshots and never establish a real daemon subscription.
3. `src/tool/task.rs` submits delegated `JobKind::Subagent` through `JobSubmissionService::submit` directly; the located `JobCreated` publication is in the `CoreRequest::JobSubmit` handler (`src/core/daemon_jobs.rs`). Trace scheduler-wide creation/transition publications and provide an authoritative job event path for **all** admission routes. In `crates/codegg-core/src/projection_replay/publication.rs`, `JobStarted/Completed/Failed` currently produce unscoped/empty-session upserts. Existing `BackgroundJobCompleted` is terminal-only; cannot establish in-flight liveness.
4. `src/core/daemon_refresh.rs::projection_snapshot_for_session` currently reconstructs agent runs/worktrees/groups but not the parent `active_turn.agent_tree` or outstanding `snapshot.jobs`; late subscribers cannot rely on an initial complete live-work view. `snapshot.agent_runs` does not currently feed the `Ctrl+T`/`/sessions` builder.
5. `ProjectionReducer::SubagentStarted` drops `orphan_subagent_started` after parent turn exit. C006 explicitly proved real races (queued cancellation and child timeout); no session-owned projection fixes them.
6. Job rows are displayed but not actionable: `SessionDialog::activate_selected` and `state::tasks::activatable_target` return no target for jobs. ADR-0041 permits parent-turn addressed steering, **not** arbitrary direct child steering.

These are source-level findings, not a completed live runtime reproduction on this review baseline. The user has reported independent real-session reproduction. M001 and M006 must capture falsifying as well as confirming evidence.

## 4. Target architecture

```text
real scheduler/worker/AgentRunStore
  -> daemon-owned, owner-scoped durable lifecycle + initial snapshot/replay
  -> per-active-session TUI subscription (generation-fenced, resumed on reconnect)
  -> canonical bounded session-work row selector, keyed by stable ids
  -> /sessions + Ctrl+T + sidebar + inspect/action views
```

Truth is sourced from authorized projected state, not process-local `GlobalEventBus` in the socket client. A late attach gets the same outstanding rows as an attached client; terminal events resolve rows and controls deterministically. `AgentRun`, `Job`, `ToolProgram`, and transient child projection are related **entities**, not necessarily four separate work items. De-duplicate by explicit durable relationships (`run_id`, `job_id`, `program_id` and canonical child instance identity), never approximate labels or decimal/UUID string joins. Job and child status remain separately inspectable when needed.

## 5. Dependency graph

```text
M001 production subscription / unavailable-vs-empty ────┐
M002 scheduler job publication + late-attach rebuild ───┤
ADR-0043 accepted -> M003 child identity & lifetime ───┤
                                                        +--> M004 one work model
                                                        +--> M005 inspect/control UX
                                                        +--> M006 real transport and TUI acceptance
```

M001 and M002 are closed. ADR-0043 Option A was accepted and M003 is closed. M004 and M005 are closed in dependency order. M006 is conditionally closed under its explicit stop rule: the socket-to-painted-TUI slice passes, while real-worker concurrency, late-attach/restart recovery, the full denial matrix, and the manual terminal receipt remain unqualified. These are re-entry conditions for full capability acceptance, not blockers on another registered plan in this line.

## 6. Milestones

| Milestone | Class | Status | Bounded deliverable |
|---|---|---|---|
| M001 | infrastructure | **closed** | Default `/core` TUI subscribes for its bound session, handles connection errors honestly, and installs/drains real projection envelopes; reconnect takes a fresh authoritative snapshot. Closure: `plans/closure/tui-live-work-visibility-corrective/001-status.md`. |
| M002 | invariant | **closed** | Scheduler admission and lifecycle updates produce session-scoped projected jobs; late snapshots contain bounded outstanding jobs. Closure: `plans/closure/tui-live-work-visibility-corrective/002-status.md`. |
| M003 | invariant | **closed** | Session-owned bounded child lifecycle and generation-safe identity; ADR-0043 Option A accepted. Closure: `plans/closure/tui-live-work-visibility-corrective/003-status.md`. |
| M004 | capability | **closed** | One ID-based session-work selector feeds `/sessions`, `Ctrl+T`, and sidebar; socket/event publication reaches both painted work surfaces. Closure: `plans/closure/tui-live-work-visibility-corrective/004-status.md`. |
| M005 | capability | **closed** | Identity-keyed job detail and daemon-authorized cancel; observer cancel disabled; parent-mediated child interaction preserved. Closure: `plans/closure/tui-live-work-visibility-corrective/005-status.md`. |
| M006 | capability | **conditionally closed** | Socket-to-painted-TUI/job cancel slice passes, including one real held worker on a late socket-attached TUI. Two-worker concurrency, actual parent timeout/stop, in-flight restart, complete denial matrix, and manual terminal receipt remain required for full acceptance. Closure: `plans/closure/tui-live-work-visibility-corrective/006-status.md`. |

Plans: `plans/implementation/tui-live-work-visibility-corrective/001-...` through `006-...`. Each milestone has a self-contained implementation plan; open only one primary milestone per agent.

## 7. Cross-cutting requirements

**Authorization:** session subscription must bind authenticated project/session identity, reuse established observation/control checks and exact stream ownership. A session's work must not be exposed through another session or a global broadcaster. Read-only observers cannot gain cancel/steer powers. Preserve ADR-0041 Option A; do not send directly to a child without a successor decision.

**Storage/protocol:** prefer current job/agent-run stores and additive projection schema. Any new child identity, scope, DTO or persisted state requires ADR-0043 and an explicit migration/compatibility decision. Legacy events with no identity use clearly defined read-only fallback; never fabricate durable identities.

**Failure/races:** handle subscribe response-before-events, switch while request in flight, reconnect with cursor gaps, daemon restart, rapid create→start→finish, parent cancellation, child outliving turn, duplicate/out-of-order event, bounded retention, resource starvation, and unreadable/unauthorized session. Terminal and missing states must not be conflated.

**Performance:** bounded rows, snapshot rebuild, per-frame overhead and event fanout; no hot-path DB query per paint or periodic polling when stream events suffice. No SBC startup/compile benchmark.

## 8. Verification / close definition

Every milestone supplies focused unit/integration/security tests, explicit fail-before/pass-after regression and exact-head CI/verify receipts. M006 must drive a real daemon/socket/authorization stack with deterministic worker/provider fixtures, then the actual `App::on_key`/slash command and painted dialog/overlay. Demonstrate two concurrently running children, one separate background job, late attachment, cross-session isolation, cancel/timeout, completion and reconnect; prove visible row identity and successful inspect/control. A manual TUI run on a development workstation is an **additional** qualification, not a substitute for automated gates. No claim of closure from seeded `TestBackend` rows alone.

Run `scripts/verify.sh quick`, supported-feature `cargo clippy --workspace --all-targets --locked -- -D warnings`, applicable focused tests, projection/transport/ownership guards and `git diff --check`. Use `cargo test -p codegg --test <target> --locked` for new integration targets. Do not use `--all-features` or mandate a live-model CI, VM, or SBC hardware job.

The subsystem closes only after the **same real work** is visible and actionable through both entry points in a normal attached session, a late-attached session and after recovery, with no false-empty claims or unresolved high-severity lifecycle losses. Each milestone requires `plans/closure/tui-live-work-visibility-corrective/NNN-status.md` with commit hashes, requirement→test matrix, exact commands/results, transport/authorization negatives, cancellation/restart evidence, residuals and the observed binary/commit.

## 9. Explicit milestone handoff and closure status

| Milestone | Status | Implementation plan | Closure record |
|---|---|---|---|
| M001 | **closed** | `plans/implementation/tui-live-work-visibility-corrective/001-default-tui-session-projection-lifecycle.md` | `plans/closure/tui-live-work-visibility-corrective/001-status.md` |
| M002 | **closed** | `plans/implementation/tui-live-work-visibility-corrective/002-scheduler-job-publication-and-snapshot-recovery.md` | `plans/closure/tui-live-work-visibility-corrective/002-status.md` |
| M003 | **closed** | `plans/implementation/tui-live-work-visibility-corrective/003-session-owned-subagent-lifecycle.md` | `plans/closure/tui-live-work-visibility-corrective/003-status.md` |
| M004 | **closed** | `plans/implementation/tui-live-work-visibility-corrective/004-unified-session-work-model-and-visibility.md` | `plans/closure/tui-live-work-visibility-corrective/004-status.md` |
| M005 | **closed** | `plans/implementation/tui-live-work-visibility-corrective/005-job-and-subagent-inspection-controls.md` | `plans/closure/tui-live-work-visibility-corrective/005-status.md` |
| M006 | **conditionally closed** | `plans/implementation/tui-live-work-visibility-corrective/006-real-daemon-to-tui-acceptance-and-recovery.md` | `plans/closure/tui-live-work-visibility-corrective/006-status.md` |

Historical convergence closures remain under `plans/closure/tui-subagent-and-job-visibility-convergence/`; do not overwrite them. Add successor closure records only after the corresponding real integration evidence is available.
