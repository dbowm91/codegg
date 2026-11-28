# M008 — Workspace scope without a session: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- The team request names a workspace even when no session is bound: `start_show_team` (`commands/team.rs:302`) derives `workspace_id` from `reveal_workspace_id` or from the selected row and puts it in the command scope, instead of depending on a session-derived identity that the route does not have.
- One inline section at a time: `prepare_workspace_section(WorkspaceSectionKind)` clears the competing section at each opener (Detail, Team/reveal), so the two `Option` fields can never both paint authority-bearing content. Exclusivity is enforced at the openers, not smeared through the renderer.
- A hidden selection does not move: `workspace_section_is_open` blocks `move_selection` and `select_edge` while a section is open, so `j`/`G` cannot retarget a row behind an open section.
- Leaving the route clears the in-flight team load, so a late completion has nothing to write into.

## Tests

`the_team_request_names_the_selected_rows_workspace`, `t_replaces_an_open_detail`,
`j_does_not_move_a_hidden_selection`, `a_failed_team_load_clears_loading`,
`a_late_team_completion_after_leaving_the_route_changes_nothing`.

Note on the last one: the plan named it `leaving_the_route_clears_loading`. The view is dropped when the route closes, so `team_loading` is unobservable afterwards and that assertion could not fail. It is renamed and re-scoped to what is observable — a completion arriving after the leave changes nothing.

Note on `j_does_not_move_a_hidden_selection`: the first version pressed `j` then `k`, which cancel out and so passed with the guard removed. Rewritten to assert `j` and `G` separately against the selected index; it then failed without the guard.

## Commands run

- `cargo test -p codegg-tui --lib workspace_dashboard` — 52 passed.
- `cargo test -p codegg-tui --lib team` — 11 passed.
- `cargo test -p codegg-tui --test tui_render` (unfiltered) — 1247 passed / 9 failed.
- Live receipt: the route paints its tree and dock with no session bound and no `run_store_unavailable` on the route — `plans/closure/tui-workspace-central-surface-corrective/live-receipt-006-010.md`, frame `receipts/m006-route-surface.png`. The `t` team path was not driven live.
