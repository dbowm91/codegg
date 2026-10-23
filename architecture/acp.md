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
| Adapter | `src/acp.rs` (single file, 1695 lines) |
| Entry point | `pub async fn run(policy: Option<CliPolicyOverride>)` at line 112 |
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
sequence as `submission_event_floor` (`drain_event_floor()`, line 480). After
the daemon acknowledges the submission, only events strictly after that floor
and for the exact requested session are eligible to bind the prompt.

The turn identity is established either by the daemon's admission receipt
(`CoreResponse::TurnAdmissionReceipt` with `status == "accepted"`, which
carries the turn id directly) or by the first eligible `TurnStarted` event
via `ActivePrompt::bind_turn()` (line 91). Projection updates and terminal
events must carry that same turn identity. A pre-floor, neighboring-session,
stale-turn, or replayed event is ignored. The projection stream is matched
separately and more strictly than the native event stream:
`projection_event_matches()` (line 911) requires the exact subscription id,
the exact session id, an event sequence above the subscription's projection
floor, and a turn id that is either unbound or equal to the bound turn.

`session/cancel`, `$/cancel_request`, and `session/close` share one
idempotent pending-cancellation path (`cancel_if_ready()`, line 490). If
cancellation arrives before `TurnStarted`, the intent is deferred until the
matching turn is identified.

### Event Mapping

`handle_event()` (line 847) maps native projection events to ACP
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
`sessionId` (`update()` at line 1106).

For harness accounting, `tool_call` adds the optional `codeggArgumentsSha256`
field when the bounded argument projection is available
(`tool_arguments_sha256()`, line 1086). It hashes the projection and does not
expose tool arguments; a handle-only argument projection yields no digest.
A successful `session/prompt` result adds `codeggUsage`
(`terminal_usage()`, line 966) with the native turn ID and provider-reported
input, output, cached and reasoning token counts plus context limit; each
metric stays null if the provider omitted it. `codeggUsage` does not include
a price or estimated cost. These additive fields are CodeGG extensions; ACP
clients may ignore them.

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

`event_is_terminal()` (line 884) checks for `TurnCompleted` or `TurnFailed` on
both `CoreEvent` and `ProjectionEvent` variants, matching the bound turn
identity. Only confirmed completion maps to `end_turn`; a client-requested
cancellation maps a completed terminal event to `cancelled`. The stop reason
is mapped by `map_acp_stop_reason()` (line 1096): `cancelled`/`canceled`,
`max_tokens`, `max_turn_requests`, and `refusal` pass through; anything else
becomes `end_turn`. Native failure maps to a standard JSON-RPC error and
never fabricates cancellation or success.
An unanswered native `PermissionPending` during a headless turn is cancelled
and returned as JSON-RPC error `-32011` with bounded session/turn correlation
(`event_is_permission_pending()`, line 934).

### Snapshot Replay

`replay_snapshot()` (line 775) replays a `session/load` snapshot. It
iterates `recent_turns` and `active_turn`, sorted by `started_at`, and
emits `session/update` notifications for `Public`-visibility `User` and
`Assistant` messages. `Tool`, `System`, and `Reasoning` messages are
skipped.

### Cleanup on Exit

The `run()` function (line 112) ensures on exit that:
- Any active prompt is cancelled.
- All projection subscriptions are unsubscribed.

## Key Types & APIs

| Type / Function | Location | Purpose |
|----------------|----------|---------|
| `RpcRequest` | `src/acp.rs:49` | Deserialized JSON-RPC frame |
| `ActivePrompt` | `src/acp.rs:60` | Tracks the in-flight prompt: request ID, session, submission/projection event floors, subscription id, turn binding, cancel state |
| `SessionBinding` | `src/acp.rs:106` | Maps session ID to subscription ID, cursor, and optional root path |
| `run()` | `src/acp.rs:112` | Main async entry point; runs the select loop |
| `drain_event_floor()` | `src/acp.rs:480` | Drains the native event queue and returns the highest observed `event_seq` |
| `cancel_if_ready()` | `src/acp.rs:490` | Sends `TurnCancel` once turn is bound and cancel is requested |
| `ensure_client()` | `src/acp.rs:510` | Lazy daemon connection via `connect_or_start_daemon()`; applies and verifies the explicit policy override |
| `acquire_policy_lease()` | `src/acp.rs:546` | Takes the OS advisory lease under `CODEGG_DAEMON_HOME` |
| `apply_and_verify_policy()` | `src/acp.rs:576` | `RuntimePolicySet` then `ExecutionPolicyGet` read-back; fails closed on narrowing |
| `absolute_cwd()` | `src/acp.rs:667` | Validates and canonicalizes the `cwd` parameter |
| `prompt_text()` | `src/acp.rs:693` | Extracts and concatenates text prompt blocks (1 MiB limit) |
| `native_agents()` | `src/acp.rs:718` | Resolves agents via `load_config_or_default()` and `resolve_agents_with_context()` |
| `subscribe()` | `src/acp.rs:734` | Subscribes to the session-scoped projection stream |
| `replay_snapshot()` | `src/acp.rs:775` | Replays snapshot as ACP notifications |
| `handle_event()` | `src/acp.rs:847` | Maps projection events to ACP session/update notifications |
| `event_is_terminal()` | `src/acp.rs:884` | Checks if an event terminates the active prompt |
| `projection_event_matches()` | `src/acp.rs:911` | Strict projection-stream correlation (subscription, session, floor, turn) |
| `event_is_permission_pending()` | `src/acp.rs:934` | Detects an unanswered permission for the bound session/turn |
| `terminal_stop_reason()` | `src/acp.rs:951` | Extracts the native stop reason from a terminal event |
| `terminal_usage()` | `src/acp.rs:966` | Builds the `codeggUsage` extension from a terminal event |
| `terminal_failure_data()` | `src/acp.rs:1034` | Builds the `-32010` `data` payload: failure category, usage, and usage reason together |
| `tool_arguments_sha256()` | `src/acp.rs:1086` | Hashes the bounded argument projection (never the raw arguments) |
| `map_acp_stop_reason()` | `src/acp.rs:1096` | Maps a native stop reason to the ACP stop-reason set |

