# TUI Editor & Files Panel Helix UX Improvement M008 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M008)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`EditorCommandName::GotoLine` is in the `:` registry, and
`parse_editor_command` recognises a **numeric** line *before* it looks for a
name. `complete_editor_command` short-circuits the fuzzy matcher for a
numeric query — otherwise `:5` offers `split-regex` and `complete` ahead of
the line jump. The line number is one-based, and `:0` is refused rather than
clamped to line one.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `:50` and `:goto 50` resolve to the same command | met | `src/tui/editor.rs` — `parse_editor_command`'s numeric branch |
| a numeric query offers only the line jump | met | `complete_editor_command`'s numeric short-circuit |
| the caret lands on the requested line | met | `goto_editor_line` (`src/tui/commands/editor.rs`) |
| `:0` is refused with a notice | met | `goto_editor_line` |

## 3. Residual risk and open items

A general **count prefix** (`123G`) is deferred and listed as a
non-goal; `:50` and `:goto 50` answer the same intent and were built.
