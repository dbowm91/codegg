# TUI Buffer Chrome, Exit Protocol, and Files Panel Navigation Roadmap

Status: active (M001–M004)

Long-term references:

- `plans/000-long-term-specification.md`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Authorizing predecessor: `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md`
(closed). This roadmap does **not** reopen it: that line's milestones are
closed and its contracts are superseded here only where §2 says so, in
writing, with the superseded text named.

Research artifact: `research/tui-files-panel-depth-and-input-chrome-2026-10-21.md`
— the surface map, the defect list with `path:line` anchors, the depth
arithmetic worked through the reported example, and the open decisions this
roadmap answers.

## 1. Purpose and ownership boundary

Give the main buffer back the vertical space its status row spent, put the
information that row carried somewhere it belongs, make leaving the program
and leaving a panel two deliberate gestures instead of one accidental one, and
make the `/files` panel a navigator that reaches any depth and can be
filtered.

**This subsystem owns** the main buffer's strip layout, the row above the
composer, the program's exit gestures, the side-panel and tree-pane focus
exit, and the `/files` panel's navigation verbs, its filter, and its chrome.

**It consumes** the daemon-owned workspace listing service
(`WorkspaceDirectoryList`) and the projection-backed todo/subagent/job
selection.

**It must not own** directory listing authority, text authority, or the
agent's own task bookkeeping. The `/files` panel still never reads the
filesystem: `scripts/check_daemon_listing_authority.py` enforces that, and
this roadmap does not weaken it. The tasks overlay still selects from the
canonical session-work projection and mints no rows of its own.

## 2. Decisions

These were answered by the user after the research artifact's §6.4 open
questions, and they are binding for this roadmap.

| # | Decision |
|---|---|
| D1 | The scope slide lands on the target's **grandparent**, not its parent — the shallowest scope in which the target's children still fit inside the listing window, so the user keeps the widest context |
| D2 | `l` on a **file** opens it in the editor. The key is never dead |
| D3 | The main buffer's "idle" status line is **dropped entirely**, and the subagent/job information moves to the `Ctrl+T` tasks overlay, which already unifies todos, active subagents, and live jobs |
| D4 | `Esc` on any panel does **not** return the selection to the main LLM. `Esc` is the mode switch (Insert ↔ Normal); the panel exit is `Shift+Q` over the selected panel in Normal mode |
| D5 | The bare `q` quit key now takes the **same two-press path** as `Ctrl+C`: two presses within three seconds, with the confirmation on the row above the input |
| D6 | The composer row above the input drops its list of bare verb keys (the `space help` chord already carries their descriptions) and keeps the row open for **situational** information — the `Ctrl+C`/`q` confirmations and the latched turn failure |

### 2.1 Superseded contracts, named

| Superseded text | Where | Replaced by |
|---|---|---|
| "`Enter` expands a directory in place and leaves the cursor alone" | `architecture/tui.md:2721`, `.opencode/skills/tui/SKILL.md:375` | `Enter` is the same motion as `l`: it enters the directory, and slides the scope at the listing window's edge |
| "the scope slides to its parent" | `src/tui/app/state/file_tree.rs` (`PeekStep::Slide`) | the scope slides to the target's **grandparent** (D1) |
| "the working strip's row 0 is reserved unconditionally" | `AGENTS.md`, `architecture/tui.md:643-651`, `.opencode/skills/tui/SKILL.md:993-1001` | the working strip is gone; `Ctrl+T` carries the task state (D3) |
| "`Esc`/`q` are the universal exits" | `src/tui/panels/mod.rs` (`is_exit_key`) | `Shift+Q` is the panel exit; `Esc` is the mode switch (D4) |
| "Ctrl+C exits" / "`q` quits" | `src/tui/input.rs`, `src/tui/app/mod.rs` | both need a second press inside the arm window (D5) |

## 3. Milestones

### M001 — The `/files` panel reaches any depth

**Deliverable.** One motion for "open this row": `l` and `Enter` are the same
verb. A directory is entered; a file is opened in the editor. When the
directory's children fall outside the daemon's listing window, the scope
slides to the target's grandparent, the chain between the two is opened so the
directory is on screen at all, the directory is expanded there, and the
cursor enters it — so the panel is never a three-level dead end, and the
workspace root never changes.

**Exit conditions.**

| Exit condition | Evidence |
|---|---|
| `Enter` on a directory enters it instead of toggling it | `src/tui/components/dialogs/files.rs` — `activate` emits `TuiMsg::FilesDescend` |
| `l` on a file opens the document | `PeekStep::Open` → `descend_to` → `open_editor` |
| the slide lands on the grandparent | `peek_into` → `parent_path(&parent_path(&row.path))` |
| the slid-to directory is on screen | `expand_ancestors_of` opens the chain between the new scope and the directory |
| the scope moves one level **inward**, and never out of the workspace | the target's grandparent is always a child of the current scope, and the scope travels as the listing request's prefix |

