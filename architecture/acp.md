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
| Adapter | `src/acp.rs` (single file, 1354 lines) |
| Entry point | `pub async fn run(policy: Option<CliPolicyOverride>)` at line 85 |
| CLI | `codegg acp` subcommand |

## How It Works

### Startup and Daemon Attachment

1. Reads JSON-RPC frames from stdin line-by-line (BufReader). A frame
   longer than `MAX_FRAME_BYTES` (1 MiB) is rejected with `-32600` before
   parsing; a frame that is not valid JSON is rejected with `-32700`; a
   frame whose `jsonrpc` is not `"2.0"` is rejected with `-32600`.
2. Responds to `initialize` with protocol version 1, agent info, and
   capabilities (text prompts only; no image/audio/embeddedContext). A
   `protocolVersion` other than 1 is rejected with `-32602`. Every other
   method is gated on `initialize` having succeeded.
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

### JSON-RPC Error Codes

| Code | Meaning |
|------|---------|
| `-32700` | Invalid JSON frame |
| `-32600` | Invalid request: frame over 1 MiB, or `jsonrpc` != `"2.0"` |
| `-32602` | Invalid params: unsupported `protocolVersion`, non-absolute/missing `cwd`, unknown `codeggAgent`, or an unattended trial missing `model`/`codeggAgent` |
| `-32601` | Method not supported by CodeGG ACP v1 |
| `-32001` | Daemon unavailable or the connection failed |
| `-32000` | A second concurrent `session/prompt`, or a native daemon rejection |
| `-32002` | Turn admission was not accepted (receipt status) |
| `-32010` | Native turn failed (bounded `kind` / `sessionId` / `turnId` data) |
| `-32011` | Headless turn reached an unanswered permission request |

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
sequence as `submission_event_floor` (`drain_event_floor()`, line 413). After
the daemon acknowledges the submission, only events strictly after that floor
and for the exact requested session are eligible to bind the prompt.

The turn identity is established either by the daemon's admission receipt
(`CoreResponse::TurnAdmissionReceipt` with `status == "accepted"`, which
carries the turn id directly) or by the first eligible `TurnStarted` event
via `ActivePrompt::bind_turn()` (line 64). Projection updates and terminal
events must carry that same turn identity. A pre-floor, neighboring-session,
stale-turn, or replayed event is ignored. The projection stream is matched
separately and more strictly than the native event stream:
`projection_event_matches()` (line 810) requires the exact subscription id,
the exact session id, an event sequence above the subscription's projection
floor, and a turn id that is either unbound or equal to the bound turn.

`session/cancel`, `$/cancel_request`, and `session/close` share one
idempotent pending-cancellation path (`cancel_if_ready()`, line 421). If
cancellation arrives before `TurnStarted`, the intent is deferred until the
matching turn is identified.

### Event Mapping

`handle_event()` (line 746) maps native projection events to ACP
notifications:

| Native Event | ACP Notification |
|-------------|-----------------|
| `ProjectionEvent::MessageAppended` (public) | `session/update` with `agent_message_chunk` |
| `ProjectionEvent::ToolStarted` | `session/update` with `tool_call` |
| `ProjectionEvent::ToolCompleted` | `session/update` with `tool_call_update` |
| `CoreEvent::TurnStarted` | Turn binding only (no notification) |
| `CoreEvent::TurnCompleted` | ACP `end_turn`, or `cancelled` when this client requested cancellation |
| `CoreEvent::TurnFailed` | JSON-RPC error `-32010` with bounded type and session/turn correlation |

Reasoning, private, and unknown projection events are omitted. The
`sessionUpdate` values are emitted with the `update` key nested under
`sessionId` (`update()` at line 937).

For harness accounting, `tool_call` adds the optional `codeggArgumentsSha256`
field when the bounded argument projection is available
(`tool_arguments_sha256()`, line 917). It hashes the projection and does not
expose tool arguments; a handle-only argument projection yields no digest.
A successful `session/prompt` result adds `codeggUsage`
(`terminal_usage()`, line 865) with the native turn ID and provider-reported
input, output, cached and reasoning token counts plus context limit; each
metric stays null if the provider omitted it. `codeggUsage` does not include
a price or estimated cost. These additive fields are CodeGG extensions; ACP
clients may ignore them.

### Terminal Detection

