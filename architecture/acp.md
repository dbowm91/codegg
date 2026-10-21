# ACP v1 Adapter

`codegg acp` is a stdio Agent Client Protocol v1 agent. It is a presentation
adapter, not an execution mode: the daemon owns sessions, turns, providers,
permissions, cancellation, and durable projection replay. With no policy flags
the adapter leaves the daemon preference unchanged. Explicit unattended
overrides use the canonical `RuntimePolicySet` and `ExecutionPolicyGet`
contracts and require `CODEGG_DAEMON_HOME`; they never silently target the
default user daemon.

## Purpose

Translate between the ACP v1 JSON-RPC wire format and CodeGG's native
daemon protocol. The adapter owns only framing and event translation; all
state and execution live in the daemon.

## Where It Lives

| Artifact | Path |
|----------|------|
| Adapter | `src/acp.rs` (single file, 1601 lines) |
| Entry point | `pub async fn run()` at line 92 |
| CLI | `codegg acp` subcommand |

## How It Works

### Startup and Daemon Attachment

1. Reads JSON-RPC frames from stdin line-by-line (BufReader).
2. Responds to `initialize` with protocol version 1, agent info, and
   capabilities (text prompts only; no image/audio/embeddedContext).
3. Daemon attachment is **lazy**: `ensure_client()` calls
   `connect_or_start_daemon()` on first session operation. Initialization
   and diagnostics remain usable when the daemon is unavailable.
4. Explicit policy flags require both `--approval-mode yolo` and
   `--sandbox full-host|workspace-write`. The adapter holds an OS advisory
   lease for the lifetime of the connection, applies policy through the
   daemon, and reads back the effective snapshot before admitting a session
   request. A narrower or unavailable policy fails closed. Harness trials
   use one ACP client per private daemon; external policy writers are outside
   that lease and must not share the trial daemon.

### Session Lifecycle

For each new or loaded native session, the adapter subscribes to the daemon's
session-scoped core event stream as well as its projection stream. The native
subscription carries correlated `TurnStarted`/terminal events; without it the
ACP prompt could submit work successfully yet wait forever for completion.

| Method | Behavior |
|--------|----------|
| `session/new` | Requires absolute, existing `cwd`. Registers workspace and project, then calls `CoreRequest::SessionCreate`. Subscribes to projection events for the session. |
| `session/load` | Loads an existing session via `CoreRequest::SessionLoad`, subscribes, and replays the snapshot as `session/update` notifications. |
| `session/resume` | Same as `session/load` but skips snapshot replay. |
| `session/prompt` | Submits a turn via `CoreRequest::TurnSubmit`. Only one active prompt per connection. Explicit-policy trials require a `model` and namespaced `codeggAgent`. |
| `session/cancel` | Sets cancel intent; sends `CoreRequest::TurnCancel` once a turn is bound. |
| `session/close` | Cancels active turn, removes projection subscription, suppresses late updates. Does NOT destroy the native session. |
| `shutdown` | Returns null result and breaks the event loop. |
| `exit` | Breaks the event loop without response. |
| `$/cancel_request` | Idempotent cancel keyed by request ID. |

Unsupported methods return JSON-RPC error code -32601.

### Turn Lifecycle and Correlation

The adapter supports one active prompt per connection. Before submitting a
turn it drains the event queue and records the highest observed native event
sequence as `submission_event_floor` (`drain_event_floor()`, line 421). After the daemon
acknowledges the submission, only events strictly after that floor and for
the exact requested session are eligible to bind the prompt.

The first eligible `TurnStarted` event establishes the native turn identity
via `ActivePrompt::bind_turn()` (line 71). Projection updates and
terminal events must carry that same turn identity. A pre-floor,
neighboring-session, stale-turn, or replayed event is ignored.

`session/cancel`, `$/cancel_request`, and `session/close` share one
idempotent pending-cancellation path (`cancel_if_ready()`, line 429). If
cancellation arrives before `TurnStarted`, the intent is deferred until the
matching turn is identified.

### Event Mapping

`handle_event()` (line 754) maps native projection events to ACP
notifications:

