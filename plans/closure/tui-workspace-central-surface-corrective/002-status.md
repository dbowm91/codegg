# M002 — Force Chat docked Right on entry: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- Entry docks Chat Right + visible + bound even from closed-Right — `open_workspace_dashboard_inner` docks Chat at Right and sets visible without persistence change, then blurs (tree keeps keys). Fallback `"Chat pane unavailable"` branch removed from the route frame; the panel always paints. Tests: `opening_workspace_preserves_tab_session_and_composer`, entry-convergence tests.
- Selection rebind; drafts preserved — `sync_workspace_chat_panel` refresh-on-stale; `move_dashboard_selection` syncs the view tree cursor to the dashboard cursor and vice versa (`move_selection` syncs back); per-project drafts stay in `ChatState` (postclosure m002 cancellation tests green).
- Live receipt: entry frame shows bound Chat (`#general`) with no manager detour, no fallback string.

## Commands run

Same gate set as 001-status (64 lib + 19 render workspace tests; quick passed; fmt/diff clean).
