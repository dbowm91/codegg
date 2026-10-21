# TUI Editor & Files Panel Helix UX Improvement M009 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M009)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

The editor widget renders a scrollbar in the rightmost column of the
text area. The column was already paid for — `text_columns = area_width -
gutter - 1` — and nothing ever drew in it.

The thumb is sized by how much of the document is visible and placed by where
the view **starts**, so it is a map of the document rather than an indicator
of the cursor: a reader who has scrolled away from the caret still sees where
they are. A document that fits on screen still shows a thumb.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the scrollbar renders in the reserved column | met | `src/tui/components/editor.rs` — `render_scrollbar` |
| the thumb is sized by how much of the document is visible and placed by where the view starts | met | `render_scrollbar` |
| a document that fits on screen still shows a thumb | met | the thumb is clamped to at least one row |

## 3. Residual risk and open items

The scrollbar is a **reserved column that is now painted**, not a new
column: the text area's width is unchanged, so no line re-wraps because of
it.