## Configuration Surface

| Constant | Value | Location |
|----------|-------|----------|
| `ACP_PROTOCOL_VERSION` | `1` | `src/acp.rs:26` |
| `MAX_FRAME_BYTES` | `1 MiB` | `src/acp.rs:27` |
| `ACP_TURN_FAILED_CODE` | `-32010` | `src/acp.rs:34` |

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
- **An incomplete observation is not a completed one**: the daemon's event
  log is a bounded broadcast, so a connection that falls behind it loses
  events. The daemon reports that loss, the adapter surfaces it, and the
  consumer is told. The adapter subscribes **loss-aware**: a dropped event
  arrives as a typed condition, never as silence. See "Reporting a hole in
  the event stream" below.

## Reporting a hole in the event stream

A consumer that pairs a tool call with its completion must be able to tell
"the tool never finished" from "the completion notification was dropped".
The adapter therefore reports a gap in the daemon's event stream on an
**optional**, namespaced session-update kind:

```json
{"sessionUpdate": "codegg_event_gap", "dropped": 12, "source": "daemon_event_stream_gap"}
```

and, when the gap happened during a turn, the turn's own result carries the
count:

```json
{"stopReason": "end_turn", "codeggUsage": {...}, "codeggEventGap": {"dropped": 12}}
```

Both are **additive and optional**:

- no ACP v1 field became **required**;
- a consumer that does not know the `codegg_event_gap` kind ignores it;
- a consumer that does not read `codeggEventGap` sees the same result it saw
  before.

The rule a consumer must follow: **a trial whose observation had a hole in it
is unmeasured.** It is never scored as a completed trial and never scored as a
failure of the subject — it is re-run, under a bounded number of re-runs.
The harness implements that rule in `dev/harness_lab/`.

The two gap sources are different facts and both are carried: a **daemon**
gap is the daemon telling the consumer it dropped events from the stream it
sends, and a **client broadcast** lag is the consumer itself falling behind
the shared broadcast.

## Testing

```bash
cargo test -p codegg --lib acp                  # unit tests for ActivePrompt, helpers
cargo test -p codegg --lib acp::tests            # lifecycle, cancellation, terminal detection
cargo test -p codegg --lib acp::helper_tests     # cwd / prompt-block / frame-limit helpers
python3 -m unittest discover -s dev/harness_lab/tests -v
```

Test coverage (inline, `src/acp.rs:1178-1695`, split across two modules —
`mod tests` at `:1178` and `mod helper_tests` at `:1674`):
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

Re-verified 2026-10-23 against the merged `src/acp.rs` (**1695 lines**),
after `main`'s ACP audit (2026-10-23, against 1354 lines) and the corrective
line's failed-turn-evidence and event-gap changes were combined. Every
`file.rs:line` reference was re-derived from the merged source rather than
carried over: `ACP_PROTOCOL_VERSION` `:26`, `MAX_FRAME_BYTES` `:27`,
`ACP_TURN_FAILED_CODE` `:34`, `RpcRequest` `:49`, `ActivePrompt` `:60`,
`SessionBinding` `:106`, `run()` `:112`, `bind_turn()` `:91`,
`drain_event_floor()` `:480`, `cancel_if_ready()` `:490`,
`ensure_client()` `:510`, `acquire_policy_lease()` `:546`,
`apply_and_verify_policy()` `:576`, `absolute_cwd()` `:667`,
`prompt_text()` `:693`, `native_agents()` `:718`, `subscribe()` `:734`,
`replay_snapshot()` `:775`, `handle_event()` `:847`,
`event_is_terminal()` `:884`, `projection_event_matches()` `:911`,
`event_is_permission_pending()` `:934`, `terminal_stop_reason()` `:951`,
`terminal_usage()` `:966`, `terminal_failure_data()` `:1034`,
`tool_arguments_sha256()` `:1086`, `map_acp_stop_reason()` `:1096`,
`update()` `:1106`, `write_error_with_data()` `:1134`, `error_frame()` `:1148`,
and the inline test range `:1178-1695` (`mod tests` at `:1178`,
`mod helper_tests` at `:1674`).

The audit's **content** was not re-litigated: the JSON-RPC error-code table,
the policy lease, the turn-admission binding path, the strict projection
correlation rule, and the corrected "presentation only" invariant all still
describe the merged source. Two sections the audit could not cover are
restored from the corrective line, because `main`'s audit was performed
against a `src/acp.rs` that did not carry them: **Failed-turn evidence** (the
`-32010` data payload) and **Reporting a hole in the event stream** (the
`codegg_event_gap` update and the `codeggEventGap` result field).
`terminal_failure_data()` is the single place that builds the `-32010` `data`
payload, so the category, the usage, and the usage reason are produced
together and cannot drift apart.

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
