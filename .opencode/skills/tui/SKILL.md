---
name: tui
description: Operational guide for changing the terminal UI safely - command registration, sync dispatch, async spawn-and-complete pattern, dialogs, and background task lifecycle
version: 1.0.0
tags:
  - tui
  - commands
  - async
  - ratatui
---

# TUI Module Guide

Operational guide for making changes to `src/tui/`. The full module contract
lives in `architecture/tui.md`; this skill covers the patterns you must follow
to avoid breaking invariants that are easy to violate in a 15K-line `mod.rs`.

## Layout

| Path | Purpose |
|------|---------|
| `src/tui/app/mod.rs` | The `App` struct (~15K lines). State, rendering, event handling. |
| `src/tui/app/types.rs` | `Dialog` enum and app-level types |
| `src/tui/app/state/` | App state helpers; `async_request.rs` holds the finish/fail guard |
| `src/tui/command.rs` | Slash-command registry (`CommandRegistry::built_in_commands()`) |
| `src/tui/commands/` | 19 command-handler submodules (sessions, git_sidebar, research, ...) |
| `src/tui/runtime/command_dispatch.rs` | `dispatch_tui_command(app, cmd)` - maps `TuiCommand` variants to handlers |
| `src/tui/runtime/` | Runtime loop and event routing |
| `src/tui/async_cmd.rs` | `spawn_tui_task` / `spawn_registered_tui_task` |
| `src/tui/task_lifecycle.rs` | `TuiTaskRegistry` - tracks spawned background tasks on `App` |
| `src/tui/components/` | Widgets; `component.rs` has `DialogType`, `focus.rs` has `FocusManager` |

## Adding a New Command

1. Add the variant to the command list in `CommandRegistry::built_in_commands()`
   (`src/tui/command.rs`). A test asserts the exact total (108) - update it.
2. Add a `TuiCommand` variant if the command needs backend work.
3. Handle the variant in `src/tui/runtime/command_dispatch.rs`.

## Dispatch Rules

- **Sync dispatch is the rule**: dispatch arms are all `fn` (non-async). Do NOT
  add `.await` in a dispatch arm.
- High-latency work uses the **spawn-and-complete** pattern instead:
  1. Spawn with `spawn_registered_tui_task(tx, registry, kind, name, fut)`
     (registers with `TuiTaskRegistry` for lifecycle tracking).
  2. On completion, send a completion `TuiCommand` back through the channel.
  3. The completion handler MUST use the stale-completion guard:
     `state.finish(request_id)` / `state.fail(request_id, err)` from
     `src/tui/app/state/async_request.rs`. These return `bool`; a `false`
     return means the completion is stale (superseded request) and must be
     dropped.
  4. Every new apply handler needs a stale-completion test (duplicate or
     out-of-order completions must not corrupt state).

## Dialogs

- `Dialog::Info` does NOT exist even though `components/dialogs/info.rs` does.
  Use `App::show_short_or_info(info_type, lines)`: toasts when <= 3 lines,
  otherwise opens a scrollable `InfoDialog`.
- `Dialog::Plugin` is generic: one variant handles every plugin dialog.
- `DialogType` lives in `src/tui/components/component.rs`, not `types.rs`.
- Focus management goes through `FocusManager` (`components/focus.rs`).

## Other Invariants

- **Git sidebar is cached**: `GitSidebarState` caches git info; stale
  generations are dropped silently. Do not render git state live per frame.
- **Remote protocol is event/state-driven**: the `/tui` WebSocket speaks the
  `TuiCommand` enum with sequence-tagged `EventEnvelope` replay. There is no
  `RenderFrame` support - do not add pixel/frame-style remote messages.
- **Human shell cells** render via `MsgPart::ShellCell`; `/shell-*` commands
  live in `commands/shell.rs` (see the `human-shell` skill).

## Testing

```bash
cargo test --test tui_render        # rendering integration tests
cargo test --test tui               # behavior tests
```

Rendering tests assert on buffer contents; keep widget output deterministic
(no wall-clock times without injection seams).
