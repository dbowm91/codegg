# Team Collaboration Corrective M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/team-collaboration-corrective/005-workspace-selected-project-chat-view.md`

Source subsystem roadmap:

- `plans/subsystems/team-collaboration-corrective-addendum.md#M005`

Repository baseline reviewed: `86217a7d`

Implementation commits or pull requests:

- `86217a7d` — feat(workspace): non-modal selected-project chat view (team-collaboration M005)

## 1. Executive finding

M005 is complete. Workspace is a stable non-modal primary TUI view
(`Route::Workspace`): the ordinary bottom Session/Task composer stays
editable for the Workspace-selected project while the sidebar region
shows project chat for that same selection over the existing daemon
`chat.v1` service and `ChatState` (no second server/cache). Selection is
an explicit routing locator (`selected_project_id`, generation +
reconnect guarded); it never activates heavy services, never confers
authority, and never falls back to cwd or the hidden prior session.
Session submits against the selected project's tab context (mismatched
hidden sessions force creation of an ordinary session for the
selection); Task submits create `WorkOrder`s for the selection;
ambiguous context fails visibly. Chat focus owns per-project draft
editing/Enter send; prompt focus keeps Session/Task input. Denied/
unsupported chat renders the generic unavailable state; revocation
clears the dashboard row plus that project's chat cache. Narrow
terminals hide the sidebar as the clean degrade. `/workspace` + `Ctrl+O`
open the view; `/chat` focuses the side panel while the view is active.
Normal Workspace navigation no longer pushes the obsolete
`Dialog::WorkspaceDashboard` modal (retained only as a compatibility
shim, never pushed).

## 2. Requirement-to-evidence matrix

| Plan requirement (§6) | Evidence |
|---|---|
| Promote Workspace to ordinary primary view (`Route::Workspace`), reuse dashboard reducer/data request, remove modal ownership for normal navigation; modals may still open above | `src/tui/route.rs` (`Route::Workspace`); `src/tui/commands/workspace_dashboard.rs::open_workspace_dashboard` (navigates, no `push_dialog`), `leave_workspace_view` (route `back()`, generation bump + cancel, never daemon cancel); `src/tui/app/render.rs::render_workspace_view` (viewport reads state, no I/O); `handle_dialog_key` still routes other modals above the view; `Dialog::WorkspaceDashboard` push removed (legacy arm only) |
| Canonical `selected_project_id` guarded by generation/reconnect; no auto-activation of heavy services | `WorkspaceDashboardState::selected_project_id/selected_display_name` + existing `generation/reconnect_epoch` guards (`apply_loaded/apply_expanded` drop stale); `open`/`refresh` issue one bounded aggregate, `toggle` one bounded `WorkOrderList` (no N+1, counted in tests) |
| Session mode submits against selected project's context; create/focus ordinary session when unsuitable; fail visibly, never cwd/prior fallback | `workspace_composer_context/composer_execution_context/composer_tab_id/composer_project_id`; `prompt_turn.rs::send_prompt` (workspace branch: hidden-session mismatch forces creation, focuses selected tab, token carries composer target); `prompt.rs::current_route_check` + mismatch-tolerant apply guard; hard negatives pinned by `session_submit_fails_visibly_without_selected_tab_never_fallback` |
| Task mode creates WorkOrder for selected project via existing path; fail visibly | `work_orders.rs::open_task_sheet_for_prompt/confirm_task_schedule/capture_route/current_task_route_check` all use composer helpers; stale guards compare `composer_project_id` (selection-aware); `task_submit_targets_selected_project` + `task_submit_fails_visibly_without_selected_tab` |
| Side panel in normal sidebar region over `ChatState`, explicit focus, per-project draft, stale-only refresh, denied → unavailable, reuse reducer | `render.rs::render_sidebar` branches to `render_workspace_chat_panel` (panel_lines, draft preview, focus highlight, no I/O); `sync_workspace_chat_panel` (needs_refresh only); `focus_workspace_chat/focus_workspace_composer`, `push/pop/send_workspace_chat` (per-project draft, failure retains); `/chat` focuses panel while active; `side_panel_shows_selected_chat_with_separate_drafts` |
| Selection/event/reconnect/revocation/unread/read/narrow wiring | `move_dashboard_selection` syncs chat; chat event/hint handlers use `chat_target_project_id` (committed/edited/redacted/composing/hints/actions); `on_projection_reconnect` refreshes chat target + dashboard resync; `clear_workspace_revoked` (row + chat, no cross-fallback) + chat-denial hooks clear dashboard row; unread/read/composing reuse reducer (no second cache); narrow hides sidebar via layout bounds (`narrow_terminal_degrades_cleanly` renders 58–120 cols with no error) |
| Remove obsolete modal ownership; docs/help/tests | No `push_dialog(WorkspaceDashboard)` in normal flow; help `OpenWorkspaceDashboard` → “Open Workspace view (selected-project chat)”; docs `architecture/tui.md`, `architecture/work_orders.md`, `architecture/collaboration.md`, `.opencode/skills/tui/SKILL.md`; tests below |

