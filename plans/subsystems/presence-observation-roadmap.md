# Presence and Read-Only Observation Roadmap

Status: closed

All milestones M001–M003 are accepted; the subsystem is complete per §11.
Closure records: M001 (`plans/closure/presence-observation/001-status.md`),
M002 (`plans/closure/presence-observation/002-status.md`),
M003 (`plans/closure/presence-observation/003-status.md`).

Long-term references:

- `plans/000-long-term-specification.md#14-presence-and-real-time-team-awareness`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/001-terminology-and-domain-model.md#11-presence-and-collaboration-terms`
- `plans/002-long-term-roadmap.md#phase-7--presence-and-collaborator-awareness`
- `plans/002-long-term-roadmap.md#phase-8--read-only-observation-mode`

Related ADRs:

- None required. The canonical specification already fixes presence as ephemeral project-scoped state and observation as a read-only frontend-neutral projection subscription. Any proposal for raw terminal mirroring or shared implicit control requires a new decision and is out of scope.

## 1. Purpose and ownership boundary

This roadmap turns the already-closed frontend-neutral projection/replay foundation into authorized collaborator awareness and read-only session observation. It owns ephemeral presence leases, bounded collaborator summaries, TUI presence affordances, observer-mode authorization and read-only UX.

It consumes canonical principals/project authorization from `identity-authorization-audit`, and canonical session projections/replay from the closed session-projections subsystem. It does not own project chat persistence, authorization policy itself, raw terminal sharing, or control transfer.

## 2. Work classification

### Invariants

- Presence is ephemeral and never becomes durable session history or audit truth.
- Unauthorized principals cannot infer project existence, collaborators, sessions, or activity.
- Observation uses canonical projection snapshot/replay/live semantics, not terminal screen frames.
- Observer mode cannot answer permissions/questions, steer, cancel, mutate files/worktrees, or otherwise acquire session-control authority.
- Multiple clients for one principal and several sessions per principal remain distinct.

### Capabilities

Authorized members see bounded collaborator/activity state and can follow another authorized session read-only with reconnect/replay.

### Infrastructure

Project-scoped renewable presence leases and privacy-filtered collaborator snapshot/events.

### Polish

TUI collaborator panel, status labels, observer-mode focus/input behavior.

## 3. Non-goals

Project chat storage/actions, implicit shared-control editing, raw PTY/screen sharing, hidden chain-of-thought exposure, durable presence history, global social presence, distributed node presence before node topology exists.

## 4. Current state

Session projections/replay M012 is closed and already provides bounded versioned snapshot/event streams, sequence/replay/resync, visibility/redaction, artifact handles, connection-local subscription ownership and real transport lifecycle. Its access context contains semantic observe capabilities and a project resolver but intentionally does not own team authorization. `ClientRegistry` tracks connection and attached-session metadata but not canonical principal presence. TUI multi-project/session state is closed; final team presence/observation product work was deferred.

## 5. Target architecture

The daemon owns an in-memory/ephemeral presence service keyed by canonical `PrincipalId` + `ProjectId` with connection/session activity contributions and bounded lease expiry. Durable membership/authorization decides visibility; presence never grants authority. Frontends consume privacy-filtered project presence snapshots/events.

Observation is an authorized specialization of the existing projection subscription path. The caller requests a session locator; daemon authorization verifies `session.observe`; projection/replay policy applies existing redaction and artifact bounds. The TUI renders the same logical activity model in an explicitly read-only state.

## 6. Dependency graph

```text
identity-authorization-audit M003
              |
              v
M001 presence leases
       |
       v
M002 TUI collaborator surface
       |
       +----------------+
       |                |
       v                |
M003 authorized observation <--- closed session-projections M012
```

M001 hard-depends on authorization M003. M002 hard-depends on M001. M003 hard-depends on M001 and M002 for the complete product slice and has an already-satisfied interface dependency on session projections.

## 7. Milestones

### M001 — Project-scoped presence leases

Class: infrastructure

Objective: represent authorized principal/session activity as bounded renewable ephemeral state with heartbeat, idle, disconnect, reconnect and privacy semantics.

Exit conditions: no permanent stale presence; high churn bounded; several clients/sessions aggregate correctly; restart clears/rebuilds ephemeral state; unauthorized queries reveal nothing.

### M002 — TUI collaborator and presence surface

Class: capability

Objective: expose authorized collaborator summaries in project tabs/panel without coupling the TUI to presence ownership.

Exit conditions: project-scoped panel/header shows bounded human/session/agent activity; stale/expired entries converge; multi-project routing is correct; no hidden project leakage.

### M003 — Authorized read-only observation

Class: capability

Objective: let an authorized member select and follow another session through existing projection replay while all ordinary observer input remains non-mutating.

Exit conditions: allow/deny matrix passes; reconnect/replay works; secret redaction survives snapshot/replay/live; steering/cancel/permission/question/file mutations are rejected; TUI visibly remains read-only.

## 8. Cross-cutting requirements

Presence state should be memory bounded and need no durable schema beyond optional settings. Protocol additions are versioned and bounded. Authorization always precedes presence/observation lookup. Disconnect/reconnect and idle transitions must avoid timer/task leaks. Observation must reuse existing queue/replay backpressure rather than create a second streaming stack.

## 9. Verification strategy

Focused lease/privacy/UI/observation tests plus existing session projection real-transport suites and normal `fmt`/`clippy`/`scripts/verify.sh quick`. No new CI lanes.

## 10. Risks and decision points

Presence can accidentally become an authorization cache; prohibit it. Observation can accidentally add a parallel stream; reuse projection subscriptions. Agent activity summaries may be expensive; aggregate bounded projections instead of scanning unbounded histories.

## 11. Completion definition

M001-M003 accepted: authorized teammates see current bounded presence and can observe another session read-only using canonical projections; unauthorized callers learn nothing; presence remains ephemeral; no control authority leaks.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/presence-observation/001-project-presence-leases.md` | `plans/closure/presence-observation/001-status.md` | — |
| M002 | closed | `plans/implementation/presence-observation/002-tui-collaborator-presence-surface.md` | `plans/closure/presence-observation/002-status.md` | — |
| M003 | closed | `plans/implementation/presence-observation/003-authorized-read-only-observation.md` | `plans/closure/presence-observation/003-status.md` | — |
