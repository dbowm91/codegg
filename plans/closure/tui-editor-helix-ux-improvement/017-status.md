# TUI Editor & Files Panel Helix UX Improvement M017 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` (M017)

Repository baseline reviewed: `a588a264` (branch `feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none. This milestone adds no
protocol, storage, migration, authorization, or dependency change, and it
reopens no closed milestone of `tui-editor-helix-parity-roadmap.md`.

## 1. What landed

`l` / `Right` on a directory **peeks** into it — expands it in place and
moves the caret into it — rather than re-rooting the pane, which used to
hide the parent and every sibling. At the deepest level the daemon's listing
window allows, the scope slides to the parent and the directory is expanded
there.

`h` / `Left` moves the caret to the parent row and collapses it; on a
top-level row the scope slides up; at the workspace root it collapses in
place, so the key is never dead. These are **directional** motions, not undo
and redo: `l` and `h` are not inverses of each other. The inline
`toggle_selected` verb is retired — `l`/`h` subsume it.

`Enter` on a directory also peeks: the inline toggle is gone, so `Enter` and
`l` are the same motion by two spellings.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| `l` on a directory expands it in place and moves the caret into it | met | `peeking_into_a_directory_expands_it_in_place_and_enters_it`, `peeking_into_a_directory_expands_it_in_place_and_keeps_the_scope` |
| a directory at the window edge slides the scope to the parent | met | `peeking_into_a_directory_at_the_window_edge_slides_the_scope` |
| `h` from inside a directory moves to the parent row and collapses it | met | `leaving_a_directory_moves_to_the_parent_row_and_collapses_it` |
| `h` on a top-level row slides the scope up | met | `leaving_a_top_level_row_slides_the_scope_up`, `ascending_from_a_top_level_row_slides_the_scope_up` |
| `h` at the workspace root collapses the selected directory in place | met | `ascending_at_the_workspace_root_collapses_the_selected_directory_in_place`, `leaving_a_top_level_row_at_the_workspace_root_collapses_in_place` |
| `l` on a file is a no-op | met | `peeking_into_a_file_is_a_no_op`, `peeking_at_something_that_is_not_a_directory_issues_no_request` |

## 3. Residual risk and open items

A scope change is still a **daemon round-trip** scoped by
`WorkspaceDirectoryListRequestDto::prefix`, not a local filter of the loaded
rows — filtering would silently drop everything the first walk hit its bounds
on and present the remainder as if it were the directory.

The peek motion is **not** an undo/redo pair: `l` then `h` returns to a
listing whose *shape* differs from the one before `l`.
