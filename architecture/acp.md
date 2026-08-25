# ACP v1 Adapter

`codegg acp` is a stdio Agent Client Protocol v1 agent. It is a presentation
adapter, not an execution mode: the singleton daemon owns sessions, turns,
providers, permissions, cancellation, and durable projection replay.

## Purpose

Translate between the ACP v1 JSON-RPC wire format and CodeGG's native
daemon protocol. The adapter owns only framing and event translation; all
state and execution live in the daemon.

## Where It Lives

| Artifact | Path |
|----------|------|
| Adapter | `src/acp.rs` (single file, ~720 lines) |
| Entry point | `pub async fn run()` at line 79 |
| CLI | `codegg acp` subcommand |

## How It Works

### Startup and Daemon Attachment

1. Reads JSON-RPC frames from stdin line-by-line (BufReader).
2. Responds to `initialize` with protocol version 1, agent info, and
   capabilities (text prompts only; no image/audio/embeddedContext).
3. Daemon attachment is **lazy**: `ensure_client()` (line 288) calls
   `connect_or_start_daemon()` on first session operation. Initialization
   and diagnostics remain usable when the daemon is unavailable.

### Session Lifecycle

| Method | Behavior |
|--------|----------|
| `session/new` | Requires absolute, existing `cwd`. Calls `CoreRequest::SessionCreate`. Subscribes to projection events for the session. |
| `session/load` | Loads an existing session via `CoreRequest::SessionLoad`, subscribes, and replays the snapshot as `session/update` notifications. |
| `session/resume` | Same as `session/load` but skips snapshot replay. |
| `session/prompt` | Submits a turn via `CoreRequest::TurnSubmit`. Only one active prompt per connection. |
| `session/cancel` | Sets cancel intent; sends `CoreRequest::TurnCancel` once a turn is bound. |
| `session/close` | Cancels active turn, removes projection subscription, suppresses late updates. Does NOT destroy the native session. |
| `shutdown` | Returns null result and breaks the event loop. |
| `exit` | Breaks the event loop without response. |
| `$/cancel_request` | Idempotent cancel keyed by request ID. |

Unsupported methods return JSON-RPC error code -32601.

### Turn Lifecycle and Correlation

The adapter supports one active prompt per connection. Before submitting a
turn it drains the event queue and records the highest observed native event
sequence as `submission_event_floor` (line 260-266). After the daemon
acknowledges the submission, only events strictly after that floor and for
the exact requested session are eligible to bind the prompt.

The first eligible `TurnStarted` event establishes the native turn identity
via `ActivePrompt::bind_turn()` (line 59-71). Projection updates and
terminal events must carry that same turn identity. A pre-floor,
neighboring-session, stale-turn, or replayed event is ignored.

`session/cancel`, `$/cancel_request`, and `session/close` share one
idempotent pending-cancellation path (`cancel_if_ready()`, line 268). If
cancellation arrives before `TurnStarted`, the intent is deferred until the
matching turn is identified.

### Event Mapping

`handle_event()` (line 454) maps native projection events to ACP
notifications:

| Native Event | ACP Notification |
|-------------|-----------------|
| `ProjectionEvent::MessageAppended` (public) | `session/update` with `agent_message_chunk` |
| `ProjectionEvent::ToolStarted` | `session/update` with `tool_call` |
| `ProjectionEvent::ToolCompleted` | `session/update` with `tool_call_update` |
| `CoreEvent::TurnStarted` | Turn binding only (no notification) |
| `CoreEvent::TurnCompleted` / `TurnFailed` | Terminal response |

Reasoning, private, and unknown projection events are omitted.

### Terminal Detection

`event_is_terminal()` (line 489) checks for `TurnCompleted` or `TurnFailed`
on both `CoreEvent` and `ProjectionEvent` variants, matching the bound turn
identity. The `terminal_reason()` function (line 515) returns `"end_turn"`
for completions and `"cancelled"` for failures.

### Snapshot Replay

`replay_snapshot()` (line 412) replays a `session/load` snapshot. It
iterates `recent_turns` and `active_turn`, sorted by `started_at`, and
emits `session/update` notifications for `Public`-visibility `User` and
`Assistant` messages. `Tool`, `System`, and `Reasoning` messages are
skipped.

