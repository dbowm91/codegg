# Project Work Orders and Task View — UX Fidelity Corrective Addendum

Status: closed

Repository audit baseline: `e013acd282afd63e7ca2211190a1342f76ae3e5d`

Related closed work:

- `plans/subsystems/project-work-orders-task-view-roadmap.md` — M001-M007 closed.
- `plans/closure/project-work-orders-task-view/003-status.md` — Task composer/scheduling sheet/project Task view closure.
- `plans/closure/project-work-orders-task-view/005-status.md` — external task-trigger capability/endpoint closure.
- `plans/closure/project-work-orders-task-view/007-status.md` — composed trajectory/recovery/security qualification closure.

Applicable architecture decisions and governance:

- `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`
- `plans/adrs/ADR-0004-approval-routing-sandbox-and-runtime-preferences.md`
- `plans/003-planning-process.md#7-corrective-passes`

No new ADR is required. The selected WorkOrder/session/scheduler/worktree/authorization architecture is sound. This addendum corrects user-facing fidelity gaps discovered after closure; it does not reopen the durable execution architecture or rewrite predecessor closure history.

## 1. Purpose and corrective trigger

A post-closure review compared the landed M001-M007 capability with the original Project Task/Workspace product brief rather than only with the milestone-local acceptance matrices. The backend, recovery, authorization, and concurrency architecture closed cleanly, but three user-facing requirements were deliberately weakened or deferred during implementation and were then accepted as closed:

1. **Composer mode selection:** the requested interaction was that normal Session composition remains the default and bare `Tab` cycles to Task composition. M003 instead preserved bare `Tab` as `SwitchAgent` and introduced `Ctrl+G` for `Session <-> Task`.
2. **Human external-trigger setup:** M005 implemented the narrow verifier-only trigger record, management protocol, and authenticated/idempotent POST fire endpoint, but its closure explicitly left the Task scheduling sheet's external-trigger row disabled and deferred a human TUI/CLI management surface to a later milestone. No such milestone was registered.
3. **Queue editing:** the requested scheduling interaction allowed direct `j/k` or arrow-key reorder. M003 uses `j/k`/arrows for selection and `Shift+J/K` for Task-view reorder; the scheduling sheet does not provide the requested focused direct reorder interaction.

These are not reasons to invalidate the closed M001-M007 correctness evidence. They require one bounded corrective capability pass whose acceptance tests pin the literal user workflow that the milestone-local tests did not.

## 2. Work classification

Primary class: **capability / usability corrective**.

### Invariants

- `WorkOrder` remains the durable project-level future-work identity; no second task scheduler or speculative Session model is introduced.
- `WorkOrderCoordinator` remains the only WorkOrder release/materialization path and `JobSubmissionService` remains the canonical durable job admission boundary.
- `InputMode::Insert | Normal` remains a text-editing/Vim concern; composer mode remains separate frontend state.
- A Task prompt is not written to a session transcript before WorkOrder creation succeeds.
- The TUI remains a controller/projection. Durable WorkOrder, trigger, occurrence, lane, session, and job truth remain daemon/core-owned.
- Trigger bearers remain narrow one-purpose capabilities and never become principals, normal project tokens, or model-visible secrets.
- Trigger plaintext is returned/displayed only at creation time and must not enter persistence, logs, audit metadata, prompt history, session transcript, event payloads, or debug projections.
- Existing All/Any gate semantics, finite repeat semantics, lane CAS, model snapshotting, policy narrowing, worktree isolation, and restart reconciliation remain unchanged.
- Closing/switching TUI views never cancels daemon-owned WorkOrders or materialized sessions implicitly.

### Capabilities

- A fresh prompt starts in Session composer mode and bare `Tab` reaches Task composer mode without changing Insert/Normal editing state.
- Existing agent selection remains reachable through a deliberate, documented configurable action after the Tab ownership change; there must not be two competing bare-Tab handlers.
- Enter in Task mode opens the scheduling sheet, whose zero-delay/default behavior remains immediate.
- A focused sequential queue editor allows direct `j/k` and Up/Down movement of eligible future entries while pinned/running entries remain immovable and every durable reorder remains revision/CAS guarded.
- A user can enable an external-trigger gate from the Task scheduling flow, create the WorkOrder, receive the one-time trigger bearer/invocation instructions through a transient safe-display surface, and use the already-landed POST endpoint from an external script.
- If WorkOrder creation commits but trigger setup does not complete or the one-time secret response is lost, the Task view exposes truthful recovery (retry setup or explicit revoke/recreate/rotate) rather than deleting the WorkOrder or pretending the secret is recoverable.

### Infrastructure

No new scheduler, authorization framework, durable workflow layer, or trigger verifier design is expected. The corrective pass consumes the existing M003 TUI state/command architecture and M005 trigger-management protocol/HTTP capability.

A new storage migration or public protocol family is not expected. If implementation proves that an atomic WorkOrder+trigger durable transaction requires a materially new cross-domain public contract, stop and split that decision into a separate plan rather than hiding it inside this UX corrective.

## 3. Non-goals

