# TUI Editor & Files Panel Helix UX Improvement M006 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M006)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`EditorCommand::PageDown` / `PageUp` are their own commands rather than
aliases of the half-page verb, so the key's name is no longer a lie. A page
moves the **view** by a page and the caret with it, keeping the caret's
screen row; a half page moves both by half.

The page is measured against the **real** pane height (`motion_height`),
with a fallback for the window before the first frame. The caret's
discontinuity at the end of the document is unchanged: a page that would land
past the end continues to the end.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `PageDown` and `Ctrl+d` are distinct commands with distinct distances | met | `EditorCommand::{PageDown, PageUp}`; `page_target` vs `half_page_target` |
| a page is measured against the real pane height, not a fixed guess | met | `motion_height`; `a_page_before_the_first_frame_uses_the_fallback_and_stays_on_the_page` |
| the caret keeps its screen row across a page | met | `half_page_down_continues_to_the_last_line` (cursor-anchored) |

## 3. Residual risk and open items

`half_page_target` is **cursor-anchored** — it moves the caret by half a
page rather than by a fixed row count — which changed an existing test's
expectation; the test was rewritten to the new contract rather than the
contract bent to the test.