## 3. Production implementation evidence

- Route/view: `src/tui/route.rs` (`Workspace`); `src/tui/app/state/workspace_dashboard.rs`
  (`WorkspaceFocus`, `selected_project_id/display_name`); `App::workspace_focus`
  (`src/tui/app/mod.rs`, both constructors); `render.rs::render_workspace_view`,
  `render_sidebar` → `render_workspace_chat_panel`, header `Route::Workspace` title.
- Navigation/input: `handle_workspace_view_key` (bool consumed; chat-focus
  owns printable/Enter/Backspace, Up/Down/PgUp/PgDn/Home/End always move,
  `Space` expands, empty-`Enter` descends, `Tab` falls through to composer,
  Normal-only `j/k/g/G`); `App::on_key` workspace branch (view keys first,
  Normal unbound text → filter, `Cancel` blurs/leaves never exits,
  `Navigate/Page/GoTo` move selection, `FocusSidebar` focuses chat,
  `ToggleSection` expands).
- Composer: `workspace_dashboard.rs` composer helpers;
  `prompt_turn.rs` (human-shell root + Session hidden-guard + tab focus +
  composer token); `work_orders.rs` (sheet/confirm/capture/check all
  composer-aware; guards use `composer_project_id`); `prompt.rs`
  (composer check + mismatch-tolerant create guard).
- Chat: `project_session.rs::chat_target_project_id`;
  `/chat` focuses panel while active; all `/chat*` use chat target
  (except `/observe`, still active); event/hint/reconnect paths use chat
  target; denial clears dashboard row (`chat.rs` history/sync +
  `clear_workspace_revoked`); `command_dispatch.rs` hint paths updated.
- Compatibility: `/workspace` + `Ctrl+O` open the view; dashboard protocol
  unchanged (aggregate + one bounded detail); project tabs + Task view
  unchanged (`Enter` descends via `open_or_focus_project` + `open_task_view`);
  modal dialogs above return to same selection (pinned).
- Help: `src/tui/input.rs` label + help entries updated; collision audit intact.

## 4. Verification executed (commands + results; local vs CI truthfully)

Local (this machine, `CARGO_BUILD_JOBS=1`):

- `cargo test --test workspace_m005_selected_project_chat -- --test-threads=1` — 9 passed:
  primary route + editable composer; Session targets B not hidden A;
  Session no-tab fails visibly; Task targets B; Task no-tab fails;
  side panel B-not-A + separate drafts; denied/granted/denied render;
  stale-drop + revocation clear; modal-above returns to same selection;
  narrow 58–120 cols render clean.
- `cargo test -p codegg --lib tui::commands::workspace_dashboard -- --test-threads=1` — 10 passed
  (updated to route semantics: same-dashboard open, Esc leaves via route,
  stale guards, revocation, hint without theft, one bounded detail fetch,
  no daemon cancel, help/collision).
- `cargo test --test collaboration_m002_chat_tui -- --test-threads=1` — 11 passed (M002 intact).
- `cargo test --test work_orders_m004_dashboard -- --test-threads=1` — 8 passed (M004 aggregate intact).
- `cargo test --test tui_project_routing --test tui_project_tabs -- --test-threads=1` — 20 passed.
- `cargo test -p codegg --lib tui:: -- --test-threads=1` — 960 passed.
- `cargo fmt --all -- --check` — pass; `git diff --check` — pass.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — pass.
- `cargo clippy -p codegg --all-targets --locked --features server,plugins,lsp-test-support -- -D warnings` — pass.
- `scripts/verify.sh quick` — pass (fmt, agent schema, core-boundary, sandbox,
  execution-ownership, tui-authority, workspace check).

Substituted verification (documented, not weakened): the plan lists
`cargo clippy --workspace --all-targets --all-features`. Per `AGENTS.md`,
workspace sweeps never use `--all-features` (drags real-server tests); the
two clippy invocations above cover the same code under the supported feature
sets (`verify.sh full` uses `--features server,plugins,lsp-test-support`).
No finding suppressed. `cargo test --test tui_project_routing --test
tui_project_tabs` covers the plan's routing/tabs suites in one invocation.

CI truthfully: only local evidence is claimed here; CI (`verify` job) will
re-run the canonical guards.

## 5. Invariant review

- Workspace is a projection: no new durable identity; daemon aggregate unchanged.
- Selection is a locator: generation + reconnect guarded; no activation on
  select; stale completions dropped (pinned).
- No silent hidden-session send: mismatch forces creation for the selection
  (pinned); ambiguous context fails visibly (pinned, no cwd/active fallback).
