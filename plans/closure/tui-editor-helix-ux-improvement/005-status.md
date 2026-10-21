# TUI Editor & Files Panel Helix UX Improvement M005 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M005)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`editor::SCROLL_OFF` is 5 — Helix's own `editor.scrolloff` default — and
the single reveal rule in `reveal_cursor` keeps the caret between row
`scrolloff` and row `H - scrolloff - 1`. The page stands still while the
caret walks the last few rows, so the text the reader is on does not move
mid-sentence. At the two ends of the document the margin gives way, so the
caret can reach the first and last lines. `resolve_viewport` uses the same
rule, so the renderer and the motion layer cannot disagree.

A short window shrinks the margin rather than breaking the rule: the margin
is clamped against the window's own height.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the reveal rule is shared by motions, `/` search, and the renderer | met | `src/tui/editor.rs` — one `reveal_cursor`, reached from `resolve_viewport` |
| a short window shrinks the margin rather than breaking the rule | met | `scrolloff_margin` |
| the caret can sit on the last line at the end of the document | met | `half_page_down_continues_to_the_last_line` |

## 3. Residual risk and open items

The margin is a **documented constant**, not a configuration key. A
configurable `editor.scrolloff` is listed as a non-goal in the roadmap and is
recorded in `research/…` §0.0 as deferred.
