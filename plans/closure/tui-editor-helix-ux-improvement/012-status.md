# TUI Editor & Files Panel Helix UX Improvement M012 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M012)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

The composer's hint row no longer carries the `COMP` and `NOR`/`INS`
prefix — the badge directly above the input box already says it — and the
hint row is **removed** while the editor is the active surface, because the
editor has a keymap of its own and two surfaces claiming the same keys is how
a keymap becomes a lie.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the hint row carries keys only | met | `src/tui/app/mod.rs` — the `COMP`/`NOR`/`INS` prefix is gone |
| the badge above the input box carries the mode | met | `src/tui/app/render.rs` |
| the hint row is absent while the editor is active | met | `src/tui/app/mod.rs` — the row is removed, not emptied |

## 3. Residual risk and open items

The hint row is **removed**, not emptied: an empty row still takes a
row, and the editor's route wants the transcript's height.
