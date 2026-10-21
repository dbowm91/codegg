# TUI Editor & Files Panel Helix UX Improvement M014 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M014)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

The composer badge reads `[SEL]` while the editor is in select mode,
in the warning colour and bold, so it is visibly different from `[NOR]` and
`[INS]` rather than a third string in the same style.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the badge reports `[NOR]`, `[INS]`, and `[SEL]` | met | `src/tui/app/render.rs` — the composer badge |

## 3. Residual risk and open items

The badge is the app's, not the editor's, and it reads the editor's
mode — the editor route keeps one keyboard axis (`ui_state.input_mode`), and
`EditorMode::Select` is a **verb mode** inside that route rather than a
second focus axis.
