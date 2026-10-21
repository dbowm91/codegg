# TUI Editor & Files Panel Helix UX Improvement M013 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M013)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`space ?` toggles a **keymap box** in the editor's bottom-right corner,
listing the editor's own verbs. It used to open the `:` command palette —
but the `:` line *is* the command matcher, and the palette was a second way
to reach the same commands, so the key that was supposed to answer "what
can I press here?" answered a different question. `Esc` closes the box.

The box is a floating overlay: it does not change the text area's height, so
opening it cannot move the caret's screen row.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `space ?` opens and closes the box | met | `space_question_mark_opens_the_editors_keymap_box`; `toggle_editor_keymap` |
| the box lists the editor's own keys | met | `EDITOR_KEYMAP` in `src/tui/components/editor.rs` |
| `Esc` closes it | met | `src/tui/commands/editor.rs` — the `Esc` arm |
| the box is a floating overlay that does not change the text area's height | met | `render_keymap_overlay` |

## 3. Residual risk and open items

The box is driven by `EditorPendingEffect::OpenKeymap` rather than by a
new state field: a pending effect is an existing mechanism, and a new field
would have been a second way to say "something is pending".