- No redesign of WorkOrder/Occurrence/SequenceLane identities or lifecycle.
- No replacement or duplication of `WorkOrderCoordinator`, scheduler, `JobSubmissionService`, SessionStore, WorktreeService, or runtime-preference ownership.
- No arbitrary webhook payloads, prompt mutation from trigger bodies, dynamic templates, third-party integrations, event subscriptions, or GET-trigger links.
- No agent/model access to trigger secrets and no extension of the model-facing `work_order` tool to create external trigger credentials.
- No unbounded recurrence or new gate language.
- No generic secrets framework or credential-store unification solely for this one-time TUI display.
- No broad TUI keymap redesign unrelated to the Session/Task selector conflict.
- No drag-and-drop/mouse queue editing requirement.
- No new CI lane, live-provider test, public-network trigger test, benchmark gate, or broad UI automation framework.

## 4. Why predecessor verification did not catch the gaps

### M003 composer/keybinding closure

M003 correctly identified the collision between bare `Tab` (`SwitchAgent`) and the proposed Task selector, but the implementation plan allowed a collision-free alternative if the selector model was not migrated. Closure then tested the chosen `Ctrl+G` behavior and treated preservation of `Tab` as success. That verified internal consistency, not literal fidelity to the original product interaction.

### M005 trigger closure

M005 deliberately scoped the trigger backend so that an existing safe secret-display surface could be used if available; otherwise protocol response alone was sufficient. Closure explicitly recorded that there was no TUI/CLI trigger surface and that the scheduling row remained a disabled placeholder pending a later milestone. Because no follow-up milestone was registered, infrastructure closure was incorrectly allowed to stand in for the originally requested human capability.

### M003 queue-reorder closure

M003 chose `Shift+J/K` in the full Task view so bare `j/k` could remain selection navigation. The tests pin that choice. They do not prove the requested scheduling-sheet interaction in which a focused queue editor can use direct `j/k`/arrows to move future work.

### M007 qualification closure

M007 strongly qualified the composed backend and pure TUI state transitions, including lane ordering, trigger replay, stale completion, and the representative 15-plan trajectory. It did not assert the literal root-composer key path or a human trigger-creation/display/recovery flow, so the above UX omissions were outside its effective coverage.

## 5. Target user flow

### Session/Task composer selection

```text
new/normal prompt composer
  Session (default)
      |
      | bare Tab
      v
  Task
      |
      | bare Tab
      v
  Session
```

The exact rendering may remain the existing `composer:task`/model/permission strip, but there must be one ownership model for bare Tab at root prompt focus. Modal-local Tab behavior remains contextual: when a scheduling/dialog component owns focus, Tab may continue to move fields rather than changing the root composer mode.

`SwitchAgent` remains a configurable action and must remain discoverable/reachable after its default Tab binding is displaced. The implementation plan owns the collision audit and help/keybinding migration; it must not install another handler that races with composer-mode Tab.

### Scheduling and sequential reorder

The scheduling sheet keeps the current zero-delay/repeat/model/policy fields. When sequential queue placement/reorder is the focused control, direct `j/k` and Up/Down move only eligible unclaimed future members. Running/claimed head entries are visibly pinned. Every committed move uses the existing lane revision/CAS service; stale revisions refresh rather than applying a speculative local order.

The existing Task-view `Shift+J/K` shortcut may remain as an additional power-user path. The corrective requirement is the focused scheduling interaction, not a forced removal of established navigation semantics elsewhere.

### External-trigger setup

When the external-trigger option is enabled:

1. the WorkOrder is created through the canonical M001 service with an `ExternalTrigger` gate participating in the selected All/Any join;
2. after WorkOrder creation returns a durable identity, the client invokes the existing project-authorized trigger-management operation to create/bind the trigger;
3. on success, the one-time bearer and a bounded invocation example are shown in a transient secret-aware dialog/surface;
4. closing the secret surface forgets the plaintext in TUI state; metadata remains listable but the secret is never re-readable;
5. external firing continues to use the existing `POST /api/v1/task-triggers/<id>/fire` capability and never starts a session directly.

A two-step WorkOrder-create then trigger-create flow is acceptable because the WorkOrder safely remains gated while no trigger has fired, but partial success must be explicit and recoverable:

- if WorkOrder creation fails, no trigger creation is attempted;
- if WorkOrder creation succeeds and trigger creation fails, do not delete the WorkOrder; surface `trigger setup incomplete` and provide an authenticated retry action;
- if trigger creation committed but the one-time response is lost/stale, metadata may reveal that an active trigger exists but plaintext cannot be recovered; offer explicit revoke+recreate/rotate to obtain a new bearer;
- a retry/rotate must be idempotent/race-safe and must not create multiple effective trigger capabilities accidentally.

## 6. Dependency graph

```text
Closed M003 Task composer/view -------\
Closed M005 trigger backend -----------+--> C001 Human Task UX fidelity + trigger surface
Closed M007 composed qualification ----/
```

All hard dependencies are closed at the audit baseline. C001 is dependency-ready.

No external operational dependency is required. Trigger HTTP tests remain loopback/in-process.

## 7. Corrective milestone

### C001 — Human Task composer, queue, and external-trigger UX fidelity