`event_is_terminal()` (line 783) checks for `TurnCompleted` or `TurnFailed` on
both `CoreEvent` and `ProjectionEvent` variants, matching the bound turn
identity. Only confirmed completion maps to `end_turn`; a client-requested
cancellation maps a completed terminal event to `cancelled`. The stop reason
is mapped by `map_acp_stop_reason()` (line 927): `cancelled`/`canceled`,
`max_tokens`, `max_turn_requests`, and `refusal` pass through; anything else
becomes `end_turn`. Native failure maps to a standard JSON-RPC error and
never fabricates cancellation or success.
An unanswered native `PermissionPending` during a headless turn is cancelled
and returned as JSON-RPC error `-32011` with bounded session/turn correlation
(`event_is_permission_pending()`, line 833).

### Snapshot Replay

`replay_snapshot()` (line 704) replays a `session/load` snapshot. It
iterates `recent_turns` and `active_turn`, sorted by `started_at`, and
emits `session/update` notifications for `Public`-visibility `User` and
`Assistant` messages. `Tool`, `System`, and `Reasoning` messages are
skipped.

### Cleanup on Exit

The `run()` function (line 85) ensures on exit that:
- Any active prompt is cancelled.
- All projection subscriptions are unsubscribed.

## Key Types & APIs

| Type / Function | Location | Purpose |
|----------------|----------|---------|
| `RpcRequest` | `src/acp.rs:29` | Deserialized JSON-RPC frame |
| `ActivePrompt` | `src/acp.rs:40` | Tracks the in-flight prompt: request ID, session, submission/projection event floors, subscription id, turn binding, cancel state |
| `SessionBinding` | `src/acp.rs:79` | Maps session ID to subscription ID, cursor, and optional root path |
| `run()` | `src/acp.rs:85` | Main async entry point; runs the select loop |
| `drain_event_floor()` | `src/acp.rs:413` | Drains the native event queue and returns the highest observed `event_seq` |
| `cancel_if_ready()` | `src/acp.rs:421` | Sends `TurnCancel` once turn is bound and cancel is requested |
| `ensure_client()` | `src/acp.rs:441` | Lazy daemon connection via `connect_or_start_daemon()`; applies and verifies the explicit policy override |
| `acquire_policy_lease()` | `src/acp.rs:475` | Takes the OS advisory lease under `CODEGG_DAEMON_HOME` |
| `apply_and_verify_policy()` | `src/acp.rs:505` | `RuntimePolicySet` then `ExecutionPolicyGet` read-back; fails closed on narrowing |
| `absolute_cwd()` | `src/acp.rs:596` | Validates and canonicalizes the `cwd` parameter |
| `prompt_text()` | `src/acp.rs:622` | Extracts and concatenates text prompt blocks (1 MiB limit) |
| `native_agents()` | `src/acp.rs:647` | Resolves agents via `load_config_or_default()` and `resolve_agents_with_context()` |
| `subscribe()` | `src/acp.rs:663` | Subscribes to the session-scoped projection stream |
| `replay_snapshot()` | `src/acp.rs:704` | Replays snapshot as ACP notifications |
| `handle_event()` | `src/acp.rs:746` | Maps projection events to ACP session/update notifications |
| `event_is_terminal()` | `src/acp.rs:783` | Checks if an event terminates the active prompt |
| `projection_event_matches()` | `src/acp.rs:810` | Strict projection-stream correlation (subscription, session, floor, turn) |
| `event_is_permission_pending()` | `src/acp.rs:833` | Detects an unanswered permission for the bound session/turn |
| `terminal_stop_reason()` | `src/acp.rs:850` | Extracts the native stop reason from a terminal event |
| `terminal_usage()` | `src/acp.rs:865` | Builds the `codeggUsage` extension from a terminal event |
| `tool_arguments_sha256()` | `src/acp.rs:917` | Hashes the bounded argument projection (never the raw arguments) |
| `map_acp_stop_reason()` | `src/acp.rs:927` | Maps a native stop reason to the ACP stop-reason set |

## Configuration Surface

| Constant | Value | Location |
|----------|-------|----------|
| `ACP_PROTOCOL_VERSION` | `1` | `src/acp.rs:25` |
| `MAX_FRAME_BYTES` | `1 MiB` | `src/acp.rs:26` |

The adapter reads the resolved config for agent selection. No ACP-specific
config keys exist. The `model` parameter in `session/prompt` defaults to
`agent::EMERGENCY_DEFAULT_MODEL`.

Explicit unattended overrides are **not** a config surface: they arrive as
CLI flags, require `CODEGG_DAEMON_HOME` (a private daemon root), and are
applied through the canonical `RuntimePolicySet` / `ExecutionPolicyGet`
contracts. With no policy flags the adapter leaves the daemon preference
unchanged.

