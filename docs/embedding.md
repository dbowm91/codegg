# Embedding CodeGG: the host protocol

`codegg host --stdio` runs one agent session in one process, driven by a
program that is not the TUI: an editor, a desktop app, a local server delegating
coding work, or a CI job. This document is the contract. [`plans/adrs/ADR-0061`](../plans/adrs/ADR-0061-embedding-host-wire-as-a-public-contract.md)
makes it a public surface with the stability policy in
[Versioning](#versioning); the line's plans are
[`plans/subsystems/codegg-embedding-host-roadmap.md`](../plans/subsystems/codegg-embedding-host-roadmap.md).

The host protocol is not ACP and not the frontend protocol. It needs no daemon,
no project, no git worktree and no configuration file.

## Contents

- [Process model](#process-model)
- [Framing](#framing)
- [Handshake](#handshake)
- [Methods](#methods)
- [Notifications](#notifications)
- [Errors](#errors)
- [The provider descriptor](#the-provider-descriptor)
- [Worked example](#worked-example)
- [Security rules](#security-rules)
- [Versioning](#versioning)
- [Operational bounds](#operational-bounds)

## Process model

```text
host process                     codegg host --stdio
    |  JSON-RPC 2.0 over stdio        |
    |-------------------------------->|  initialize
    |-------------------------------->|  session/open  (inline descriptor)
    |-------------------------------->|  turn/submit
    |<--------------------------------|  turn/text, turn/tool, turn/usage
    |<--------------------------------|  turn/done | turn/error
    |-------------------------------->|  turn/submit   (next turn)
    |-------------------------------->|  session/close
```

- **One process, one session.** A host that wants two workspaces runs two
  processes. There is no multiplexing and no daemon.
- **One active turn.** `turn/submit` while a turn is active is refused with the
  `busy` class. Turns are never queued.
- **The workspace root is the turn's writable root.** It must be an absolute
  path that exists and is a directory. It is canonicalized before use. CodeGG
  does not require it to be a git repository.
- **stdout carries protocol frames only.** Every diagnostic goes to stderr and
  is redacted; a host may discard stderr entirely.
- **stdin EOF ends the process.** An active turn is cancelled, the session is
  closed, and the process exits `0`.

## Framing

- JSON-RPC 2.0, one object per line: newline-delimited JSON, UTF-8, `\n`
  terminated. A newline inside an object is escaped by JSON, so a frame is never
  split.
- Requests carry an `id` (string or number, echoed back). Notifications from
  CodeGG carry no `id`. A notification *from* the host is not accepted.
- Maximum frame size is 8 MiB in both directions. A larger inbound frame is
  refused with `invalid_request` and the connection is dropped.
- An unparseable line is refused with `invalid_request`; the connection is
  dropped only when the stream cannot be resynchronized, which for NDJSON means
  an unparseable line.
- Unknown fields in a request are ignored, so an older CodeGG tolerates a newer
  host's optional field. Unknown methods are refused.

## Handshake

### `initialize`

Must be the first request. Any other method before it is refused with
`not_initialized`.

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
```

Params, all optional:

| Field | Type | Meaning |
|---|---|---|
| `clientName` | string, ≤ 64 | Host identity, for diagnostics only. |
| `clientVersion` | string, ≤ 32 | Host version, for diagnostics only. |
| `capabilities` | string[] | Capabilities the host requires of CodeGG. |

Result:

```json
{"jsonrpc":"2.0","id":1,"result":{
  "apiVersion":1,
  "codeggVersion":"0.1.0",
  "capabilities":["descriptor","streaming","cancellation","usage"]
}}
```

Capabilities in v1:

| Capability | Meaning |
|---|---|
| `descriptor` | `session/open` accepts an inline provider descriptor. |
| `streaming` | `turn/text` and `turn/tool` are emitted while a turn runs. |
| `cancellation` | `turn/cancel` is honoured within the bound. |
| `usage` | `turn/usage` and `turn/done` report provider usage. |

A host that lists a capability CodeGG does not advertise is refused with
`unsupported_capability` rather than downgraded silently. Listing a capability
is a requirement, not a hint.

## Methods

### `session/open`

```json
{"jsonrpc":"2.0","id":2,"method":"session/open","params":{
  "workspace":"/workspace",
  "model":"openai/deepseek-v4.1-flash",
  "agent":"build",
  "approval":"yolo",
  "sandbox":"full-host",
  "sessionId":"2f1c…",
  "provider":{ … }
}}
```

| Field | Required | Meaning |
|---|---|---|
| `workspace` | yes | Absolute path, must exist and be a directory. Canonicalized. |
| `model` | yes | `provider/model` or a bare model id. Passed to the loop unchanged. |
| `agent` | no | Built-in or configured agent name; defaults to `build`. Unknown → `invalid_request` naming the agent. |
| `approval` | no | `yolo`, `ask`, or another `ApprovalMode` the build supports. Default is the loop's own default. |
| `sandbox` | no | `full-host`, `workspace-write`, or another `SandboxProfile` the build supports. |
| `sessionId` | no | Stable id chosen by the host. Generated when absent. Never a credential. |
| `provider` | no | An [inline provider descriptor](#the-provider-descriptor). Absent → CodeGG uses its own configuration. |

Result:

```json
{"jsonrpc":"2.0","id":2,"result":{
  "sessionId":"2f1c…","model":"openai/deepseek-v4.1-flash",
  "agent":"build","workspace":"/workspace"
}}
```

The descriptor is validated before anything else happens. A rejected descriptor
names the field, never the value, and leaves no session behind.

### `turn/submit`

```json
{"jsonrpc":"2.0","id":3,"method":"turn/submit","params":{
  "sessionId":"2f1c…","prompt":"write a hello.txt containing ok"
}}
```

| Field | Required | Meaning |
|---|---|---|
| `sessionId` | yes | The open session. |
| `prompt` | yes | The user message, ≤ 1 MiB. |
| `turnId` | no | Host-chosen id; generated when absent. Reusing an id that is active is refused. |

Result:

```json
{"jsonrpc":"2.0","id":3,"result":{"turnId":"t1"}}
```

The result acknowledges admission. Everything the turn produces arrives as
notifications, in order, and the turn ends with exactly one `turn/done` or
`turn/error`.

### `turn/cancel`

```json
{"jsonrpc":"2.0","id":4,"method":"turn/cancel","params":{"sessionId":"2f1c…","turnId":"t1"}}
```

Result: `{"cancelled":true}`.

Cancellation is cooperative and bounded: the request is acknowledged
immediately, the loop stops at its next safe point, and the turn ends with
`turn/done` carrying `stopReason: "cancelled"`. Cancelling an unknown or already
finished turn is not an error — it returns `{"cancelled":false}`. The session
stays usable and the next `turn/submit` is accepted.

### `session/close`

```json
{"jsonrpc":"2.0","id":5,"method":"session/close","params":{"sessionId":"2f1c…"}}
```

Result: `{"closed":true}`. Nothing is persisted. After `session/close` the
process exits `0`; a second call is refused with `invalid_request`.

## Notifications

Every notification carries `sessionId` and `turnId`, so a host with one session
still correlates without guessing.

### `turn/text`

```json
{"jsonrpc":"2.0","method":"turn/text","params":{"sessionId":"2f1c…","turnId":"t1","delta":"I'll "}}
```

Assistant text, streaming. Deltas are fragments, not whole messages; a host
concatenates them. Reasoning deltas are not part of v1.

### `turn/tool`

```json
{"jsonrpc":"2.0","method":"turn/tool","params":{
  "sessionId":"2f1c…","turnId":"t1","phase":"started",
  "toolId":"call_1","toolName":"write","arguments":"{\"path\":\"hello.txt\"}"
}}
```

```json
{"jsonrpc":"2.0","method":"turn/tool","params":{
  "sessionId":"2f1c…","turnId":"t1","phase":"finished",
  "toolId":"call_1","toolName":"write","success":true,
  "output":"wrote 3 bytes","truncated":false
}}
```

`arguments` is the provider's raw JSON argument string. `output` is bounded to
8 KiB; when it is cut, `truncated` is `true`. A tool that fails sets
`success: false`; it does not end the turn.

### `turn/usage`

```json
{"jsonrpc":"2.0","method":"turn/usage","params":{
  "sessionId":"2f1c…","turnId":"t1","reported":true,
  "inputTokens":812,"outputTokens":96,"cachedTokens":640
}}
```

Emitted at most once per turn, before `turn/done`. When the provider reports no
usage, `reported` is `false` and the counters are absent — never zero, because
zero is a real value a host may budget against.

### `turn/done`

```json
{"jsonrpc":"2.0","method":"turn/done","params":{
  "sessionId":"2f1c…","turnId":"t1","stopReason":"end_turn",
  "usage":{"reported":true,"inputTokens":812,"outputTokens":96}
}}
```

`stopReason` is one of `end_turn`, `max_tokens`, `max_turns`, `cancelled`,
`error`. `usage` repeats the `turn/usage` payload so a host that ignored
`turn/usage` is still correct.

### `turn/error`

```json
{"jsonrpc":"2.0","method":"turn/error","params":{
  "sessionId":"2f1c…","turnId":"t1","class":"auth",
  "message":"upstream rejected the credential","retryable":false,"providerStatus":401,
  "usage":{"reported":false}
}}
```

A provider or turn failure ends the **turn**, not the process: the session stays
open, and the next `turn/submit` is accepted. `message` is redacted and is never
the only signal — `class` is the actionable field.

## Errors

A failed request returns a JSON-RPC error object. Every application-level error
carries `data.class`:

```json
{"jsonrpc":"2.0","id":3,"error":{
  "code":-32000,"message":"turn already active",
  "data":{"class":"busy","retryable":true}
}}
```

| `code` | Used for |
|---|---|
| `-32600` | Malformed request object. |
| `-32601` | Unknown method. |
| `-32602` | Invalid params, including descriptor validation and an unknown agent. |
| `-32603` | Internal defect. |
| `-32000` | Any application-level failure; `data.class` names it. |
| `-32001` | Busy: a turn is already active. |
| `-32002` | Not initialized. |
| `-32003` | Unsupported capability. |

Error classes in v1:

| Class | Meaning | `retryable` |
|---|---|---|
| `auth` | The upstream rejected the credential. | no |
| `model_not_found` | The upstream does not have that model id. | no |
| `model_not_supported` | The model exists but the request shape is unsupported (for example a prefill refusal). | no |
| `context_length` | The request exceeds the model's context. | no |
| `rate_limit` | The upstream is throttling. | yes |
| `invalid_request` | The request or the descriptor is malformed. Names a field, never a value. | no |
| `transport` | The upstream could not be reached, or the stream broke. | yes |
| `protocol` | The upstream answered something this build cannot parse. | no |
| `upstream` | Any other provider-reported failure. | no |
| `busy` | A turn is already active. | yes |
| `cancelled` | The turn was cancelled. Reported as `turn/done`, not an error frame. | — |
| `workspace` | The workspace root is missing, not a directory, or not accessible. | no |
| `not_initialized` | `initialize` has not succeeded. | no |
| `unsupported_capability` | A required capability is not advertised. | no |
| `internal` | A CodeGG defect. | no |

## The provider descriptor

The descriptor is how a host says "use the connection I already have". It
travels inside `session/open` on stdin; it is never a file, never an argv value
and never persisted.

```json
{
  "id": "eggui",
  "displayName": "EggUI",
  "baseUrl": "https://opencode.ai/inference/go/openai/v1/",
  "protocol": "chat_completions",
  "headers": [{"name": "x-opencode-session", "value": "9b1d…"}],
  "credential": {"kind": "api_key", "value": "sk-…"},
  "models": [
    {"id": "deepseek-v4.1-flash", "contextWindow": 128000,
     "maxOutputTokens": 8192, "supportsTools": true}
  ]
}
```

| Field | Required | Rules |
|---|---|---|
| `id` | yes | `[a-z0-9._-]`, 1–64 characters. Registers as a provider id for this session only. |
| `displayName` | no | ≤ 64 characters, diagnostics only. |
| `baseUrl` | yes | Absolute `http`/`https` URL. |
| `protocol` | yes | v1 allows `chat_completions` only. |
| `headers` | no | At most 16 `{name, value}` pairs. Valid HTTP header names, no duplicates, no CR/LF in a value, ≤ 8 KiB total. Auth, content-type and user-agent names are reserved and refused. |
| `credential` | yes | `{"kind":"api_key","value":"…"}` or `{"kind":"env","name":"VAR"}`. The `env` form is resolved in the child's environment at registration; it is never stored. |
| `models` | no | At most 64 entries. An entry supplies facts CodeGG would otherwise guess. An empty list is legal and falls back to CodeGG's own resolution. |
| `models[].id` | yes | Any model id the endpoint accepts. It is **not** checked against a built-in list. |
| `models[].contextWindow` | no | Tokens. |
| `models[].maxOutputTokens` | no | Tokens reserved for the response. |
| `models[].supportsTools` | no | Defaults to the provider's capability when absent. |

Rules that a host must not work around:

- **A descriptor declares its wire surface.** CodeGG never guesses one. Built-in
  providers keep the reviewed-wire-mapping rule; only an explicit descriptor
  declaration bypasses it.
- **A descriptor is per session.** It is never installed globally, never merged
  into configuration and never reused by a second session.
- **The model id is passed unchanged.** CodeGG does not remap it to a catalogue
  entry.
- **A rejected descriptor names fields, not values.** A host may log the error
  safely.

## Worked example

A complete session: handshake, descriptor, one streamed turn that writes a file,
a cancelled turn, and a close. Frames are shown one per line; ids are the host's.

```text
-> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientName":"example"}}
<- {"jsonrpc":"2.0","id":1,"result":{"apiVersion":1,"codeggVersion":"0.1.0","capabilities":["descriptor","streaming","cancellation","usage"]}}

-> {"jsonrpc":"2.0","id":2,"method":"session/open","params":{"workspace":"/workspace","model":"openai/deepseek-v4.1-flash","agent":"build","approval":"yolo","sandbox":"full-host","provider":{"id":"example","baseUrl":"https://example.invalid/v1","protocol":"chat_completions","headers":[{"name":"x-tenant","value":"demo"}],"credential":{"kind":"env","name":"EXAMPLE_API_KEY"},"models":[{"id":"deepseek-v4.1-flash","contextWindow":128000,"maxOutputTokens":8192,"supportsTools":true}]}}}
<- {"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1","model":"openai/deepseek-v4.1-flash","agent":"build","workspace":"/workspace"}}

-> {"jsonrpc":"2.0","id":3,"method":"turn/submit","params":{"sessionId":"s1","turnId":"t1","prompt":"write hello.txt containing ok"}}
<- {"jsonrpc":"2.0","id":3,"result":{"turnId":"t1"}}
<- {"jsonrpc":"2.0","method":"turn/text","params":{"sessionId":"s1","turnId":"t1","delta":"Writing it."}}
<- {"jsonrpc":"2.0","method":"turn/tool","params":{"sessionId":"s1","turnId":"t1","phase":"started","toolId":"call_1","toolName":"write","arguments":"{\"path\":\"hello.txt\",\"content\":\"ok\"}"}}
<- {"jsonrpc":"2.0","method":"turn/tool","params":{"sessionId":"s1","turnId":"t1","phase":"finished","toolId":"call_1","toolName":"write","success":true,"output":"wrote 2 bytes","truncated":false}}
<- {"jsonrpc":"2.0","method":"turn/usage","params":{"sessionId":"s1","turnId":"t1","reported":true,"inputTokens":812,"outputTokens":96}}
<- {"jsonrpc":"2.0","method":"turn/done","params":{"sessionId":"s1","turnId":"t1","stopReason":"end_turn","usage":{"reported":true,"inputTokens":812,"outputTokens":96}}}

-> {"jsonrpc":"2.0","id":4,"method":"turn/submit","params":{"sessionId":"s1","turnId":"t2","prompt":"summarise the whole repository"}}
<- {"jsonrpc":"2.0","id":4,"result":{"turnId":"t2"}}
-> {"jsonrpc":"2.0","id":5,"method":"turn/cancel","params":{"sessionId":"s1","turnId":"t2"}}
<- {"jsonrpc":"2.0","id":5,"result":{"cancelled":true}}
<- {"jsonrpc":"2.0","method":"turn/done","params":{"sessionId":"s1","turnId":"t2","stopReason":"cancelled","usage":{"reported":false}}}

-> {"jsonrpc":"2.0","id":6,"method":"session/close","params":{"sessionId":"s1"}}
<- {"jsonrpc":"2.0","id":6,"result":{"closed":true}}
```

A busy refusal, from a second submit while `t1` is running:

```text
<- {"jsonrpc":"2.0","id":7,"error":{"code":-32001,"message":"a turn is already active","data":{"class":"busy","retryable":true}}}
```

## Security rules

- **A credential travels on stdin.** Never on argv, never in a file, never in an
  environment variable that CodeGG writes, never in a log.
- **Header values are secrets.** A host may put a credential in one; CodeGG
  treats every value as secret and never logs it, echoes it in an error, or
  includes it in a frame dump.
- **Frames are not logged by default.** A debug mode prints method names, ids and
  durations only.
- **stderr is redacted.** A host may capture stderr for diagnostics without
  risking a secret.
- **The descriptor is not persisted.** No session, connection, model or header
  from a descriptor is written to disk.
- **The workspace root is the only writable root** the turn is given; the
  sandbox profile decides how strictly that is enforced.

## Versioning

- `apiVersion: 1` as returned by `initialize` is the contract's major version.
- **Additive changes are allowed within 1**: a new optional field, a new
  notification kind, a new error class. A host ignores what it does not know.
- **Removals and renames require version 2**: any method, notification, required
  field or error class. `initialize` is how a host discovers which version it is
  talking to, so a host pinned to an older SDK refuses rather than misbehaves.
- **One definition.** The wire types live in `crates/codegg-host-protocol` and
  are used by both the host binary and `crates/codegg-sdk`. A second copy is a
  defect.
- **Golden frames are the gate.** Every message has a serialized fixture; a
  non-additive change fails it.
- **Behaviour is not the wire.** Which tools exist, which models a built-in
  provider accepts, and how the loop plans are not part of this contract.

## Operational bounds

| Bound | Value |
|---|---|
| Frame size, both directions | 8 MiB |
| Prompt length | 1 MiB |
| Descriptor headers | 16 pairs, 8 KiB total |
| Descriptor models | 64 |
| Tool output in `turn/tool` | 8 KiB, then `truncated: true` |
| Cancellation to `turn/done` | 5 s at the loop's next safe point |
| Concurrent turns per session | 1 |
| Concurrent sessions per process | 1 |

A provider failure, a cancellation or a crash inside a session never takes the
process down with it, except an unparseable inbound frame, which does.
