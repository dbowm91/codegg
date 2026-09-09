# Project Collaboration Roadmap

Status: active

Long-term references:

- `plans/000-long-term-specification.md#21-project-communication`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md#11-presence-and-collaboration-terms`
- `plans/002-long-term-roadmap.md#phase-12--project-communication`

Related ADRs:

- None required. Canonical direction already chooses CodeGG-owned project channels/messages over the native protocol, separate chat/audit records, and separately authorized structured actions. External chat bridges remain adapters.

## 1. Purpose and ownership boundary

This roadmap adds software-development project communication after identity/authorization, observation and audit foundations are complete. It owns durable project channels/messages/replies/mentions/read markers, ephemeral composing state, bounded synchronization, TUI chat, and structured chat actions that create ordinary separately authorized CodeGG tasks/jobs.

It does not own general-purpose social chat, authorization policy, audit storage, agent-run/job implementations, or external bridge protocols as canonical state.

## 2. Work classification

### Invariants

- Chat is project scoped and server-authorized.
- Free text never silently executes privileged work.
- Chat records and audit records are separate; chat edits/redactions cannot rewrite audit history.
- Human/agent authorship is canonical and attributable.
- Message synchronization is ordered/idempotent/bounded.
- Secrets are redacted at chat/event/reference boundaries.

### Capabilities

Project members communicate while observing work, reference CodeGG objects, and can invoke explicit structured actions that pass through ordinary authorization/run/job/audit owners.

### Infrastructure

Channel/message/read-marker persistence and incremental protocol synchronization.

### Polish

TUI chat side panel/tab, mentions/replies/composing/read ergonomics.

## 3. Non-goals

General DMs/social network, end-to-end encrypted messenger, Slack/Matrix as source of truth, arbitrary slash-command execution from message text, chat-owned task/job records, raw file/output embedding without handles.

## 4. Current state

Typed `ChannelId` exists and terminology defines ProjectChannel/ChatMessage/Mention/ReadMarker, but no canonical project communication store is active. Multi-project TUI has project tabs; observer target behavior reserves insert input for project chat. Durable agent runs/jobs/worktrees already exist. The new identity/audit and presence/observation roadmaps supply prerequisites.

## 5. Target architecture

Coordinator/daemon owns durable channel/message sequence per project plus idempotent message IDs. Messages contain bounded text and structured references to authorized objects; large content uses existing handles/artifacts. Typing/composing is ephemeral presence-like state; read markers are principal-scoped durable/lightweight state as policy chooses.

Native protocol exposes versioned namespaced channel/message sync. The TUI renders chat from this state. A structured chat action is a distinct DTO/operation with its own capability check and idempotency key; it creates an ordinary agent task/review/job and audit event linked back to the message.

## 6. Dependency graph

```text
identity-authorization-audit M005 ----+
                                      +--> M001 channel/message protocol
presence-observation M003 ------------+
                                              |
                              +---------------+---------------+
                              v                               v
                    M002 TUI project chat            M003 structured actions
```

M001 hard-depends on completed authorization/audit instrumentation and observation. M002/M003 hard-depend on M001 and may then proceed in parallel; M003 also consumes already-closed agent-run/job owners.

## 7. Milestones

### M001 — Project channel, message, and synchronization contract
Class: capability. Objective: durable authorized channels/messages/replies/mentions/references/read markers with idempotent bounded incremental sync and ephemeral composing. Exit: ordering/retry/restart/retention/auth/privacy tests and public versioned protocol.

### M002 — TUI project chat and observer input routing
Class: capability. Objective: project chat panel/tab with multi-project state, replies/mentions/references/composing/read behavior and observer-mode insert input routed to chat. Exit: no observer control mutation; reconnect/sync and project isolation pass.

### M003 — Separately authorized structured chat actions
Class: capability. Objective: explicit message-associated actions for agent task/review/job references/launches through existing authorization, run/job and audit owners. Exit: free text never executes; duplicate action is idempotent; action audit/task/run linkage is reconstructable.

## 8. Cross-cutting requirements

Storage migrations are restart-safe. Message bodies and structural metadata have explicit retention/redaction. Protocol pages/events are bounded and capability-versioned. Authorization applies to channel enumeration, messages, references and actions. External bridges, if later added, map into these contracts and never own project policy.

## 9. Verification strategy

Focused store/protocol/UI/action tests plus authorization/audit/observation regression and existing bounded repository verification. No chat-specific new CI lane.

## 10. Risks and decision points

Message references can leak unauthorized object existence; resolve/redact server-side. Typing state can become durable noise; keep ephemeral. Structured actions can become a command injection surface; require typed action DTO and explicit user operation, never text parsing.

## 11. Completion definition

M001-M003 accepted: authorized project communication is durable and synchronized; observer chat UX works; structured actions are explicit, separately authorized/idempotent/audited; chat remains distinct from audit and execution.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | closed | `plans/implementation/project-collaboration/001-project-channel-message-protocol.md` | `plans/closure/project-collaboration/001-status.md` | — (identity-authorization-audit M005 and presence-observation M003 closed) |
| M002 | ready | `plans/implementation/project-collaboration/002-tui-project-chat-and-observer-routing.md` | — | — (unblocked by M001 closure) |
| M003 | ready | `plans/implementation/project-collaboration/003-structured-chat-actions.md` | — | — (unblocked by M001 closure; authorization/audit prerequisites transitively satisfied by M001) |
