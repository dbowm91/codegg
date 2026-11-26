# TUI Panel Reachability C001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-panel-reachability-corrective/001-panels-load-and-work-without-a-session.md`
Repository baseline: `be6d68b18`
Implementation state: committed working-tree change on this branch.

## 1. Executive finding

Panels now load when the directional walk lands on them, Chat works on a fresh
session, the configured placement is the edge the frame draws, and the terminal
attaches a live shell on macOS. The walk had been the only route that skipped
`select_panel`; Chat could not bind without a tab project and never asked for
its channels; the human PTY was held to a Linux-only constrained profile; and
command effects bypassed the pre-routers, so `terminal_attach` was
`unimplemented`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The walk loads its panel | `walking_the_panel_list_loads_the_panel_it_lands_on`; tui-test `Shift+↑` renders the file tree | pass |
| Chat binds from the session project | `the_chat_panel_binds_the_session_project_without_an_active_tab_project` | pass |
| Chat loads its channel | tui-test fresh session renders `#general 0/0` and "press i to write" | pass |
| A viewless panel does not consume keys | `a_focused_chat_panel_in_insert_mode_leaves_typing_to_the_composer` | pass |
| The configured placement is the visible edge | `a_configured_placement_is_the_edge_the_frame_shows`; tui-test renders the column on the left | pass |
| The human PTY follows the platform backend | `the_human_pty_profile_follows_the_platform_backend` | pass |
| Terminal attaches and runs a command | tui-test: `bash-3.2$` prompt, `echo $((40+2))` → `42` | pass |
| Stepping onto Chat arms the chat composer | `selecting_the_chat_panel_arms_the_chat_composer`; tui-test badge `[CHAT]` | pass |
| The `/files` box closes on opening a document | `opening_a_document_puts_the_files_box_away` (handoff test); tui-test: box gone, editor visible | pass |
| The terminal opens INS, unfiltered | tui-test: prompt without `i`, `echo $((40+2))` → `42`, `echo $((8*8))` → `64`, Esc → NOR, `i` → INS | pass |
| The human PTY carries no sandbox/profile | source: the create arm fixes `FullHost` + `LegacyUnrestricted`; `write_input` is size-bounded only | pass |
| Pre-routed command effects are served | `terminal_attach` family assertion in `daemon_commands` tests | pass |

## 3. Verification executed

- `cargo test -p codegg-tui --test tui_render` — 1252 passed.
- `cargo test -p codegg-tui --lib` — 3208 passed.
- `cargo test -p codegg --lib daemon_commands` — 20 passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — exit 0.
- `scripts/verify.sh quick` — passed.

## 4. Non-claims

- The terminal was verified on macOS only, with the default `workspace_write`
  preference. Linux constrained PTY behaviour is unchanged and was not run here.
- The shell's own `bash: : command not found` line comes from the user's shell
  startup files, not from the spawn.