## Invariants & Gotchas

- **Presentation only**: The adapter owns no state. Every session, turn,
  subscription, and provider effect is a daemon request — it issues
  `SessionCreate` / `TurnSubmit` to the daemon rather than creating
  sessions or running turns itself. All authority stays in the daemon.
- **Single active prompt**: Only one `session/prompt` can be in-flight at
  a time per connection. A second prompt returns error -32000.
- **No global cwd mutation**: `session/new` requires an absolute path and
  canonicalizes it; there is no `std::env::set_current_dir()` anywhere in
  `src/acp.rs` (0 matches).
- **Durable sessions survive ACP disconnect**: `session/close` releases the
  projection subscription and cancels any active turn, but the native
  session persists for later `session/load` or `session/resume`.
- **Text-only prompts**: Only `type: "text"` prompt blocks are accepted.
  Image, audio, and embedded context blocks return an error.
- **Event floor is strict**: Events at or below the submission floor are
  silently dropped. This prevents stale events from a previous turn from
  binding to the current prompt.
- **Daemon failure is typed**: If the daemon is unavailable or rejects a
  request, the adapter returns a JSON-RPC error (code -32001 for a failed
  connection, -32000 for a native rejection via `write_core_error`). No
  standalone ACP runtime is created.
- **The policy lease is per daemon root**: `acquire_policy_lease()` fails
  when another ACP policy trial already holds the lock under the same
  `CODEGG_DAEMON_HOME`, so two trials cannot silently fight over one
  daemon's policy.

## Testing

```bash
cargo test -p codegg --lib acp                  # unit tests for ActivePrompt, helpers
cargo test -p codegg --lib acp::tests            # lifecycle, cancellation, terminal detection
cargo test -p codegg --lib acp::helper_tests     # cwd / prompt-block / frame-limit helpers
python3 -m unittest discover -s dev/harness_lab/tests -v
```

Test coverage (inline, `src/acp.rs:1004-1354`, split across two modules —
`mod tests` at `:1005` and `mod helper_tests` at `:1332`):
- `projection_stream_uses_nested_session_turn_subscription_and_cursor` —
  projection subscription shape and cursor
- `acp_harness_extensions_expose_turn_usage_and_argument_fingerprint` —
  `codeggUsage` and `codeggArgumentsSha256`
- `lifecycle_rejects_pre_submission_and_neighbor_events` — event floor and
  session filtering
- `lifecycle_binds_one_turn_and_rejects_stale_terminal_events` — turn
  binding and stale rejection
- `cancellation_and_close_are_pending_and_idempotent` — cancel/close
  semantics
- `permission_pending_is_correlated_to_the_active_session_and_turn` —
  `-32011` correlation
- `terminal_outcome_does_not_mislabel_failure_as_cancellation` — native
  failure is distinct from successful completion and requested cancellation
- `acp_maps_only_known_standard_stop_reasons` — stop-reason mapping
- `rejects_relative_and_missing_cwd` — cwd validation
- `accepts_text_prompt_and_rejects_non_text` — prompt block type check
- `rejects_oversized_prompt` — 1 MiB frame limit

## Related Docs

- [core.md](core.md) — `CoreClient`, `SocketCoreClient`, daemon lifecycle
- [server.md](server.md) — HTTP/WebSocket transport (separate from ACP)
- [protocol.md](protocol.md) — `CoreRequest`, `CoreResponse`, `CoreEvent`

## Source verification

Verified 2026-10-23 against `src/acp.rs` (1354 lines) and
`src/main.rs`'s `codegg acp` subcommand.
- **Corrected the file length**: the doc said "~736 lines"; the file is
  **1354 lines**. Every `file.rs:line` reference in the doc had drifted
  and was re-derived from the live source: `ACP_PROTOCOL_VERSION` `:22` ->
  `:25`, `MAX_FRAME_BYTES` `:23` -> `:26`, `RpcRequest` `:26` -> `:29`,
  `ActivePrompt` `:37` -> `:40`, `SessionBinding` `:73` -> `:79`,
  `run()` `:78` -> `:85`, `cancel_if_ready()` `:281` -> `:421`,
  `ensure_client()` `:301` -> `:441`, `absolute_cwd()` `:317` -> `:596`,
  `prompt_text()` `:343` -> `:622`, `native_agents()` `:368` -> `:647`,
  `subscribe()` `:384` -> `:663`, `replay_snapshot()` `:425` -> `:704`,
  `handle_event()` `:467` -> `:746`, `event_is_terminal()` `:502` -> `:783`,
  the inline test range `:599-736` -> `:1004-1354`, the
  `submission_event_floor` reference `260-266` -> `drain_event_floor()` at
  `:413`, and `bind_turn()` `59-71` -> `:64`.