| Native Event | ACP Notification |
|-------------|-----------------|
| `ProjectionEvent::MessageAppended` (public) | `session/update` with `agent_message_chunk` |
| `ProjectionEvent::ToolStarted` | `session/update` with `tool_call` |
| `ProjectionEvent::ToolCompleted` | `session/update` with `tool_call_update` |
| `CoreEvent::TurnStarted` | Turn binding only (no notification) |
| `CoreEvent::TurnCompleted` | ACP `end_turn`, or `cancelled` when this client requested cancellation |
| `CoreEvent::TurnFailed` | JSON-RPC error `-32010` with bounded type and session/turn correlation |

Reasoning, private, and unknown projection events are omitted.

For harness accounting, `tool_call` adds the optional `codeggArgumentsSha256`
field when the bounded argument projection is available. It hashes the
projection and does not expose tool arguments. A successful `session/prompt`
result adds `codeggUsage` with the native turn ID and provider-reported input,
output, cached and reasoning token counts plus context limit; each metric stays
null if the provider omitted it. `codeggUsage` does not include a price or
estimated cost. These additive fields are CodeGG extensions; ACP clients may
ignore them.

### Failed-turn evidence

A native turn failure is the JSON-RPC error `-32010` whose `data` payload
carries the redacted failure class and the usage provenance:

| Field | Meaning |
|---|---|
| `kind` | Always `turn_failed`. |
| `sessionId`, `turnId` | The session and turn correlation, unchanged. |
| `codeggFailureCategory` | One allowlisted `TurnFailureCategory` value. `native_failure_unspecified` when the native side projected no category. |
| `codeggUsage` | The token counts observed before the failure, or `null` when no count was available. |
| `codeggUsageReason` | Why the counts are present or absent: `reported`, `not_observed`, `not_provided`, `lost_at_projection`, or `unspecified`. |

The three provenance states are distinct facts and must stay distinct: a turn
that ended before any provider response is `not_observed`; a provider response
that carried no token counts is `not_provided`; a turn that reported token
counts is `reported`. `codeggUsage` is `null` whenever no count is available,
so a missing measurement is never read as zero usage.

The failure payload **never** carries `CoreEvent::TurnFailed.message`. That
message embeds upstream provider text and can carry credentials; it stays in
the local transcript. The category is derived from the typed native error, and
the usage aggregate covers every provider response in the failed run — a turn
that made two metered calls and then failed spent both.

`codeggFailureCategory` and `codeggUsageReason` are CodeGG extensions on an
existing error code. The ACP protocol version, method names, error codes, and
the successful result shape are unchanged.

### Terminal Detection

`event_is_terminal()` checks for `TurnCompleted` or `TurnFailed` on both
`CoreEvent` and `ProjectionEvent` variants, matching the bound turn identity.
Only confirmed completion maps to `end_turn`; a client-requested cancellation
maps a completed terminal event to `cancelled`. Native failure maps to a
standard JSON-RPC error and never fabricates cancellation or success.
An unanswered native `PermissionPending` during a headless turn is cancelled
and returned as JSON-RPC error `-32011` with bounded session/turn correlation.

### Snapshot Replay

`replay_snapshot()` (line 712) replays a `session/load` snapshot. It
iterates `recent_turns` and `active_turn`, sorted by `started_at`, and
emits `session/update` notifications for `Public`-visibility `User` and
`Assistant` messages. `Tool`, `System`, and `Reasoning` messages are
skipped.

### Cleanup on Exit

The `run()` function (line 92) ensures on exit that:
- Any active prompt is cancelled.
- All projection subscriptions are unsubscribed.

## Key Types & APIs

