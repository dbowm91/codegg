# TUI Editor & Files Panel Helix UX Improvement M018 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M018)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`display_depth` renders the indent **relative to the scope**. Under peek
the absolute scope depth is unbounded while the relative depth stays within
0-2, so the absolute clamp flattened exactly the deep scopes peek is designed
to reach. The scope's absolute depth is carried by the breadcrumb in the
title instead.

`display_depth` reads state and never writes it: it is a display concern,
and if it started changing expansion or selection it would have become
navigation.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `display_depth` is relative to the scope | met | `display_depth_is_relative_to_the_scope` |
| the two existing tests that assert the old absolute contract are rewritten to the new one | met | `src/tui/components/dialogs/files.rs` — `a_scoped_listing_draws_its_rows_one_level_in` |

## 3. Residual risk and open items

The rewritten test asserts the **new** contract rather than dropping
the assertion: a scoped row and a root row are drawn at the same column,
because the indent is relative to the scope. The scope's absolute depth is
in the breadcrumb.
