# Team Collaboration Corrective M005 — Workspace Selected-Project Chat View

Status: ready (unblocked by M002 closure at
plans/closure/team-collaboration-corrective/002-status.md;
M001+M002 hard dependencies closed)

Repository baseline: `4e12ecc192ba7e2192d3a62acce6e15fc1bb1153`

Source roadmap: `plans/subsystems/team-collaboration-corrective-addendum.md#M005`

Long-term requirements: `plans/000-long-term-specification.md#13`, `#15`, `#21`, `#25`.

Applicable ADRs: `plans/adrs/ADR-0005-project-work-orders-and-task-orchestration.md`, `plans/adrs/ADR-0006-project-channel-chat-access-policy.md`. Primary class: capability.

Hard dependencies: M001 and M002 strict closure.

## 1. Objective

Convert the global Workspace dashboard from a modal overlay into a non-modal primary TUI view. Retain the ordinary bottom Session/Task composer and use the right-side panel as project chat for the Workspace-selected project. The chat server/state remains the existing daemon `chat.v1` service; selection only changes the project locator used to address it.

## 2. Why this milestone is blocked

Workspace-selected chat should be built on final M002 access semantics and must not expose a polished team surface before M001 closes.

## 3. Current implementation evidence

`Dialog::WorkspaceDashboard` is FocusManager-owned. Bare printable keys are dashboard filter input, so opening it necessarily steals the ordinary prompt. `WorkspaceDashboardState` already has bounded rows, selected project, generation/reconnect guards, one lazy task expansion, and no daemon truth. `ChatState` already caches bounded project-keyed chat windows/drafts and chat commands accept explicit project IDs. The existing `SidebarWidget` is session-centric and does not own chat.

## 4. Invariants that must not regress

- Workspace is a projection, not a durable Workspace identity or execution owner.
- Selected project is an explicit routing locator, never authority.
- Main composer must never silently send to the hidden prior session when Workspace selection points at another project.
- Chat side panel always corresponds to the current Workspace-selected project and uses existing `ChatState`/Core requests.
- Project switching/reconnect/revocation cannot display one project's messages/draft under another project.
- Render paths perform no I/O and caches stay bounded.

## 5. Scope

In: primary Workspace route/view state, non-modal rendering/input, selected-project route token, Session/Task composer routing, chat side-panel component/focus/draft/send/history, `/workspace` compatibility, return semantics, tests/docs.

Out: new chat storage/server, general multi-pane framework rewrite, new scheduler semantics, eager loading every project, replacing project tabs.

## 6. Required production changes

Promote Workspace to an ordinary primary view (for example additive `Route::Workspace` or an equivalent explicit primary-view enum) rather than `Dialog::WorkspaceDashboard`. Reuse the current dashboard reducer/data request and remove modal ownership for normal Workspace navigation. Modal dialogs may still open above Workspace for confirmations/details.

Keep a canonical `selected_project_id` in Workspace view state, guarded by generation/reconnect epoch. Selection does not automatically activate heavy project services.

Main composer semantics while Workspace is active:

- Session mode submits against the selected project's canonical project/workspace context. If no suitable session is selected, create/focus an ordinary session for that project through existing session creation rather than using the hidden prior session.
- Task mode creates/schedules a WorkOrder for the selected project through the existing Task composer path.
- if project/workspace context is ambiguous or unavailable, fail visibly; never fall back to cwd or the previously active project.

Side panel semantics:

- render a dedicated project-chat panel in the normal sidebar region when Workspace is active;
- route history/sync/draft/send to `ChatState[selected_project_id]` and existing `chat.v1` Core operations;
- allow explicit panel focus with its own draft editing/Enter send; when prompt focus is active, typing remains Session/Task input;
- selection change switches chat projection/draft and refreshes only when stale; denied/unsupported chat shows the generic unavailable state;
- unread/read/composing behavior reuses the existing reducer and does not create a second chat cache.

## 7. Ordered work packages

A. Add primary Workspace route/view state and migrate open/close/return behavior from modal to view without changing the daemon aggregate.
B. Adapt Workspace navigation/filter/expand to non-modal focus semantics and preserve existing bounded refresh behavior.
C. Add selected-project execution-context resolution for Session/Task composer; add hard negatives preventing hidden-session/cwd fallback.
D. Add project-chat side-panel projection/component over existing `ChatState`, with explicit focus and per-project draft.
E. Wire selection changes, event hints, reconnect/revocation, unread/read and narrow-terminal behavior.
F. Remove obsolete Workspace modal ownership after compatibility migration and update docs/help/tests.

## 8. Failure, cancellation, restart, contention semantics

Leaving Workspace cancels/invalidates only frontend requests, never daemon work. Stale dashboard/chat/session-create completions are dropped by project+generation+epoch tokens. A chat send failure retains that project's draft. A session-create failure retains the main prompt. Revocation clears denied project/chat cache and does not fall back to another project's data.

## 9. Compatibility and migration

`/workspace` and its current hotkey continue opening Workspace. Existing dashboard data protocol remains unchanged unless an additive selected-project detail request is strictly required. Project tabs and Task view continue to work; Enter may still descend into the selected project's Task/session view. The old modal variant can be removed only after no input/render path depends on it.

## 10. Required tests

Open Workspace while a session is active and prove the session keeps running; ordinary composer remains editable; selecting Project B then Session-submit creates/focuses only Project B session; Task submit targets Project B; no cwd/prior-session fallback; side panel shows Project B chat and not A; switching A/B preserves separate drafts; Viewer without grant unavailable; Viewer with M002 grant chats; Contributor denied channel unavailable; reconnect/revocation clears correctly; modal confirmation above Workspace returns to same selection; narrow terminal degrades cleanly.

## 11. Required verification commands

- existing Workspace tests (`work_orders_m004_dashboard` and TUI dashboard tests)
- `cargo test --test collaboration_m002_chat_tui`
- new Workspace/chat routing TUI integration tests
- `cargo test --test tui_project_routing --test tui_project_tabs`
- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `scripts/verify.sh quick`
- `git diff --check`

## 12. Documentation updates

Update `architecture/tui.md`, `architecture/work_orders.md`, `architecture/collaboration.md`, TUI skill docs, help/keybinding descriptions, and any closure-era text that still calls Workspace a modal overlay.

## 13. Acceptance criteria

Workspace behaves as a stable top-level view. The bottom composer can start ordinary Session/Task work for the selected project, while the side panel independently shows and sends chat for that same selected project. No operation is routed by the hidden prior session, cwd, or stale selection.

## 14. Stop conditions

Do not build a second chat server/cache or load full project/session state for every dashboard row. Stop if selected-project composer routing cannot obtain canonical workspace context without eager project activation; surface an explicit chooser/error instead.

## 15. Closure evidence required

Render/input route diagrams, project-switch stale-guard tests, selected-project session/task/chat routing tests, compatibility evidence for `/workspace`, and confirmation that normal session execution continues while Workspace is displayed.

## 16. Handoff notes

The key UX distinction is focus: main composer = Session/Task for selected project; chat panel composer = inert project chat. Keep those two input targets explicit.
