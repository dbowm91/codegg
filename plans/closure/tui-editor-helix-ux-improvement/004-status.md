# TUI Editor & Files Panel Helix UX Improvement M004 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M004)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

A byte range cannot say "this row is selected" when the row has no
bytes: an empty line under a line selection has a zero-width range, so
nothing was painted and the reader pressed `x` and saw nothing happen.

`editor::line_selection_covers_line` answers the **row-level** question, and
the renderer paints the whole row's background for a line-selected row. A
character selection still highlights only its bytes.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| an empty line under a line selection renders the selection background across the row | met | `a_line_selection_paints_an_empty_row_across_its_whole_width` (`src/tui/components/editor.rs`) |
| a character selection still highlights only its bytes | met | `a_character_selection_still_highlights_only_its_bytes` |

## 3. Residual risk and open items

The row-level answer is a **separate predicate**, not a widening of the
byte range: a line selection still resolves to the same bytes, and the row
paint is a rendering decision layered on top. `line_selection_covers_line`
lives in `src/tui/editor.rs` so the question has one owner.
