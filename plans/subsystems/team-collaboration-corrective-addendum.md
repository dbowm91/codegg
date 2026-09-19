# Team Collaboration, Shared Sessions, and Workspace Chat — Corrective Addendum

Status: active

Repository baseline reviewed: `4e12ecc192ba7e2192d3a62acce6e15fc1bb1153`

Related closed work:

- `plans/subsystems/identity-authorization-audit-roadmap.md`
- `plans/subsystems/presence-observation-roadmap.md`
- `plans/subsystems/project-collaboration-roadmap.md`
- `plans/subsystems/project-work-orders-task-view-roadmap.md`
- `plans/closure/identity-authorization-audit/001-status.md` through `005-status.md`
- `plans/closure/presence-observation/001-status.md` through `003-status.md`
- `plans/closure/project-collaboration/001-status.md` through `003-status.md`
- `plans/closure/project-work-orders-task-view/004-status.md`

Long-term references:

- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#14-presence-and-real-time-team-awareness`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#21-project-communication`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#27-security-requirements`

Related ADRs:

- `plans/adrs/ADR-0006-project-channel-chat-access-policy.md`
- `plans/adrs/ADR-0007-shared-session-control-ownership.md`

## 1. Purpose and corrective trigger

The closed identity, observation, collaboration, and Workspace milestones provide most of the machinery required for teams, but review of current production code found five gaps that prevent the capability from being treated as a coherent multi-user product:

1. Network REST/SSE compatibility routes authenticate callers but generally bypass the daemon's project capability boundary and direct-read/write stores or the global event bus.
2. Chat is statically bundled with Contributor+, so a Viewer cannot discuss observed work and large projects cannot restrict individual project/channel spaces.
3. Principal/membership/token primitives exist without a normal team-administration workflow.
4. Writable shared sessions serialize turns but do not identify the human controller of an active turn.
5. The global Workspace dashboard is still a modal overlay. It owns keyboard input while open, cannot retain the ordinary project composer, and cannot use the side panel as project chat routed by the selected project.

This addendum is corrective: it does not replace the closed foundations. It converges exposed compatibility surfaces on the existing authority model and adds the missing policy/control/frontend seams.

## 2. Work classification

### Invariants

- Every network principal is authenticated and every project-scoped read/mutation is authorized at the daemon or an equivalent canonical authorization boundary.
- Active project membership is required before chat policy can grant access.
- Chat access never grants execution authority.
- An active shared turn has at most one effective human controller principal.
- Workspace selection is presentation/routing state, never a new project/workspace authority.
- Project/channel/session identities are canonical locators; no ambient cwd or frontend-selected principal grants authority.

### Capabilities

- Read-only observers may be explicitly granted project or channel chat.
- Owners can adjust chat access and manage project memberships through a normal surface.
- Team members can safely observe a shared session and request/transfer control without implicit co-driving.
- Workspace view keeps the normal Session/Task composer and shows/focuses chat for the selected project in the side panel.

### Infrastructure

REST authorization adapters, chat policy persistence/resolution, membership/token management protocol, controller lease persistence, and Workspace primary-view state.

### Polish

Team-management dialogs, chat side-panel focus/composer, controller indicators, and bounded diagnostics.

## 3. Non-goals

- OIDC/device login, email invitations, SCIM, organization-wide groups, or a general policy language.
- End-to-end encrypted chat or general social/DM functionality.
- Concurrent turns in one session or collaborative text editing.
- Replacing isolated sessions/worktrees as the preferred parallel-development model.
- Rewriting the project collaboration store, scheduler, projection system, or WorkOrder scheduler.
- Making legacy REST a second authority equal to Core.

## 4. Current implementation evidence

- `auth_middleware` resolves personal tokens to `AuthenticatedPrincipal`, but `src/server/routes/session.rs`, `project.rs`, `workspace.rs`, `file.rs`, permission/question routes, and `/api/event` do not generally consume that principal through `AuthorizationService`.
- `/api/event` subscribes directly to `GlobalEventBus`, creating an unfiltered network event surface.
- `ProjectRole::Viewer` lacks `ProjectChat`; Contributor+ receives it together with broad mutation capabilities.
- `TeamStore` and `PersonalTokenStore` implement principal, membership, revision, token issuance/revocation, and restart-safe persistence; production TUI/Core administration operations are missing.
- `TurnSubmit`, `TurnSteer`, and `TurnCancel` all map to `agent.invoke`; `active_turn` serializes turns but does not constrain the controlling human.
- `WorkspaceDashboard` is a `FocusManager` modal (`Dialog::WorkspaceDashboard`) whose bare characters are filter input. The ordinary prompt cannot remain active while it is open.
- `ChatState` is already project keyed with per-project drafts and stale/reconnect guards, so Workspace chat should reuse it rather than introduce another chat cache/server.

## 5. Target architecture

Network compatibility adapters become thin clients of daemon/Core authority or use one shared server authorization adapter backed by the same `AuthorizationService` and canonical scope resolver. Unsafe global compatibility streams are LocalOwner-only or replaced by filtered project/session subscriptions.

Chat access follows ADR-0006: role defaults plus revisioned project/channel overrides, with membership always required. Team administration exposes only canonical daemon operations; project membership administration uses `member.manage`, while global principal/device-token issuance remains LocalOwner-only until a deployment-admin identity is deliberately added.

Shared active-turn control follows ADR-0007. The controller lease narrows existing capability authority and is visible through bounded projection/presence metadata.

