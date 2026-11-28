# TUI Workspace Central Surface Corrective Milestone 007 — The Chat dock follows the selection, and filter mode keeps its promises

Status: ready for handoff

Repository baseline: `256b215ac` on `feat/auto-20261126-cae46b00` (worktree clean; verification performed 2026-11-27)

Source roadmap:

- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#13-post-closure-verification-and-corrective-milestones`
- `plans/subsystems/tui-workspace-central-surface-corrective-addendum.md#m002--entering-workspace-forces-chat-docked-right-visible-and-bound`

Source closure record this milestone corrects:

- `plans/closure/tui-workspace-central-surface-corrective/002-status.md` — records M002 closed on entry docking alone; the selection and filter paths were never exercised.

Long-term requirements:

- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui` — a project tab includes project chat and the selected workspace/worktree; the two selections must agree.
- `plans/000-long-term-specification.md#25-tui-target-behavior`

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — filter mode is a text input; keys it does not act on must not be consumed.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — no command catalog change.

Primary class: **capability** (the dock must show the selected project's chat) with an **invariant** half (selection and binding cannot disagree).

## 1. Objective

Selecting a different project — by `j`/`k`, `g`/`G`, Home/End, or by filtering the tree — binds the Chat dock to that project **and asks for its history**, painting a loading state rather than "Chat unavailable"; and in filter mode the keys the title row advertises either work or are declined instead of being swallowed.

## 2. Why this milestone is ready

The machinery already exists; the route bypasses it.

- `sync_workspace_chat_panel` (`crates/codegg-tui/src/commands/workspace_dashboard.rs:680-695`) sets the selected project, syncs the panel binding, and calls `start_chat_history` when the entry is stale. Its own comment states the contract: "selection only changes the project locator. Refreshes only when stale."
- `ChatState` is already typed — `Unknown`, `Loading`, `Ready`, `Unavailable`, `Error` (`crates/codegg-tui/src/app/state/chat.rs:143-159`) — and `needs_refresh` requests unknown or errored entries while suppressing duplicate requests in flight (`:695-709`). `start_chat_history` is the existing ensure/history path (`crates/codegg-tui/src/commands/chat.rs:100-150`).
- Live receipt: entry docking works without a session (bound to `#general 1/1`, a typed message delivered and read back), so the history path is reachable in this route and needs no M008 work.

## 3. Current implementation evidence

### 3.1 The route's selection seam does only half the work

`move_selection` and `select_edge` call `sync_selected_project` (`crates/codegg-tui/src/commands/workspace_view.rs:192-245`), whose body updates the panel binding and nothing else. The dashboard's own seam does both. That asymmetry is the whole defect: a project whose chat has not been visited is bound with no entry, and the panel paints `Chat unavailable` (`crates/codegg-tui/src/panels/chat.rs:109-111`, `:295-310`) — which is indistinguishable from a refusal.

### 3.2 Filter edits never re-bind at all

The filter arms (`workspace_view.rs:1012-1038`) update the filter text, clamp `view.selected`, and return. A filter that changes which row is selected therefore leaves the Chat dock bound to the previous project.

### 3.3 Filter mode consumes keys it does not handle

In `WorkspaceMode::Insert` the handler arms `Esc`, `Backspace`, arrows and printable characters; everything else falls through to a catch-all that still reports the key as handled (`:1012-1041`). `Enter` and `Tab` are therefore swallowed. Meanwhile the title row paints one fixed string in every mode — `j/k move · Enter reveal · i filter · Shift+H input · Shift+L chat · Shift+X exit` (`crates/codegg-tui/src/app/render.rs:1398`) — so while filtering it advertises an `Enter` that does nothing, an `i` that types the letter, and three capitals that (after M006) type text.

### 3.4 Test surface

- `side_panel_shows_selected_chat_with_separate_drafts` (`crates/codegg-tui/tests/tui_render/workspace_m005_selected_project_chat.inc:392-427`) seeds both projects' chats and drives `TuiMsg::WorkspaceDashboardMove`; it cannot observe a missing fetch or a keyboard path.
- `tui_side_panels::workspace_focus_mirrors_which_side_owns_the_keyboard` passes at `256b215ac` (it failed at `eefbfa0fa`) — the focus half of M002 is real and must stay green.

## 4. Invariants that must not regress

- The Chat dock is docked Right, visible, and bound on entry, and survives `Shift+X` + re-entry (measured, keep it).
- One authority for "which project is selected": the tree selection (`workspace_selected_project_id`, `workspace_dashboard.rs:34-46`) resolves the binding; no second selection model is added.
- `ChatState`'s typed statuses keep their meaning: `Unavailable` is a refusal, never "not fetched yet".
- No new loader, no second chat panel kind, no new protocol request: the existing `start_chat_history` path only.
- Filter mode keeps accepting printable text, including capitals (M006).
- **The eight `tui_render` failures present at the baseline are pre-existing and out of scope** (same list as milestone 006 §4). Do not fix or attribute them here.

## 5. Scope

### In scope

- Route keyboard selection and filter-clamp through `sync_workspace_chat_panel`.
- Correct what the panel paints when the binding has no entry yet.
- `Enter` activates the selected row in filter mode; unhandled keys are declined rather than consumed.
- Make the title-row vocabulary mode-aware so it stops advertising verbs that cannot work in Insert.
- Tests for each.

### Explicitly out of scope

- Sessionless team scope and the detail/team overlay (M008).
- Lifecycle, launch, request identity (M009); `/worktree` (M010); capitals in the composer (M006).
- Removing or reworking the `catch_unwind` fence around the chat render (`crates/codegg-tui/src/app/render.rs:1606-1611`). It is **not** the live failure, and turning a panic into a crash is a separate diagnostic decision. Record it as a watch item; if a panic is observed in a receipt, open its own plan.
- The pre-existing eight failures.

## 6. Required production changes

### Frontend

1. **One seam.** Delegate `sync_selected_project` to `sync_workspace_chat_panel` (or call it directly from `move_selection`/`select_edge`), so moving the selection binds *and* refreshes. Call the same seam after a filter edit or `Backspace` changes the clamped selection.
2. **Honest paint.** In `ChatPanel::draw`, a binding with no entry paints the typed status: loading for `Unknown`/`Loading`, the error for `Error`, and `Unavailable` only for a real refusal. Keep the existing statuses' renderers.
3. **Filter keys.** `Enter` in filter mode performs the Normal-mode activation for the selected row (reveal a workspace/worktree, open a project). Any key the filter does not act on returns `false` so the app sees it; do not keep the handled catch-all.
4. **Mode-aware vocabulary.** The title row picks its key list by `WorkspaceMode`/`WorkspaceInputFocus`: Insert advertises what Insert does (`type to filter · Enter reveal · Esc back`), Normal keeps today's list.

### Documentation and static guards

`architecture/tui.md:5274-5286` already says the two surfaces share `ChatState` and `start_chat_*`; extend it to state that a filter-driven selection also re-binds. `docs/tui.md`'s route key list gains the filter-mode line.

The title-row change moves the I4 advertised-key measurement for `WorkspaceDashboard`. If the measured set changes, re-bank only that surface's rows in `docs/tui-render-invariant-baseline.toml`; `scripts/check_command_catalog_ownership.py` and `check_frontend_action_set.py` are unaffected.

## 7. Ordered work packages

### Work package A — Bind and fetch through one seam

Intent: keyboard selection and filter-clamp behave like the entry path.

Required changes: the delegation above; no new request type.

Acceptance evidence: after `j` onto an unvisited project, the binding changes, one history request is issued, and a second `j` back does not re-issue while the first is loading.

### Work package B — Paint the state, not the absence

Intent: `Chat unavailable` means refused.

Required changes: the draw branching; a `Loading` pane while a fetch is outstanding.

Acceptance evidence: a bound-but-unfetched project paints loading; a refused project paints unavailable.

### Work package C — Filter mode keys

Intent: the advertised verb works and nothing is silently eaten.

Required changes: `Enter` activation; decline unhandled keys.

Acceptance evidence: `Enter` in filter mode reveals the highlighted workspace; `Tab` is not consumed (the filter text is unchanged and the app receives the key).

### Work package D — Mode-aware title row

Intent: the frame never advertises a key that the current mode cannot deliver.

Required changes: hint by mode; re-bank the I4 row if the measurement moves.

Acceptance evidence: an Insert-mode frame does not advertise `i`/`Shift+X` as verbs.

## 8. Failure, cancellation, restart, and contention semantics

A history request that fails leaves the entry `Error`, which `needs_refresh` will retry on the next selection change; it must not leave a permanent loading pane. Rapid `j` presses must not issue concurrent fetches — `needs_refresh` already suppresses while loading, and the test asserts one request per project. Route exit cancels workspace work as today (`leave_workspace_view` cancels `TuiTaskKind::Workspace`); a completion for a left route is dropped by the existing stale guard.

## 9. Compatibility and migration

No storage, protocol, config or DTO change. The panel's binding keys and the channel derivation are untouched.

## 10. Required tests

### Focused unit tests

Add to the workspace route tests (`commands/workspace_dashboard.rs`) with a fake `CoreClient` that captures requests:

1. `moving_to_an_uncached_project_asks_for_its_history` — one request, the right project, and no request on the second visit while loading.
2. `filtering_to_another_project_rebinds_the_chat` — filter text changes the selected row, the binding follows, and the request is issued.
3. `a_bound_project_without_an_entry_paints_loading_not_unavailable` — pane text assertion.
4. `enter_in_filter_mode_reveals_the_selected_row` — the reveal state is set and the filter text is untouched.
5. `tab_in_filter_mode_is_not_consumed` — the handler declines and the filter text is unchanged.

Keep `tui_side_panels::workspace_focus_mirrors_which_side_owns_the_keyboard` green.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --lib workspace_dashboard
cargo test -p codegg-tui --lib chat
cargo test -p codegg-tui --test tui_render          # unfiltered; eight known reds only
scripts/verify.sh quick
scripts/verify.sh scoped 256b215ac
```

Live receipt (addendum §13.2): register a second project in the isolated environment, `j` onto it, and capture the pane before and after the fetch; then type into the filter until the selected row changes and capture the dock again.

## 12. Documentation updates

`architecture/tui.md` (the shared-chat paragraph), `docs/tui.md` (filter-mode line), and the I4 baseline row if the measured advertisement moves.

## 13. Acceptance criteria

- Moving the selection by keyboard or by filter binds and fetches, and the dock paints the selected project's history.
- A bound project with no entry never paints `Chat unavailable`.
- `Enter` in filter mode reveals the selected row; `Tab` is not swallowed.
- The title row's Insert-mode vocabulary matches what Insert mode does.
- The unfiltered `tui_render` run shows no new failure, and the live receipt shows the two projects' histories swapping.

## 14. Stop conditions

- Stop and report if the history request needs a session (a `run_store_unavailable` on this path): that is M008's scope, not a second fetch mechanism here.
- Stop if the fix would require a new `TuiCommand`, a second chat state type, or a change to the channel derivation.
- Do not remove the `catch_unwind` fence under this plan.

## 15. Closure evidence required

`plans/closure/tui-workspace-central-surface-corrective/007-status.md` with: the unfiltered family run, the five test names and their before/after result, the two-project live receipt path, and the I4 consequence (re-banked or unchanged).

## 16. Handoff notes

- Read `workspace_dashboard.rs:680-695` (the seam) and `workspace_view.rs:192-245` (the half-seam) first; the fix is a delegation, not new plumbing.
- The filter arms are short but the catch-all is the trap: returning `true` for an unhandled key is what swallows `Tab`.
- M008 changes team scope and the detail overlay in the same file; run these two milestones one at a time, M007 first.
