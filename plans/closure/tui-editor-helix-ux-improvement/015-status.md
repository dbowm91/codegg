# TUI Editor & Files Panel Helix UX Improvement M015 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M015)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`FileTreeState::reveal_selected` clamps the scroll offset against the
**measured** window height rather than the row count, and `page_selection`
falls back to `FALLBACK_PAGE_ROWS` when no height has been measured instead
of paging by zero. `Ctrl+d` in the file tree therefore moves the listing by
a page and keeps the selection on screen.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the scroll offset never exceeds `total - viewport_height` | met | `the_scroll_offset_never_leaves_the_measured_window` |
| paging with no measured height uses the fallback | met | `FALLBACK_PAGE_ROWS` |

## 3. Residual risk and open items

The viewport height is **measured by the renderer** and handed to the
state; the state does not measure it, so there is one owner of the number.