Workspace becomes a non-modal primary TUI view. Its selected project is an explicit route target. The main composer uses that project for ordinary Session/Task submission; a dedicated project-chat side panel uses the same selected project and the existing `ChatState`/`chat.v1` server. Switching Workspace selection switches the chat projection/draft without changing authorization or fabricating a session.

## 6. Dependency graph

```text
M001 network authorization convergence
   |
   +--> M002 project/channel chat policy ----+--> M003 team administration
   |                                        +--> M005 Workspace + chat side panel
   |
   +--> M004 shared-session controller lease
                                               \
                         M001-M005 --------------> M006 trajectory/security qualification
```

M001 is the release gate for all team-facing work. M002 consumes accepted ADR-0006. M004 consumes accepted ADR-0007. M003 requires M002 so one administration surface can manage both membership and chat overrides. M005 requires M002 so selected-project chat has correct policy semantics. M006 hard-depends on all production milestones.

## 7. Milestones

### M001 — Network API authorization convergence

Class: invariant/security corrective. Make every authenticated REST/SSE compatibility route either pass through canonical Core/AuthorizationService project scope or be explicitly LocalOwner-only. Close permission/question and global-event bypasses. No team deployment may expose a store-mutating authenticated route that bypasses project authorization.

Plan: `plans/implementation/team-collaboration-corrective/001-network-api-authorization-convergence.md`.

### M002 — Project/channel chat access policy

Class: capability/security. Implement ADR-0006 with durable revisioned overrides, privacy-safe channel filtering, and owner administration operations. Viewer remains read-only by default but can be explicitly granted chat; Contributor+ remains allowed by default and can be denied.

Plan: `plans/implementation/team-collaboration-corrective/002-project-channel-chat-access-policy.md`.

### M003 — Team membership and device-token administration

Class: capability. Add canonical membership management and LocalOwner principal/device-token provisioning surfaces, then make `/team` a real project-team administration view while `/collaborators` remains ephemeral presence.

Plan: `plans/implementation/team-collaboration-corrective/003-team-membership-and-token-administration.md`.

### M004 — Shared-session controller lease and handoff

Class: capability/security. Implement ADR-0007 across turn steer/cancel and permission/question control, with explicit request/transfer/takeover and reconnect/recovery behavior.

Plan: `plans/implementation/team-collaboration-corrective/004-shared-session-controller-lease.md`.

### M005 — Non-modal Workspace view and selected-project chat

Class: capability/polish. Promote Workspace from a modal dashboard into a primary view that retains the ordinary Session/Task composer and uses the side panel for chat belonging to the selected project.

Plan: `plans/implementation/team-collaboration-corrective/005-workspace-selected-project-chat-view.md`.

### M006 — Multi-user collaboration trajectory and security qualification

Class: invariant/qualification. Exercise several principals/devices across REST/Core, chat grants/denials, membership revocation, controller handoff, reconnect, and Workspace routing. No new product semantics unless a defect requires a separately recorded corrective.

Plan: `plans/implementation/team-collaboration-corrective/006-multi-user-trajectory-security-qualification.md`.

## 8. Cross-cutting requirements

All new durable records use additive restart-safe migrations and optimistic revision checks where administrators can race. Protocol changes are additive and version/capability discoverable. Secret plaintext is one-time only and never enters chat, audit metadata, logs, snapshots, or persisted TUI state. Revocation applies at request time. Unauthorized project/channel/session existence remains indistinguishable from absence.

Frontend reducers remain bounded and generation guarded. Workspace and chat views perform no I/O in render. No task-per-message or per-project eager activation is introduced.

## 9. Verification strategy

Each production milestone owns focused unit/integration tests. M006 supplies the cross-cutting multi-principal harness. Every implementation must run formatting, authorization-matrix/static guards affected by its surface, targeted tests, `scripts/verify.sh quick`, and `git diff --check`; closure records must distinguish local from CI evidence.

## 10. Risks and decision points

The main risk is accidentally creating a second authorization system. M001/M002 must reuse canonical principal/project resolution and keep chat policy narrow. Another risk is frontend route confusion: Workspace-selected project must never silently mutate the hidden prior session. M005 therefore requires explicit selected-project route tokens and stale-completion guards.

## 11. Completion definition

The corrective campaign closes only when a Viewer can be granted one project/channel chat surface without receiving write authority; a Contributor can be denied chat without losing unrelated project capabilities; project membership can be administered through CodeGG; active shared turns have explicit controller ownership; Workspace provides selected-project chat alongside the ordinary composer; and no authenticated network compatibility route bypasses canonical authorization.

## 12. Milestone status

| Milestone | Status | Dependencies |
|---|---|---|
| M001 | closed | closed identity/authorization foundation (`plans/closure/team-collaboration-corrective/001-status.md`; implementation `cc41e4a4`) |
| M002 | closed | chat access overlay (`plans/closure/team-collaboration-corrective/002-status.md`; implementation `2f4bec32`) |
| M003 | closed | team administration (`plans/closure/team-collaboration-corrective/003-status.md`; implementation `f7afefb1`) |
| M004 | closed | M001 closed (closure `plans/closure/team-collaboration-corrective/004-status.md`; implementation `1bc967c1`) |
| M005 | closed | M001 + M002 closed (closure `plans/closure/team-collaboration-corrective/005-status.md`; implementation `86217a7d`) |
| M006 | ready | M001-M005 (M001+M002+M003+M004+M005 closed) |