### Cleanup on Exit

The `run()` function (line 79) ensures on exit that:
- Any active prompt is cancelled.
- All projection subscriptions are unsubscribed.

## Key Types & APIs

| Type / Function | Location | Purpose |
|----------------|----------|---------|
| `RpcRequest` | `src/acp.rs:27` | Deserialized JSON-RPC frame |
| `ActivePrompt` | `src/acp.rs:38` | Tracks the in-flight prompt: request ID, session, event floor, turn binding, cancel state |
| `SessionBinding` | `src/acp.rs:74` | Maps session ID to subscription ID and optional root path |
| `run()` | `src/acp.rs:79` | Main async entry point; runs the select loop |
| `ensure_client()` | `src/acp.rs:288` | Lazy daemon connection via `connect_or_start_daemon()` |
| `absolute_cwd()` | `src/acp.rs:304` | Validates and canonicalizes the `cwd` parameter |
| `prompt_text()` | `src/acp.rs:330` | Extracts and concatenates text prompt blocks (1 MiB limit) |
| `native_agents()` | `src/acp.rs:355` | Resolves agents via `Config::load()` and `resolve_agents_with_context()` |
| `subscribe()` | `src/acp.rs:371` | Subscribes to projection events for a session |
| `replay_snapshot()` | `src/acp.rs:412` | Replays snapshot as ACP notifications |
| `handle_event()` | `src/acp.rs:454` | Maps projection events to ACP session/update notifications |
| `event_is_terminal()` | `src/acp.rs:489` | Checks if an event terminates the active prompt |
| `cancel_if_ready()` | `src/acp.rs:268` | Sends `TurnCancel` once turn is bound and cancel is requested |

## Configuration Surface

| Constant | Value | Location |
|----------|-------|----------|
| `ACP_PROTOCOL_VERSION` | `1` | `src/acp.rs:23` |
| `MAX_FRAME_BYTES` | `1 MiB` | `src/acp.rs:24` |

The adapter reads `Config::load()` for agent resolution (line 359). No
ACP-specific config keys exist. The `model` parameter in `session/prompt`
defaults to `agent::EMERGENCY_DEFAULT_MODEL`.

## Invariants & Gotchas

- **Presentation only**: The adapter never creates sessions, runs turns,
  or modifies provider state. All authority stays in the daemon.
- **Single active prompt**: Only one `session/prompt` can be in-flight at
  a time per connection. A second prompt returns error -32000.
- **No global cwd mutation**: `session/new` requires an absolute path and
  canonicalizes it; no `std::env::set_current_dir()`.
- **Durable sessions survive ACP disconnect**: `session/close` releases the
  projection subscription and cancels any active turn, but the native
  session persists for later `session/load` or `session/resume`.
- **Text-only prompts**: Only `type: "text"` prompt blocks are accepted.
  Image, audio, and embedded context blocks return an error.
- **Event floor is strict**: Events at or below the submission floor are
  silently dropped. This prevents stale events from a previous turn from
  binding to the current prompt.
- **Daemon failure is typed**: If the daemon is unavailable or rejects a
  request, the adapter returns a JSON-RPC error (code -32001 or from
  `write_core_error`). No standalone ACP runtime is created.

## Testing

```bash
cargo test -p codegg --lib acp        # unit tests for ActivePrompt, helpers
cargo test -p codegg --lib acp::tests  # lifecycle, cancellation, terminal detection
```

Test coverage (inline, `src/acp.rs:586-723`):
- `lifecycle_rejects_pre_submission_and_neighbor_events` — event floor and
  session filtering
- `lifecycle_binds_one_turn_and_rejects_stale_terminal_events` — turn
  binding and stale rejection
- `cancellation_and_close_are_pending_and_idempotent` — cancel/close
  semantics
- `rejects_relative_and_missing_cwd` — cwd validation
- `accepts_text_prompt_and_rejects_non_text` — prompt block type check
- `rejects_oversized_prompt` — 1 MiB frame limit

## Related Docs

- [core.md](core.md) — `CoreClient`, `SocketCoreClient`, daemon lifecycle
- [server.md](server.md) — HTTP/WebSocket transport (separate from ACP)
- [protocol.md](protocol.md) — `CoreRequest`, `CoreResponse`, `CoreEvent`
