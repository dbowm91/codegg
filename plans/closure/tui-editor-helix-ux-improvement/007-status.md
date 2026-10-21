# TUI Editor & Files Panel Helix UX Improvement M007 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M007)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

The renderer publishes the **measured** text height on the presentation
(`visible_lines`), and the motion layer prefers it over the nominal
fallback. A short pane therefore pages by what it shows rather than by a
fixed guess.

A `z` prefix arms **view mode** with `c` (centre), `t` (top), `b` (bottom),
`j` (scroll down), and `k` (scroll up). The view offset rides on the
presentation (`view_scroll`) and any caret motion re-attaches the view.
`gt` / `gc` / `gb` become bound leaves: the caret moves to the top, middle,
and bottom of the **screen**.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the measured height reaches the motion layer | met | `src/tui/document_session.rs` — `visible_lines`; `src/tui/editor.rs` — `motion_height` |
| a short pane pages by what it shows | met | `a_page_before_the_first_frame_uses_the_fallback_and_stays_on_the_page` |
| `z c` centres the view without moving the caret | met | `EditorCommand::ViewAlign(ViewAlign)` — `view_align_line` |
| `gt`/`gc`/`gb` land on the screen's own edges | met | `EditorCommand::{ScreenTop, ScreenCenter, ScreenBottom}` |

## 3. Residual risk and open items

`gt`/`gc`/`gb` were deliberately **unbound** in the predecessor roadmap
(`tui-editor-helix-parity`) because the motions resolved against a fixed
`NOMINAL_VISIBLE_LINES` guess. They are bound here because the measured
viewport exists; the binding is therefore a consequence of M007, not a
re-spelling of a dead key.