- **Added the JSON-RPC error-code table** — the doc named only five codes
  (-32601, -32000, -32001, -32010, -32011). The adapter also uses
  `-32700` (invalid JSON), `-32600` (oversize frame / non-`2.0` jsonrpc),
  `-32602` (invalid params: bad `protocolVersion`, bad `cwd`, unknown
  `codeggAgent`, incomplete unattended trial), and `-32002` (turn admission
  not accepted).
- **Corrected the "Presentation only" invariant**: the old wording said
  the adapter "never creates sessions, runs turns", but it *does* issue
  `CoreRequest::SessionCreate` and `CoreRequest::TurnSubmit` (and
  `WorkspaceRegister` / `ProjectRegister`) to the daemon. Reworded to the
  accurate claim: it owns no state and every effect is a daemon request.
- **Corrected the daemon-failure code**: `write_core_error` uses `-32000`,
  not an unspecified code.
- **Added the missing helpers** the source owns and the doc omitted:
  `drain_event_floor()` (`:413`), `acquire_policy_lease()` (`:475`),
  `apply_and_verify_policy()` (`:505`), `projection_event_matches()`
  (`:810`), `event_is_permission_pending()` (`:833`),
  `terminal_stop_reason()` (`:850`), `terminal_usage()` (`:865`),
  `tool_arguments_sha256()` (`:917`), `map_acp_stop_reason()` (`:927`).
- **Added the turn-admission binding path**: the turn id now comes from
  `CoreResponse::TurnAdmissionReceipt { status == "accepted" }` as well as
  from the first eligible `TurnStarted` event.
- **Added the strict projection correlation rule**
  (`projection_event_matches()`): exact subscription id, exact session id,
  event sequence above the subscription's projection floor, and a turn id
  that is unbound or equal to the bound turn.
- **Corrected the test section**: the inline tests are split across two
  modules — `mod tests` (`:1005`) and `mod helper_tests` (`:1332`) — and
  four tests the doc omitted were added
  (`projection_stream_uses_nested_session_turn_subscription_and_cursor`,
  `acp_harness_extensions_expose_turn_usage_and_argument_fingerprint`,
  `permission_pending_is_correlated_to_the_active_session_and_turn`,
  `acp_maps_only_known_standard_stop_reasons`).
- **Corrected `native_agents()`**: it resolves through
  `crate::core::load_config_or_default()`, not `Config::load()`.
- Verified accurate, no change: `ACP_PROTOCOL_VERSION` = 1 and
  `MAX_FRAME_BYTES` = 1 MiB; the ten handled methods
  (`initialize`, `session/new`, `session/load`, `session/resume`,
  `session/prompt`, `session/cancel`, `session/close`, `shutdown`, `exit`,
  `$/cancel_request`); the "only one active prompt" rule and its -32000
  code; the lazy daemon attachment; the `yolo` + `full-host|workspace-write`
  policy requirement and the `CODEGG_DAEMON_HOME` lease requirement; the
  `agent_message_chunk` / `tool_call` / `tool_call_update` mapping; the
  `codeggArgumentsSha256` and `codeggUsage` extension shapes; the
  `Public`-visibility-only snapshot replay; the exit-time cancel and
  unsubscribe; and the absence of `std::env::set_current_dir()` (0 matches).

Verified 2026-10-06 against `src/acp.rs`. Corrected six off-by-one
`file.rs:line` refs: `RpcRequest` `:27` → `:26`, `ActivePrompt` `:38` →
`:37`, `SessionBinding` `:74` → `:73`, `ACP_PROTOCOL_VERSION` `:23` →
`:22`, `MAX_FRAME_BYTES` `:24` → `:23`, and the inline test range
`:598-736` → `:599-736`. Verified accurate: the 12 method refs
(`run` `:78`, `cancel_if_ready` `:281`, `ensure_client` `:301`,
`absolute_cwd` `:317`, `prompt_text` `:343`, `native_agents` `:368`,
`subscribe` `:384`, `replay_snapshot` `:425`, `handle_event` `:467`,
`event_is_terminal` `:502`), the 736-line file length,
`ACP_PROTOCOL_VERSION` = 1, and `MAX_FRAME_BYTES` = 1 MiB.
