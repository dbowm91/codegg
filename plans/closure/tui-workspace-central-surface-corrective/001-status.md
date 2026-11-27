# M001 — Route composer and key fall-through: closed

Status: closed. Implementation branch `feat/auto-20261126-cae46b00`.

## Requirement-to-evidence matrix

- Printable typed with chat focus lands once in the visible route input; badge names destination first — `render_workspace_frame` paints the shared `prompt_state.prompt` with destination badge; `focus_workspace_composer` arms a plain `ChatComposeSeed::say()` so `input_target()` is Chat and Enter delivers via `route_composer_to_chat`; `leave_workspace_view` clears the route seed and restores entry mode. Contract tests: `workspace_focus_routes_composition_once_and_restores_the_session_mode` (updated: asserts Chat target + seed while focused, seed cleared + Build target after leave). Live receipt: isolated HOME/DATA_HOME 150x44 session, `/workspace` then Shift+H shows `[INS][CHAT]` badge; typed text delivered into the Chat panel as the bound project message (not a `provider_not_found` turn error).
- No key is both tree verb and text in one mode — `handle_workspace_view_key` returns false for Composer/Panel/MemberForm focus (Esc still owned); Insert-mode filter owns only its keys; tree verbs own Normal; test `filter_mode_is_separate_from_composer_and_tree_verbs` plus fall-through assertion in the focus test (`n` does not open launch while composing).
- Exit restores the session composer; no daemon work cancelled — leave clears seed, restores `return_composer_mode`, cancels only `TuiTaskKind::Workspace`, generation bump drops late completions.

## Commands run

- `cargo test -p codegg-tui --lib workspace` — 64 passed.
- `cargo test -p codegg-tui --test tui_render -- workspace` — 19 passed.
- `python3 scripts/check_test_assertion_quality.py` / `check_test_duplication.py` — pass.
- `python3 scripts/check_command_catalog_ownership.py` — clean.
- `scripts/verify.sh quick` — passed (after comment-budget trim, no ceiling change).
- `cargo fmt --check`, `git diff --check` — clean.

## Receipts

- Live frames: `/workspace` full-size with Chat Right and bottom composer; `[INS][CHAT]` badge after Shift+H; delivered chat message visible in Chat panel; `/worktree` (no selector) lands on the route with no toast-or-dialog. Receipt env: isolated HOME + CODEGG_DATA_HOME, remote-backed repo, 150x44.