| Type / Function | Location | Purpose |
|----------------|----------|---------|
| `RpcRequest` | `src/acp.rs:26` | Deserialized JSON-RPC frame |
| `ActivePrompt` | `src/acp.rs:37` | Tracks the in-flight prompt: request ID, session, event floor, turn binding, cancel state |
| `SessionBinding` | `src/acp.rs:73` | Maps session ID to subscription ID and optional root path |
| `run()` | `src/acp.rs:78` | Main async entry point; runs the select loop |
| `ensure_client()` | `src/acp.rs:301` | Lazy daemon connection via `connect_or_start_daemon()` |
| `absolute_cwd()` | `src/acp.rs:317` | Validates and canonicalizes the `cwd` parameter |
| `prompt_text()` | `src/acp.rs:343` | Extracts and concatenates text prompt blocks (1 MiB limit) |
| `native_agents()` | `src/acp.rs:368` | Resolves agents via `Config::load()` and `resolve_agents_with_context()` |
| `subscribe()` | `src/acp.rs:384` | Subscribes to projection events for a session |
| `replay_snapshot()` | `src/acp.rs:425` | Replays snapshot as ACP notifications |
| `handle_event()` | `src/acp.rs:467` | Maps projection events to ACP session/update notifications |
| `event_is_terminal()` | `src/acp.rs:502` | Checks if an event terminates the active prompt |
| `cancel_if_ready()` | `src/acp.rs:281` | Sends `TurnCancel` once turn is bound and cancel is requested |

## Configuration Surface

| Constant | Value | Location |
|----------|-------|----------|
| `ACP_PROTOCOL_VERSION` | `1` | `src/acp.rs:22` |
| `MAX_FRAME_BYTES` | `1 MiB` | `src/acp.rs:23` |

The adapter reads the resolved config for agent selection. No ACP-specific
config keys exist. The `model` parameter in `session/prompt` defaults to
`agent::EMERGENCY_DEFAULT_MODEL`.

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
python3 -m unittest discover -s dev/harness_lab/tests -v
```

Test coverage (inline, `src/acp.rs:599-736`):
- `lifecycle_rejects_pre_submission_and_neighbor_events` — event floor and
  session filtering
- `lifecycle_binds_one_turn_and_rejects_stale_terminal_events` — turn
  binding and stale rejection
- `cancellation_and_close_are_pending_and_idempotent` — cancel/close
  semantics
- `terminal_outcome_does_not_mislabel_failure_as_cancellation` — native
  failure is distinct from successful completion and requested cancellation
- `rejects_relative_and_missing_cwd` — cwd validation
- `accepts_text_prompt_and_rejects_non_text` — prompt block type check
- `rejects_oversized_prompt` — 1 MiB frame limit

## Related Docs

- [core.md](core.md) — `CoreClient`, `SocketCoreClient`, daemon lifecycle
- [server.md](server.md) — HTTP/WebSocket transport (separate from ACP)
- [protocol.md](protocol.md) — `CoreRequest`, `CoreResponse`, `CoreEvent`

## Source verification

Re-verified 2026-10-21 against `src/acp.rs` after the failed-turn evidence
change. `src/acp.rs` is now **1601 lines**.

| Symbol | Line |
|---|---|
| `ACP_PROTOCOL_VERSION` | 25 |
| `MAX_FRAME_BYTES` | 26 |
| `ACP_TURN_FAILED_CODE` | 33 |
| `RpcRequest` | 36 |
| `ActivePrompt` | 47 |
| `SessionBinding` | 86 |
| `run()` | 92 |
| `cancel_if_ready()` | 429 |
| `ensure_client()` | 449 |
| `absolute_cwd()` | 604 |
| `prompt_text()` | 630 |
| `native_agents()` | 655 |
| `subscribe()` | 671 |
| `replay_snapshot()` | 712 |
| `handle_event()` | 754 |
| `event_is_terminal()` | 791 |
| `terminal_stop_reason()` | 858 |
| `terminal_usage()` | 873 |
| `terminal_failure_data()` | 941 |
| `map_acp_stop_reason()` | 1003 |
| `write_error_with_data()` | 1041 |
| `error_frame()` | 1055 |
| test module | 1084-1577 |

`ACP_PROTOCOL_VERSION` is 1 and `MAX_FRAME_BYTES` is 1 MiB.

`terminal_failure_data()` is the single place that builds the `-32010` `data`
payload, so the category, the usage, and the usage reason are produced
together and cannot drift apart.
