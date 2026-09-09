# Interactive Process Sessions Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/interactive-process-sessions/003-tui-terminal-integration-and-legacy-disposition.md`

Source subsystem roadmap:

- `plans/subsystems/interactive-process-sessions-roadmap.md#M003--TUI-terminal-integration-and-legacy-disposition`

Repository baseline reviewed: `5da1d201efa8024c977bcd77ec35b800f5fbe898`

Implementation commits or pull requests:

- `a6734e96` — feat(interactive): M003 TUI terminal integration and legacy disposition
- This closure commit — `plans: close interactive-process M003, subsystem complete` (closure record, registry/roadmap/plan status updates)

## 1. Executive finding

M003 is complete as a capability: a user can create a real interactive
workspace process from the TUI, watch bounded output, focus the terminal
so keystrokes reach the process, resize/detach/reattach/terminate/remove
it, and recover cleanly from disconnect (reconnecting with scrollback
retained), process exit (typed exited state, input rejected), typed
lag/resync, and daemon restart (handles gone). The TUI owns no
PTY/process state — every mutation is a daemon M002
`CoreRequest::InteractiveProcess*` operation and every view change is a
projection of an M002 answer. The misleading deferred `terminal` model
tool was dispositioned by retention-with-truthful-description (removal
would break demonstrated supported consumers without migration — a plan
stop condition): the historic name stays registered so stored runs,
session-import redaction, permission modes, and agent deny-lists keep
resolving, while the description now says one-shot non-interactive with
`bash` canonical and human interactive terminals owned by the TUI over
the M002 protocol. `shell_session` received the remaining truth pass
(metadata-only, not the PTY owner); its full removal stays with the
residual-runtime-consolidation roadmap per the handoff notes. The
subsystem roadmap M001–M003 is now fully closed.

## 2. Requirement-to-evidence matrix

| Requirement (plan §) | Evidence | Result | Notes |
|---|---|---|---|
| TUI terminal reducer/view + bounded scrollback over M002 (§7A) | `src/tui/interactive_terminal.rs`: `InteractiveTerminalController` + `BoundedScrollback` (64 KiB window = M002 default read; ≤16 views = per-client attachment cap); `render_lines`/`header_lines` | pass | 19 controller unit tests |
| Input/focus/resize/create/attach/detach/terminate commands + key-state tests (§7B) | `src/tui/commands/interactive_terminal.rs` (11 `/terminal-*` slash commands, `CoreClient` + `spawn_registered_tui_task` + `terminal_request` stale guards); `classify_key` focus gate; `Terminal` dialog + `TerminalShow` info type | pass | 8 command unit tests incl. 3 stale-completion tests |
| Reconnect/process-exit/lag/resync UX + multi-project routing (§7C) | `TerminalLinkState::{Live,Reconnecting,ResyncRequired,Exited,Gone}`; `note_transport_disconnect` wired into `on_projection_reconnect`; workspace from active session binding (no ambient cwd); detach-on-close unless explicit terminate | pass | E2E disconnect/restart + routing fixtures |
| `terminal` tool + shell-session census/disposition (§7D) | Census table (§7); name retained, description truthful one-shot; `shell_session` skill + arch truth pass; residual M001 keeps full-removal ownership | pass | Tool-surface test pins registration + description + redaction |
| User docs/help + end-to-end fixture (§7E) | 11 slash registry entries + help-overlay entries + `architecture/tui.md` section + ownership-doc rows; `tests/interactive_terminal_tui.rs` | pass | 6/6 e2e (real PTY) |
| Interactive command requiring input (§10) | `tui_terminal_full_lifecycle_over_cat`: input through `cat` echoes into scrollback | pass | Real PTY, ~10 s |
| Resize (§10) | Daemon resize echo recorded + controller queue validates/coalesces | pass | Unit + e2e |
| Focus/key escape (§10) | Unfocused keys ignored; `Esc` escapes focus, never forwards, never submits | pass | Unit + e2e gate test |
| Project/workspace routing (§10) | Two-workspace fixture; session-binding resolution refuses unbound sessions | pass | E2E + unit |
| Detach/reattach (§10) | Detach keeps process + scrollback; re-attach resumes from cursor | pass | E2E |
| Process exit (§10) | Terminate → exited state + headers; input rejected; post-mortem readable | pass | E2E |
| Lag/resync (§10) | `CursorAhead` + `HandleGone` typed fixtures; gap-replacement unit test | pass | E2E + unit |
| Daemon disconnect/restart (§10) | Disconnect → reconnecting + re-attach; restart → gone | pass | E2E |
| Model tool registry/disclosure after disposition (§10) | Tool-surface test: registered, one-shot description, deferred, bash core, history redaction intact | pass | Sync test |
| No observer/projection raw terminal coupling (§10) | Controller imports only protocol + M001/M002 bound constants; headers test asserts no output bytes in headers | pass | Structural + unit |

