# M005 — /worktree navigates into /workspace: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- `/worktree [<selector>]` opens the route, expands lineage, selects + focuses the target — `begin_selector` + lazy sequential `resolve_pending_selector` over cached projections (bounded: 256-char selector, per-page dashboard bounds, no fan-out); unknown selectors render a typed in-panel notice and stay on the route.
- No toast-or-dialog from this command: legacy `dispatch_worktree`/`WorktreeList` toast-or-dialog path removed from this dispatch; `InfoType::WorktreeList` no longer reachable from `/worktree`.
- Catalog-ownership guard passes; `HTTP /api/projects*` untouched.
- Live receipt: bare `/worktree` lands on the full-size route (frame captured); selector matching proven by unit tests (`worktree_selector_matches_workspace_name_without_case`, `worktree_selector_matches_cached_branch_and_selects_its_row`, `unknown_worktree_selector_stays_as_an_in_panel_notice`).

## Commands run

Same gate set as 001-status (64 lib + 19 render workspace tests; quick passed; fmt/diff clean).
