# TUI Editor & Files Panel Helix UX Improvement M003 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M003)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`EditorMode` gains `Select`. `v` enters it and arms a character
selection; `v` again and `Esc` both leave it and drop the selection.

Outside Select mode a movement **clears** the selection; inside it a movement
**grows** it. The selection's head is always the caret, so the two rules are
one rule about the same span: a movement that lands somewhere new either
replaces the span or extends it, and the reader is never left with a
highlight that outlived the movement that made it.

`Select` is a **verb mode, not a focus axis**. The editor route keeps one
keyboard axis (`ui_state.input_mode`); `Select` changes what a key means
inside the editor and nothing about who holds the keyboard.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `v` shows `[SEL]` | met | `src/tui/app/render.rs` — the composer badge reports `[SEL]` |
| a movement in Normal mode clears the held selection | met | `src/tui/editor.rs` — `apply_motion_selection`; `move_cursor_to_reveals_the_target_and_drops_a_held_selection` |
| a movement in Select mode grows it | met | `movement_extends_a_char_selection_but_not_a_line_selection` |
| `Esc` leaves Select and clears the selection | met | `src/tui/commands/editor.rs` — `leave_editor_select` |
| the badge reports all three states | met | `src/tui/app/render.rs` — `[NOR]` / `[INS]` / `[SEL]` |

## 3. Residual risk and open items

A **line** selection does not extend on a movement: extending a line
selection would have to mean extending by whole lines, which is a different
verb from extending a character selection. The rule is explicit
(`line_selection_covers_line` answers the row-level question separately) and
is asserted by `movement_extends_a_char_selection_but_not_a_line_selection`.
