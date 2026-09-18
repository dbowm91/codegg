# ADR-0007: Shared Session Control Ownership and Transfer

Status: accepted

Date: 2026-09-18

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`

Affected subsystem roadmap:

- `plans/subsystems/team-collaboration-corrective-addendum.md`

Related ADRs:

- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`

## Context

CodeGG serializes turns per session, but today any project principal with `agent.invoke` can submit the next turn and can also steer or cancel the currently active turn. The active-turn lock prevents concurrent turns; it does not establish which human controls the in-flight turn. Permission/question response surfaces have similarly lacked a human-controller concept.

Read-only observation already forbids control in the TUI. Team deployments need the complementary positive rule: when a session is writable by several contributors, control of an active turn must be explicit rather than implicitly shared.

## Decision drivers

- Observing and chatting must never imply agent control.
- A teammate opening a writable session must not accidentally cancel or steer another human's active turn.
- Normal collaboration should not require permanently locking a session to one owner.
- The control model must survive reconnects and be attributable in audit.
- Takeover must be explicit and policy-gated; loss of a frontend must not silently transfer authority.

## Considered options

### A. Any `agent.invoke` member may control any active turn

This is the current behavior. It is simple but unsafe for multi-user shared sessions. Rejected.

### B. Permanently own each session by its creator

This prevents accidental interference but makes handoff and team continuity unnecessarily rigid. Rejected.

### C. Turn-scoped controller lease with explicit transfer/takeover

This protects in-flight work while leaving idle sessions collaborative. Accepted.

## Decision

An idle shared session has no exclusive human controller. A principal with the ordinary project/session authority may submit the next turn. Successful `TurnSubmit` atomically establishes a controller lease for that active turn, attributed to the submitting principal and originating client.

While the turn is active, the controller principal is required in addition to existing semantic capabilities for:

- turn steering;
- turn cancellation;
- answering permissions owned by that session/turn;
- answering questions owned by that session/turn;
- other future operations explicitly classified as in-flight human control.

Other authorized project members may observe, chat, and request control, but they cannot mutate the active turn merely because they possess `agent.invoke`.

The lease is keyed by canonical session and active turn identity, revisioned, and daemon-owned. It clears when the turn reaches a terminal state. The same principal may resume control from another authenticated client; client identity remains audit/presence metadata rather than a second authorization identity.

Control transfer is explicit:

- a non-controller Contributor+ may submit a control request/suggestion;
- the current controller may transfer control to another active project member with sufficient underlying authority;
- Maintainer/Owner may perform an explicit forced takeover with a bounded reason when recovery is necessary;
- takeover/transfer never grants capabilities the recipient does not already possess.

A disconnect does not silently hand control to another principal. The lease remains through a bounded reconnect grace. If the controller never returns, an eligible Maintainer/Owner uses the explicit takeover operation. Ordinary Contributors may request but not force takeover.

## Persistence and recovery

Control records should be durable enough to survive transport reconnect and daemon recovery: session id, turn id, controller principal, originating client, revision, timestamps, transfer/takeover provenance, and terminal/released state. The daemon reconciles the record against canonical active-turn state on startup. A stale lease for a terminal or nonexistent turn is released; an active turn with ambiguous controller provenance fails control closed until an explicit recovery takeover.

## Authorization relationship

The controller lease narrows authority; it never widens it. Existing project capability checks run first. The effective authority for an in-flight control operation is therefore `project/session capability AND controller lease`, with explicit Maintainer/Owner takeover as a separate audited operation.

Observer mode remains hard read-only. Chat access under ADR-0006 is orthogonal and cannot satisfy the controller predicate.

## Consequences

Positive: shared sessions become safe to inspect collaboratively; active-turn mutations have one accountable human controller; reconnect and takeover behavior are explicit.

Negative: turn control gains a small state machine and additional recovery tests. Some formerly accepted cross-user steer/cancel calls will now be denied until transfer/takeover.

Neutral: parallel development should still use separate sessions/worktrees. This ADR is not a collaborative text editor and does not make one session support concurrent turns.

## Compatibility and migration

Solo/LocalOwner behavior is unchanged in effect: the submitting principal immediately controls its turn. Existing inactive sessions require no rows. Existing active turns encountered during upgrade must derive controller identity only from trustworthy stored origin attribution; otherwise control fails closed and exposes recovery takeover to Maintainer/Owner.

## Security and reliability implications

Control-request payloads are bounded inert metadata and never execute work. Transfer/takeover is audited. Revoked/suspended principals immediately lose controller effectiveness even if a lease row remains. Permission/question lookup must resolve the owning session/turn before controller evaluation; opaque IDs cannot be used to bypass it.

## Verification

Required evidence includes two-principal steer/cancel denial, same-principal reconnect, explicit transfer, forced takeover authorization, controller revocation, turn-terminal release, restart reconciliation, permission/question gating, observer/chat non-escalation, and races between transfer, cancellation, and turn completion.

## Supersession

A future richer collaborative-control system may supersede this ADR only if it preserves explicit ownership, attribution, fail-closed recovery, and capability intersection.
