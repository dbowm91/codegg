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
to avoid breaking invariants that are easy to violate across the TUI composition
root and lifecycle modules.

## Layout

| Path | Purpose |
|------|---------|
| `src/tui/app/mod.rs` | `App` composition root, construction, facade, shutdown |
| `src/tui/app/types.rs` | `Dialog`, `TuiMsg`, and stable app-facing types |
| `src/tui/app/commands.rs` | `TuiCommand` effect requests/completions |
| `src/tui/app/render.rs` | Cached render composition; no I/O or async work |
| `src/tui/app/input.rs` | Synchronous `TuiMsg` processing |
| `src/tui/app/project_session.rs` | Active project/session and projection lifecycle |
| `src/tui/app/prompt_turn.rs` | Prompt submission and route-safe turn start |
| `src/tui/app/modal.rs` | FocusManager-backed modal lifecycle |
| `src/tui/app/plugin_ui.rs` | Plugin UI validation and effect application |
| `src/tui/app/state/` | App state helpers; `execution_context.rs` resolves explicit project scope and `async_request.rs` holds the finish/fail guard |
| `src/tui/command.rs` | Slash-command registry, scoped catalog, and discovery metadata |
| `src/tui/commands/` | command-handler submodules (see `mod.rs` for the current set) |
| `src/tui/runtime/command_dispatch.rs` | `dispatch_tui_command(app, cmd)` - maps `TuiCommand` variants to handlers |
| `src/tui/runtime/` | Runtime loop and event routing |
| `src/tui/async_cmd.rs` | `spawn_tui_task` / `spawn_registered_tui_task` |
| `src/tui/task_lifecycle.rs` | `TuiTaskRegistry` - tracks spawned background tasks on `App` |
| `src/tui/components/` | Widgets; `component.rs` has `DialogType`, `focus.rs` has `FocusManager` |

### Intent/effect direction

- `TuiMsg` is synchronous component/user intent. `App::process_msg` applies
  immediate UI state changes or enqueues an explicit runtime effect.
- `TuiCommand` is the runtime-channel boundary. Request variants start work
  through existing domain handlers; completion variants carry typed results
  back for synchronous `App` mutation.
- Do not add daemon completions to `TuiMsg`, or use `process_msg` as a
  completion router. Keep one top-level `runtime/command_dispatch.rs` entry
  point.
- Render modules may only read cached state and prepare widgets. They must not
  perform filesystem, network, daemon, process, or blocking work.

## Adding a New Command

1. Add the variant to the command list in `CommandRegistry::built_in_commands()`
   (`src/tui/command.rs`). Keep its description and discovery domain searchable;
   the exact built-in count is asserted by the registry tests.
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
- Focus management goes through `FocusManager` (`components/component/focus.rs`).
  Only the top mounted modal receives key input; unhandled keys are dropped,
  never bubbled to lower modals or the prompt.
- `FocusManager` owns mounted live modal components. Use its narrow typed
  `with_dialog`, `with_dialog_mut`, or `with_component` helpers for updates;
  do not clone a component into `DialogState` and replace the mounted copy.
  `ui_state.dialog` is a derived compatibility discriminator, while
  `active_dialog_type()` controls modal lifecycle.

## Other Invariants

- **Command discovery metadata**: `CommandRegistry` is the only command
  catalog. Its domain/source/scope/keywords fields are presentation metadata;
  they do not authorize or execute commands. The palette searches those fields
  with a bounded result set and must use the active project catalog. Built-in,
  config, project, and plugin collisions are deterministic, with existing
  entries winning. `ActionKey::all()` is the exhaustive configurable action
  list; `Char` is the explicit non-configurable input exception.

- **Sidebar focus is projection-only**: `SidebarFocusTarget` identities are
  shared by keyboard navigation and mouse hit testing. Selection is cleared on
  project-scope changes, and the bounded `agent_tree` is joined to durable
  runs only by exact canonical task IDs. Enter opens the existing lazy run
  detail surface; it never copies transcripts or logs into sidebar state.
- **Git sidebar is cached**: `GitSidebarState` caches git info; stale
  generations are dropped silently. Do not render git state live per frame.
- **Remote protocol is event/state-driven**: the `/tui` WebSocket speaks the
  `TuiCommand` enum with sequence-tagged `EventEnvelope` replay. There is no
  `RenderFrame` support - do not add pixel/frame-style remote messages.
- **Human shell cells** render via `MsgPart::ShellCell`; `/shell-*` commands
  live in `commands/shell.rs` (see the `human-shell` skill).
- **Project scope**: resolve `App::project_execution_context()` before
  spawning project-scoped work. The active tab supplies project/workspace/
  session identities and the workspace root. Never use process cwd or the
  legacy `session_state.project_dir` mirror as project authority; do not
  change process cwd to switch tabs.
- **Prompt/session creation**: `App::send_prompt` captures immutable prompt
  text and a `ProjectExecutionContext`/`UiRouteToken` when no session exists.
  The event loop starts the registered `PromptSessionCreated` continuation;
  it must not await `CoreClient::request`. Completion validates request,
  tab/project/workspace/view/reconnect scope before calling `set_session` and
  submitting the captured text exactly once. Failures restore the prompt and
  explicit retry must not duplicate the already-visible user message. Tab
  switch/close, reconnect, and shutdown invalidate the continuation.

## Testing

```bash
cargo test --test tui_render        # rendering integration tests
cargo test --test tui               # behavior tests
```

Rendering tests assert on buffer contents; keep widget output deterministic
(no wall-clock times without injection seams).
