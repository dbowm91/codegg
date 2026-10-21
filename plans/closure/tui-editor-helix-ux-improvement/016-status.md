# TUI Editor & Files Panel Helix UX Improvement M016 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M016)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`FileTreeState` gains a `filter: String` and a **panel-local** mode
(`TreePanelMode`), distinct from the app-wide `InputMode` — a panel-local
INS must not be reported as the composer's INS, because the composer badge
would then claim an insert mode the reader is not in.

The filter survives the Normal↔Insert transition, because the point of a
filter is that it is still there after the reader stops typing. The panel
renders the filter as an in-panel input box with a caret
(`TreeFilterField`), in one reserved row under the listing.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the filter matches case-insensitively on name and path | met | `the_filter_matches_the_name_and_the_path_case_insensitively` |
| it survives the mode change | met | `the_panel_filter_is_typed_in_insert_mode_and_survives_esc` |
| the panel-local mode does not move the app's `InputMode` | met | `TreePanelMode` is a field of `FileTreeState` |
| the filter is rendered | met | `the_filter_field_draws_the_text_and_the_caret` |

## 3. Residual risk and open items

The filter narrows the **rows the listing already holds** (the
research's §C2 contract: a filtered variant of `rows()` matching the path or
the file name). It is not a daemon-side search, so a match inside a
directory that has not been walked is not found — the panel still has to be
pointed at it with the peek motion first.
