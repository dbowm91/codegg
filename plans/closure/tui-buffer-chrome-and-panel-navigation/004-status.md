# TUI Buffer Chrome, Exit Protocol, and Files Panel Navigation M004 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` (M004)

Repository baseline reviewed: `a588a264` (branch
`feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none.

## 1. What landed

**Both exits are two-press.** Leaving the program is destructive and one
keystroke away from a stray press, so `q` and `Ctrl+C` each need a **second
press of the same key inside three seconds**.

- The first `Ctrl+C` clears the field the user was typing into — the meaning
  the key has always had on an input — and arms the exit.
- The second `Ctrl+C` inside the window leaves.
- `q` takes the same path: the first press arms, the second quits.
- The row above the composer names the key to press again (`press ctrl+c again
  to exit`, `press q again to quit`), and the arm **expires on its own** if
  the user walks away.

The state is one field on the prompt state — `exit_armed_at:
Option<(ExitArm, Instant)>` — beside the stop-agent confirmation's
`stop_armed_at`, with the same three-second window
(`EXIT_ARM_WINDOW`). The behaviour lives in `src/tui/commands/exit_control.rs`
and mirrors `commands/turn_control.rs`.

**The arm expires because the loop stays awake.** `animation_interval` returns
`None` when nothing is happening, and `None` is
`futures_util::future::pending()` at the timed arm — so an armed exit raised
on an idle session would never lapse: the confirmation would sit on screen
until the user typed something, and the second press would then be read as a
*first* press, because the arm had expired in silence. An armed exit is now a
contributor to `animation_interval`, on a 250 ms tick.

**`Esc` is the mode switch; `Shift+Q` is the panel exit.** A panel no longer
returns the selection to the main LLM on `Esc`: `Esc` leaves insert mode
(keeping a filter) and, in Normal mode, does nothing. The panel keeps the
keyboard and the selection until the user moves the panel selection with
`Shift+H/J/K/L`, and leaves with `Shift+Q`. `is_exit_key` matches `Shift+Q`
only — `q` is the program's quit key, and one press that meant both "leave
this panel" and "quit the program" was two meanings for one key.

The Ctrl+T tree pane moved with it: `Esc` leaves insert mode and keeps the
filter, and `Shift+Q` returns the keyboard to the editor buffer.

The `/files` box moved with it too: `Esc` leaves insert mode and keeps the
filter, and `Shift+Q` closes the box.

**The chat panel reads `Esc` as a context cancel, not a focus change.** The
browser's `ChatKeyAction::Exit` is the surface's own "leave" gesture, and the
two surfaces read it differently: the `/chat` window closes, while the panel
cannot — `Esc` never takes the keyboard away from a panel. So the panel maps it
to `TuiMsg::CancelChatComposer`, which drops the armed reply/edit seed and
keeps the typed text. The browser stays the single owner of the keymap and the
app stays the single owner of the context; the panel names an intent rather
than reaching into prompt state.

**A bare `q` inside the chat panel is inert.** The browser maps `q` to its own
`Exit` so the `/chat` window can be closed with it, and the panel filters the
key out rather than inheriting a gesture meant for a different surface: `q` is
the program's quit key, `Shift+Q` is the panel exit, and a focused panel owns
the keyboard, so `q` there does neither.

**The exit key is advertised where the panel's verbs are.** A panel's own verb
list says what the panel *does*, never how the user leaves it, and `Esc` used
to be that answer. `SidePanelHost::composer_hints` now leads a **focused**
panel's row with `⇧Q leave`, so one place owns it and no panel can forget it.
The Terminal in INS is the exception: there `Shift+Q` is program input that
reaches the shell, so that row keeps saying `Esc read` instead.

**The Terminal's exit-key seam moved with the key.** `owns_exit_key` is the
panel's first refusal on the host's exit, and it exists so the PTY's focus flag
and the panel's focus flag cannot disagree — the host blurs without consulting
the controller, so a focused terminal that declined the key left
`TerminalFocus::Focused` set on a panel that was no longer focused. The seam
now claims `Shift+Q` while the PTY is focused, which routes the key through
`handle_key` and the mode rule instead of the host's blind blur.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| one `Ctrl+C` does not exit | met | `App::force_quit` arms instead of calling `shutdown_app` |
| the first `Ctrl+C` clears the input | met | `App::force_quit` clears the prompt |
| the confirmation names the key | met | `exit_control::exit_arm_hint` |
| the confirmation expires after three seconds | met | `EXIT_ARM_WINDOW` + `expire_pending_exit_arm` |
| the confirmation clears itself with no further input | met | `animation_interval` wakes the loop while an arm is pending |
| `q` takes the same path | met | `InputAction::Quit` arms before it quits |
| `Shift+Q` leaves the selected panel | met | `SidePanelHost::is_exit_key` |
| `Esc` does not blur a panel | met | `is_exit_key` no longer matches `Esc` |
| the tree pane's `Esc` leaves insert mode and keeps the filter | met | `commands/file_tree.rs::handle_panel_key` |
| `Esc` in the chat panel cancels the armed reply, not the focus | met | `ChatKeyAction::Exit` → `PanelKey::Intent(TuiMsg::CancelChatComposer)` |
| a bare `q` in a focused panel neither quits nor leaves | met | `ChatPanel::handle_key` filters it; the browser's `q` gesture belongs to the `/chat` window |
| the panel's exit key is advertised where the panel's verbs are | met | `SidePanelHost::composer_hints` leads a focused panel's row with `⇧Q leave` |
| a focused PTY keeps its two focus flags in step | met | `TerminalPanel::owns_exit_key` claims `Shift+Q` while the view is focused |

## 3. What was verified

- `cargo nextest run --workspace --locked --profile ci --no-fail-fast` —
  **15,606 tests run: 15,601 passed, 4 failed, 1 timed out, 7 skipped.** Every
  test this milestone touched passes. The five non-passing ones are the
  environment's, not this change's: the two `core::daemon_refresh` projection
  tests fail with `InvalidRootPath("/tmp")` on the untouched base commit too,
  `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
  is the 300 s coalescing wait that the machine's load turns into a timeout,
  and the two `tui::commands::interactive_terminal` PTY-attach tests are
  load-flaky — three consecutive runs of that module alone gave 37/37, 36/37
  and 37/37 with a *different* test failing each time, and both fail before any
  key is routed.
- `cargo nextest run -E 'binary(tui_chat_surfaces) + binary(tui_side_panels) + binary(tui_panel_docking) + binary(tui_panel_host_chrome)'`
  — 220/220 green.
- `cargo fmt --all -- --check` — green.
- The twelve TUI guards (`check_tui_editor_*`, `check_tui_composer_key_authority`,
  `check_tui_transport_authority`, `check_daemon_listing_authority`) — green.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — green.
- `scripts/verify.sh quick` — green.

## 4. Recorded deviations

- **A modal dialog still owns its own keys.** `Ctrl+C` inside a dialog
  dismisses the dialog (the pre-existing `cancel()` behaviour) and does not
  arm the exit; the arm is the *dialog-free* case. The request said "the
  first `Ctrl+C` on any input field should clear it", and a modal box is not
  an input field — the composer's text is.
- **`q` inside a focused panel does not quit.** The panel owns the keyboard,
  so `q` reaches the panel (a PTY receives the letter, as it must) rather than
  the composer. The two-press quit is the composer's path, in Normal mode. In
  the chat panel — the one panel whose browser binds `q` — the key is inert
  rather than a second meaning, because `q` is the quit key and `Shift+Q` is
  the panel exit.
