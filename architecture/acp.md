# ACP v1 adapter

`codegg acp` is a stdio Agent Client Protocol v1 agent. It is a presentation
adapter, not an execution mode: the singleton daemon owns sessions, turns,
providers, permissions, cancellation, and durable projection replay.

The adapter deliberately uses the repository's native JSON protocol and
projection types. The current official Rust ACP crate requires Rust 1.88,
above CodeGG's Rust 1.81 MSRV, so the wire contract is implemented with a
small local JSON-RPC wrapper. ACP protocol version negotiation remains
explicit and is independent of the crate release version.

Supported baseline methods are `initialize`, `session/new`, `session/prompt`,
`session/load`, `session/resume`, `session/cancel`, `session/close`,
`shutdown`, and `exit`. Only text prompt blocks are advertised. Visible text
and tool updates are mapped from canonical projection events; reasoning and
unknown/private events are omitted. Unsupported optional methods return a
JSON-RPC method-not-found error and are not advertised.

The process performs no global cwd mutation. `session/new` requires an
absolute, existing directory and binds it through native session creation.
Daemon attachment is lazy after ACP initialization, so initialization and
protocol diagnostics remain usable when the daemon is unavailable. A daemon
failure is returned as a typed session-operation error; no standalone ACP
runtime is created.

Stdout is owned by the ACP JSON-RPC writer and contains one UTF-8 JSON frame
per line. Tracing is sent to stderr. Frames and prompt content are bounded at
1 MiB, while native projection limits remain authoritative for streamed
content and tool output.

Example editor launch configuration:

```json
{
  "agent": "codegg",
  "command": "codegg",
  "args": ["acp"]
}
```

The adapter preserves durable native sessions when an ACP connection closes;
`session/close` only releases its transient subscription and cancels an
active turn.

## Turn lifecycle and correlation

The adapter supports one active prompt per ACP connection. Before submitting a
turn it drains the connection event queue and records the highest observed
native event sequence as the submission floor. After the daemon acknowledges the
submission, only events strictly after that floor and for the exact requested
session are eligible to bind the prompt. The first eligible `TurnStarted` event
establishes the native turn identity; projection updates and terminal events
must carry that same turn identity. A pre-floor, neighboring-session, stale-turn,
or replayed event is ignored and cannot complete the prompt.

`session/cancel`, `$/cancel_request`, and `session/close` share one idempotent
pending-cancellation path. If cancellation arrives before `TurnStarted`, the
intent remains in the transient ACP binding and is sent once the matching turn
is identified. Closing also removes the projection subscription and suppresses
later updates while the native turn reaches its terminal response. EOF performs
the same bounded cancellation and unsubscribe cleanup.

`session/load` uses the role-bearing canonical projection snapshot for replay.
Public user and assistant messages retain their roles; tool, system, reasoning,
private, and unsupported entries are omitted rather than relabeled. Snapshot
limits and the 1 MiB ACP frame limit remain authoritative. A subscription result
that is not a successful `ProjectionSubscribed` response is an adapter error,
not an empty subscription.