- Chat panel always matches selection: `sync` on open/move/refresh/filter;
  per-project drafts never cross-route (pinned).
- Revocation fail-closed: row + chat cleared, never another project's data (pinned).
- Render performs no I/O; caches bounded (dashboard 128/16/8, chat 16/16/100/50).
- Observer/turn separation intact (M002 suite green); controller lease intact
  (no turn/control paths touched).

## 6. Failure and recovery review

- Leaving Workspace cancels/invalidates frontend requests only (generation bump
  + cancel); daemon work untouched (session keeps running, pinned).
- Stale dashboard/chat/session-create completions dropped by
  project+generation+epoch tokens (pinned).
- Chat send failure retains that project's draft (existing `note_failed_send`
  path, reused).
- Session-create failure retains the prompt (existing failure path, composer
  token scoped).
- Revocation clears denied project/chat immediately (pinned); no fallback.
- Ambiguous timeouts surface explicit chooser/error (no-tab message names the
  selected project and forbids auto-activation, per stop condition).

## 7. Migration and compatibility review

- Additive route (`Route::Workspace`); dashboard protocol unchanged (no new
  Core operations, no migration, no layout version change).
- `/workspace` + hotkey open the same view; project tabs + Task view unchanged;
  empty-`Enter` descends, text-`Enter` submits (documented).
- Rollback drops the binary: view state is frontend-only; daemon rows/chat
  unchanged; older clients keep modal behavior (no protocol break).
- Help updated (`Ctrl+O`); collision audit green.

## 8. Security review

- No new network, crypto, secret, or authorization primitive. Chat still
  round-trips `chat.v1` with M002 policy; denials render identically
  (`Unavailable`, pinned); drafts are per-project and bounded; no prompt,
  path, secret, or content leaks into badges/titles (counts only).
- Authorization matrix unchanged (no new Core ops; `check_authorization_matrix`
  via quick guard green). `eggsentry`/adversarial suites not re-run beyond
  focused suites; no new exfiltration surface (sidebar shows the same
  `panel_lines` the modal showed).

## 9. Documentation and operations

- `architecture/tui.md`: Workspace primary-view section (route, focus,
  composer/chat routing, keys, compatibility shim note).
- `architecture/work_orders.md`: M004→M005 TUI paragraph (non-modal,
  `Space` expand, composer routing, revocation clears chat).
- `architecture/collaboration.md`: M005 section + test-table row (9 tests).
- `.opencode/skills/tui/SKILL.md`: workspace command row + project-scope
  Workspace rule.
- User help: `Open Workspace view (selected-project chat)` (`Ctrl+O`, `W`).
- Operational note: `Esc` blurs chat before leaving; `Ctrl+R` refreshes a
  stale view; revocation applies at the next request boundary; narrow
  terminals hide the chat sidebar (viewport still shows Workspace).

## 10. Unresolved findings (severity: critical/high/medium/low)

None. No critical/high/medium findings remain. One explicit non-claim: the
obsolete `Dialog::WorkspaceDashboard`/`DialogType::WorkspaceDashboard` enum
variants and the legacy `WorkspaceDashboardDialog` component are retained as
a never-pushed compatibility shim (documented in code + §7); no input/render
path depends on them for normal navigation. Full enum deletion is deferred
polish, not a correctness gap.

## 11. Roadmap disposition

M005 closes the Workspace selected-project capability. Per the addendum
dependency graph, M006 (multi-user trajectory/security qualification) hard
requires M001–M005; with M001+M002+M003+M004 already closed and M005 now
closed, M006 becomes `ready`. The subsystem roadmap stays `active` with M005
marked closed and M006 marked ready.

## 12. Registry updates

- `plans/implementation/team-collaboration-corrective/005-workspace-selected-project-chat-view.md`:
  `ready` → `implemented` (closure at this record; implementation `86217a7d`).
- `plans/implementation/team-collaboration-corrective/006-multi-user-trajectory-security-qualification.md`:
  `blocked` → `ready` (M001–M005 now closed; final qualification gate unblocked).
- `plans/registry.md`: M005 `ready` → `closed` (closure at this record);
  M006 `blocked` → `ready` on M001–M005 (M005 now closed); subsystem current
  milestone + gate paragraph + blocked-work + recently-closed rows updated.
- Subsystem roadmap
  `plans/subsystems/team-collaboration-corrective-addendum.md`: M005
  `ready` → closed, M006 `blocked` → ready in the milestone table.

Dependency audit: M006
(`006-multi-user-trajectory-security-qualification.md`) requires M001–M005
(all closed; consumes accepted ADR-0006/ADR-0007 DTOs) — now `ready`. No other
registered plan lists M005 as a hard/interface dependency. No corrective
follow-up is registered: no new defect was found that M005 itself must fix.