Class: capability / usability corrective.

Implementation plan:

- `plans/implementation/project-work-orders-task-view-corrective/001-human-task-ux-and-trigger-surface.md`

Objective:

Finish the human-facing Task workflow without changing the closed WorkOrder execution architecture: bare Tab cycles Session/Task at root composer focus, focused queue editing supports direct `j/k`/arrow reorder through lane CAS, and the scheduling/Task view exposes secure creation/display/retry/rotation of the already-landed external-trigger capability.

Exit conditions:

- Session is the default composer mode after normal startup/new-session flow.
- At root prompt focus, bare `Tab` deterministically cycles Session↔Task in both Insert/Normal editing maps without mutating `InputMode`.
- `SwitchAgent` remains reachable/configurable and help/keybinding docs contain no stale statement that bare Tab owns it.
- Modal-local scheduling Tab navigation remains scoped to the modal and does not toggle root composer state.
- Focused sequential queue editing accepts `j/k` and Up/Down for eligible future movement, visibly pins running/claimed entries, and commits through existing revisioned lane CAS.
- External trigger can be enabled in the human scheduling sheet and combined correctly with delay/not-before/sequence gates under All/Any.
- WorkOrder creation still happens before/through canonical daemon WorkOrder ownership; no TUI session/job creation appears.
- Trigger creation uses the existing authorized M005 management operation and the existing narrow POST fire endpoint.
- The one-time trigger bearer is displayed only in a transient safe surface and is absent from durable TUI caches, transcript/history, logs, audit/events, debug output, and model/tool context.
- Partial success/lost-response recovery is truthful: retry setup where no trigger exists; rotate via revoke+create where an active trigger exists but the bearer is unavailable.
- Existing M001-M007 WorkOrder, trigger, dashboard, agent-tool, permission, model-policy, restart, and trajectory suites remain green.
- A new closure record explicitly maps these three original-product requirements to end-to-end evidence.

## 8. Cross-cutting requirements

### Storage/migration

No migration is expected. Existing `task_trigger`/receipt and WorkOrder tables remain authoritative. If a new durable field appears necessary only to make the UI easier, first prove that existing trigger metadata and WorkOrder attention/projection cannot represent the state; otherwise do not bump storage.

### Protocol/compatibility

Prefer the already-landed `WorkOrderCreate` and `WorkOrderTriggerCreate/List/Get/Revoke` protocol. Do not add a second trigger family. A new composite protocol request is not part of C001 by default; if atomic creation is judged mandatory for correctness rather than convenience, stop and register a follow-up contract plan.

Older servers without trigger-management capability continue to render the external-trigger option disabled with a clear capability diagnostic. WorkOrder-capable but trigger-incapable compatibility must fail visibly, never silently drop the gate.

### Authorization/privacy

Trigger management continues to require the existing project-scoped capabilities. TUI controls are display conveniences, not authority. Revocation or project/session capability loss during an in-flight setup must fail closed and preserve opaque-project privacy rules.

### Secret handling

The one-time bearer is allowed to exist only in the immediate response path and transient display state needed to show/copy it to the authorized user. The implementation should use an opaque/redacted wrapper or equivalent local type whose normal `Debug`/display path does not expose plaintext; do not introduce a repository-wide secrets abstraction unless existing code already provides a suitable generic one.

### Verification

Keep verification targeted. Add input/keymap, modal queue, trigger-setup/recovery, stale-route, and secret-redaction tests plus reuse existing M003/M005/M007 suites and static guards. Do not create a new UI harness framework or hosted CI lane.

## 9. Risks and decision points

- **Tab regression:** changing bare Tab can make agent switching undiscoverable. The corrective pass must migrate that action intentionally and test discoverability rather than simply deleting its binding.
- **Terminal encoding:** avoid choosing a replacement binding for `SwitchAgent` that is indistinguishable from Tab in common terminal encodings. Use the repository's keybinding abstraction and collision tests.
- **Partial trigger setup:** WorkOrder creation may commit before trigger creation. Never rollback by deleting durable user work from the frontend. Preserve it and expose recovery.
- **Lost one-time secret:** an active trigger's plaintext cannot be reconstructed. Recovery is rotation, not a hidden secret-read API.
- **Stale async completion:** a trigger may be created after the user switches project/view. Drop foreground UI effects without logging the secret; later metadata should show configuration and offer rotation if the bearer was not captured.
- **Secret leakage through convenience:** do not put the bearer in ordinary notifications, command history, prompt text, model context, structured tracing fields, or persisted view snapshots just to make copying easier.
- **Queue control ambiguity:** direct `j/k` reorder is required only when the queue editor/reorder control owns focus. Outside that context, normal Vim navigation may remain.

## 10. Deferred work

- third-party webhook/provider integrations;
- arbitrary trigger payloads or prompt templating;
- generic service-account/token management;
- mouse/drag queue manipulation;
- web/desktop/mobile Task trigger UX;
- automated trigger rotation policies;
- broader keymap redesign beyond the Session/Task/agent-selection collision;
- atomic multi-resource WorkOrder+trigger API unless C001 implementation proves it is a correctness requirement and registers it separately.
