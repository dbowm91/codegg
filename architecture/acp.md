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