## 3. Production implementation evidence

- **State controller** (`src/tui/interactive_terminal.rs`, new):
  `TerminalFocus` (Hidden/Viewing/Focused; views open Viewing so
  stray keys stay on the prompt), `TerminalLinkState`
  (Live/Reconnecting/ResyncRequired/Exited/Gone),
  `BoundedScrollback` (newest 64 KiB + `base_seq`/`next_seq`; a
  gapped/discontinuous chunk replaces the window with a history-expired
  notice, mirroring typed M002 resync), per-view coalesced
  pending-input (32 KiB) and last-wins pending-resize (1..=1000, mirroring
  `MAX_PTY_DIMENSION`), `InteractiveTerminalController` (≤16 views,
  active handle, workspace routing, typed `TerminalInputError`/
  `TerminalResizeError`), framework-neutral `TerminalKey`/`classify_key`
  (`Esc` → `EscapeFocus` always; unfocused → `Ignored`; focused
  printable/Enter(`\r`)/Backspace(DEL)/Tab/arrows/Ctrl-C/Ctrl-D →
  `Forward`). No session/projection/observer/model imports.
- **Command layer** (`src/tui/commands/interactive_terminal.rs`, new):
  `start_terminal_{create,list,attach,resume,input-flush,resize,detach,terminate,remove}`
  through `CoreClient` with `spawn_registered_tui_task` and
  `terminal_request.begin()` generations; `apply_*` completions guarded
  by `finish`/`fail`; `handle_terminal_show`/`refresh_terminal_dialog`
  (InfoDialog `TerminalShow`); `focus_terminal`/`close_terminal_view`
  (detach unless explicit terminate)/`handle_terminal_key`
  (`i` focuses, `Esc` unfocuses then closes-with-detach; prompt never
  submitted); `terminal_error_hint` (gone → `/terminal-list`,
  foreign/unknown attachment → re-attach, post-exit → rejected).
  Workspace resolves from the active session's canonical binding only;
  unbound sessions get an actionable error (no `current_dir` fallback).
- **TUI wiring** (`app/mod.rs`, `commands/mod.rs`, `command.rs`,
  `runtime/command_dispatch.rs`, `components/{component.rs,dialogs/info.rs}`,
  `app/{types.rs,state/dialog.rs}`, `input.rs`): `Dialog::Terminal` +
  `DialogType::Terminal` + `InfoType::TerminalShow`, `App` field
  `interactive_terminals`, `DialogState::{terminal_dialog,
  terminal_detail_handle, terminal_request}`, 11 `/terminal-*` slash
  commands + dispatch arms, 6 completion `TuiCommand` variants,
  `handle_dialog_key` interception, `close_dialog` teardown,
  `on_projection_reconnect` terminal-disconnect note, TUI-stats lines,
  help-overlay entries.
- **Legacy disposition** (`src/tool/terminal.rs`): historic `terminal`
  name retained; struct docs + `description()` now say one-shot
  non-interactive via `ManagedProcessService` with `bash` canonical and
  human interactive terminals in the TUI over the M002 protocol.
- **Docs/skills**: `architecture/tui.md` (Interactive Terminals M003
  section + dialog/registry updates), `architecture/
...[truncated 7373 chars]