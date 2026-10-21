# TUI Editor & Files Panel Helix UX Improvement M011 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M011)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

The `:` and `/` lines take the **bottom row** — the path is erased while
either is open, because the reader is looking at what they are typing — and
the `:` command's fuzzy match list renders as a **floating overlay** on the
row above.

The overlay is an overlay rather than an allocated row on purpose: allocating
one changes the text area's height, which changes the visible line count,
which re-resolves the viewport and can move the caret's screen row under the
reader's first keystroke.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the `:` line renders on the bottom chrome row | met | `render_chrome`'s `chrome_input` |
| the path is not drawn while it is open | met | `chrome_input` returns the modal line, and the cluster is not drawn |
| the match list is drawn above it | met | `render_command_matches` |
| the text area's height does not change when the line opens | met | `CHROME_ROWS` is a constant |

## 3. Residual risk and open items

A modal line **owns the whole chrome row**: the path is erased and the
cluster is not drawn, because a cluster of status beside a line the reader is
typing into is a second answer to a question nobody asked.
