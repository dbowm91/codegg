# TUI Editor & Files Panel Helix UX Improvement M020 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M020)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

The tree's advertised keymap names the peek motion and the insert-mode
filter, and no longer names a verb that does not exist. The hint string is
one constant (`dialogs::files::HINT`), shared by the box, its hosted panel,
and the help dialog, so the line that tells the reader what the tree can do
is true.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the tree's hint line names only chords the tree handles | met | `dialogs::files::HINT` — `l/→ peek into dir`; `src/tui/panels/mod.rs` — the advertised verbs |

## 3. Residual risk and open items

The hint line is the **single source** for the box, the hosted panel,
and the help dialog: three copies of the truth is how one of them becomes
false.
