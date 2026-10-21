# TUI Editor & Files Panel Helix UX Improvement M019 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M019)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

After a file opens successfully, the tree's focus is released
(`file_tree::set_focus(app, false)`) and the editor is put in
`EditorMode::Normal` (`commands::editor::enter_editor_normal`).

The handoff runs on the **success** path, not the request path: handing the
keyboard to an editor whose document has not attached yet would show the
reader an empty buffer.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| a successful open releases the tree's focus and forces Normal mode | met | `opening_a_file_hands_the_keyboard_to_the_editor_after_the_open_succeeds` |
| a failed open leaves the tree focused | met | `a_failed_open_keeps_the_keyboard_in_the_tree` |

## 3. Residual risk and open items

The handoff is `complete_editor_open_handoff`, shared by the `/files`
box and the Ctrl+T pane, so the two surfaces cannot disagree about who holds
the keyboard after an open.

The completion is **matched to the open the tree asked for**: a second open
elsewhere supersedes it, and then the tree does not steal the keyboard back
(`complete_editor_open_handoff` compares the completed path with the
editor's own). The editor is put in Normal and not merely "not Insert": an
editor left in Select would keep extending a selection that no longer means
anything once the tree moved on.
