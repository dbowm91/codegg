# M007 — Chat dock follows selection, filter keys: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- One seam, no second copy: `sync_selected_project` now delegates to `sync_workspace_chat_panel` (`commands/workspace_dashboard.rs:685`) instead of hand-setting `chat_panel_project`, so the tree cursor is the only input and the dock cannot disagree with it.
- The filter rebinds the dock: edit/`Backspace` and filter-clear re-run the seam, so narrowing the tree moves the chat to the row that is now selected rather than to the row that was selected before the query.
- `activate_selected_row` is the single activation seam; Normal `Enter` and Insert `Enter` both call it, so filtering and then pressing `Enter` reveals the same row `Enter` would reveal from the tree.
- Unhandled Insert keys `return false` instead of being swallowed, so `Tab` and friends pass through rather than disappearing.
- `panels/chat.rs` paints the dock's real state (Loading / Error / Unavailable) instead of the blanket "Chat unavailable", and the workspace title hint is mode- and focus-aware (`type to filter · Enter reveal · Esc back` while filtering).
- `catch_unwind` untouched (watch item only).

## Tests (each proven red with production reverted, tree restored byte-identical)

`moving_to_an_uncached_project_asks_for_its_history`, `filtering_to_another_project_rebinds_the_chat`,
`a_bound_project_without_an_entry_paints_loading_not_unavailable`, `enter_in_filter_mode_reveals_the_selected_row`,
`tab_in_filter_mode_is_not_consumed`, `workspace_filter_title_advertises_only_insert_mode_keys`.

## Commands run

- `cargo test -p codegg-tui --lib workspace_dashboard` — 47 passed.
- `cargo test -p codegg-tui --lib chat` — 95 passed.
- `cargo test -p codegg-tui --test tui_render` (unfiltered) — 1247 passed / 9 failed.
- Live receipt: the filter mode and its vocabulary (`INS · filter  type to filter · Enter reveal · Esc back`) — `plans/closure/tui-workspace-central-surface-corrective/live-receipt-006-010.md`, frame `receipts/m007-filter.png`. The selection-following rebind needs two projects and was not driven live; it rests on `moving_to_an_uncached_project_asks_for_its_history`.