### M002 — The `/files` panel filters, and its bottom chrome is quiet

**Deliverable.** `i` opens the panel's filter in the `/files` box as well as
in the Ctrl+T pane; printable keys narrow the rows, `Backspace` deletes, and
`Esc` leaves insert mode **keeping** the filter. The box's own hotkey row is
gone (the row above the composer already carries the panel's keymap), and the
listing no longer reports the depth window as a truncation — the depth window
is a navigational bound the panel slides around, not a loss.

**Exit conditions.**

| Exit condition | Evidence |
|---|---|
| `i` reaches insert mode in the `/files` box | `FilesDialog::handle_key` → `handle_insert_key` |
| `Esc` keeps the filter | `handle_insert_key` — the filter is not cleared |
| the filter narrows loaded rows only | no walk is issued; `FileTreeState::rows()` filters |
| a listing completion does not wipe a filter being typed | the mirror carries the box's `filter` and `panel_mode` across |
| the depth window is not reported as a truncation | `directory_listing::is_depth_stop_reason` |

### M003 — The main buffer's chrome

**Deliverable.** The working strip — its reserved status row and its
per-subagent rows — is deleted from the main buffer. The status information
that mattered moves to the `Ctrl+T` tasks overlay, which already unifies
todos, active subagents, and live jobs; the latched turn failure and the exit
confirmations move to the row above the composer. That row drops its list of
bare verb keys and keeps the `space help` chord.

**Exit conditions.**

| Exit condition | Evidence |
|---|---|
| the main buffer reserves no status row | `working_strip_rows` and `render_working_strip` are gone |
| the transcript gains the row | `prompt_height` no longer adds the strip |
| the latched turn failure is still durable | `situational_hints` carries `✗ {error}` on the row above the input |
| the row names the `space help` chord and no verb list | `composer_hint_row` |
| the `Ctrl+T` overlay still lists todos, active subagents, and live jobs | `src/tui/app/state/tasks.rs` (unchanged) |

### M004 — The exit protocol

**Deliverable.** Leaving the program takes two deliberate presses. The first
`Ctrl+C` clears the field the user was typing into and arms the exit; a
second `Ctrl+C` inside three seconds leaves. The bare `q` takes the same
path, and the row above the input names the key to press again. Leaving a
panel is `Shift+Q` over the selected panel in Normal mode; `Esc` is the mode
switch and never takes the keyboard away from a panel.

**Exit conditions.**

| Exit condition | Evidence |
|---|---|
| one `Ctrl+C` does not exit | `force_quit` arms instead of calling `shutdown_app` |
| the first `Ctrl+C` clears the input | `force_quit` clears the prompt |
| the confirmation names the key | `exit_arm_hint` — "press ctrl+c again to exit" / "press q again to quit" |
| the confirmation expires after three seconds | `EXIT_ARM_WINDOW` + `expire_pending_exit_arm` on the render pump |
| the confirmation clears itself with no further input | `animation_interval` wakes the loop while an arm is pending |
| `q` takes the same path | `InputAction::Quit` arms before it quits |
| `Shift+Q` leaves the selected panel | `is_exit_key` matches `Shift+Q` only |
| `Esc` does not blur a panel | `is_exit_key` no longer matches `Esc` |
| the tree pane's `Esc` leaves insert mode and keeps the filter | `handle_panel_key` |
| `Esc` in the chat panel cancels the armed reply, not the focus | `ChatKeyAction::Exit` → `PanelKey::Intent(TuiMsg::CancelChatComposer)` |
| a bare `q` in a focused panel neither quits nor leaves | `ChatPanel::handle_key` filters it; the browser's `q` gesture belongs to the `/chat` window |
| the panel's exit key is advertised where the panel's verbs are | `SidePanelHost::composer_hints` leads a focused panel's row with `⇧Q leave` |

## 4. Out of scope

- A configurable `editor.scrolloff`, and a general count prefix (`123G`).
- The `Ctrl+T` overlay's own chrome. It already carries the three groups;
  this roadmap moves the main buffer's task state *into* it and does not
  redesign it.
- The modal `/files` box's own keymap surface beyond the verbs it now
  implements. Its hotkey row is removed because the row above the composer
  carries the panel's keymap.
- Any change to the daemon-owned listing contract, its wire budget, or its
  node budget.

## 5. Current state

M001–M004 are implemented in the working tree of
`feat/auto-20261020-513a8ce6`. The verification evidence is recorded in the
closure records under `plans/closure/tui-buffer-chrome-and-panel-navigation/`.
