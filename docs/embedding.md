# Embedding CodeGG: the host protocol

`codegg host --stdio` runs one agent session in one process, driven by a host
program such as an editor, desktop app, local service, or CI job. It uses
newline-delimited JSON-RPC 2.0 over stdin and stdout. Rust hosts can use the
shared wire types in `crates/codegg-host-protocol` and the client in
`crates/codegg-sdk`.

The host protocol is separate from ACP and the daemon frontend protocol. It
does not need a daemon or a Git repository. Include an inline provider
descriptor to run without a configuration file; if the descriptor is omitted,
CodeGG loads its own configuration.

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
- **The workspace root is the turn's workspace.** It must be an absolute path
  that exists and is a directory. CodeGG canonicalizes it before use and does
  not require a Git repository. The sandbox profile controls OS-level process
  containment: `read-only` and `workspace-write` request it, while `full-host`
  explicitly disables CodeGG filesystem containment.
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
- An inbound frame is limited to 8 MiB. A larger frame or an unparseable line
  is refused with `invalid_request` and ends the connection.
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
| `cancellation` | `turn/cancel` requests cooperative cancellation of the active turn. |
| `usage` | `turn/usage` and `turn/done` report provider usage. |

A host that lists a capability CodeGG does not advertise is refused with
`unsupported_capability` rather than downgraded silently. Listing a capability
is a requirement, not a hint.

## Methods

### `session/open`

```json
{"jsonrpc":"2.0","id":2,"method":"session/open","params":{
  "workspace":"/workspace",
  "model":"eggui/deepseek-v4.1-flash",
  "agent":"build",
  "approval":"auto",
  "sandbox":"workspace-write",
  "sessionId":"2f1c…",
  "provider":{ … }
}}
```

| Field | Required | Meaning |
|---|---|---|
| `workspace` | yes | Absolute path, must exist and be a directory. Canonicalized. |
| `model` | yes | `provider/model` or a bare model id. An inline descriptor fixes the provider; use its `id` as any prefix. CodeGG sends the selected model id to that endpoint without built-in catalog mapping. |
| `agent` | no | Built-in or configured agent name; defaults to `build`. Unknown → `invalid_request` naming the agent. |
| `approval` | no | `yolo`, `ask`, `auto`, or another `ApprovalMode` the build supports. Default is the loop's own default. |
| `sandbox` | no | `full-host`, `workspace-write`, or another `SandboxProfile` the build supports. |
| `sessionId` | no | Stable id chosen by the host. Generated when absent. Never a credential. |
| `provider` | no | An [inline provider descriptor](#the-provider-descriptor). Absent → CodeGG uses its own configuration. |

Result:

```json
{"jsonrpc":"2.0","id":2,"result":{
  "sessionId":"2f1c…","model":"eggui/deepseek-v4.1-flash",
  "agent":"build","workspace":"/workspace"
}}
```

CodeGG validates the descriptor before building the session. Malformed shapes
receive a generic error; field-validation errors name the invalid field without
echoing its value. A rejected descriptor leaves no session open.

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

Cancellation is cooperative. The request is acknowledged immediately, and the
loop stops when it next observes cancellation. The turn then ends with
`turn/done` carrying `stopReason: "cancelled"`. Cancelling an unknown or
finished turn returns `{"cancelled":false}`. The session remains usable for the
next `turn/submit`.

### `session/close`

```json
{"jsonrpc":"2.0","id":5,"method":"session/close","params":{"sessionId":"2f1c…"}}
```

Result: `{"closed":true}`. Host session state is not persisted. After a
successful `session/close`, the process exits `0` and does not process further
requests.

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

For a successful or cancelled turn, `turn/usage` is emitted before
`turn/done`. If the provider reports no usage, `reported` is `false` and the
counters are absent, not zero. A failed turn reports usage in `turn/error`.

### `turn/done`

```json
{"jsonrpc":"2.0","method":"turn/done","params":{
  "sessionId":"2f1c…","turnId":"t1","stopReason":"end_turn",
  "usage":{"reported":true,"inputTokens":812,"outputTokens":96}
}}
```

`stopReason` is one of `end_turn`, `max_tokens`, `max_turns`, or `cancelled`.
A failed turn uses `turn/error` instead. The `usage` field repeats the
`turn/usage` payload so a host that ignored `turn/usage` is still correct.

### `turn/error`

```json
{"jsonrpc":"2.0","method":"turn/error","params":{
  "sessionId":"2f1c…","turnId":"t1","class":"auth",
  "message":"upstream rejected the credential","retryable":false,"providerStatus":401,
  "usage":{"reported":false}
}}
```

A provider or turn failure ends the **turn**, not the process: the session stays
open, and the next `turn/submit` is accepted. `message` is a fixed, secret-safe
summary; use `class` as the actionable field.

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
| `invalid_request` | The request or descriptor is malformed. Descriptor validation names the field and does not echo its value. | no |
| `transport` | The upstream could not be reached, or the stream broke. | yes |
| `protocol` | The upstream answered something this build cannot parse. | no |
| `upstream` | Any other provider-reported failure. | no |
| `busy` | A turn is already active. | yes |
| `cancelled` | The turn was cancelled. Reported as `turn/done`, not an error frame. | not applicable |
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
| `credential` | yes | `{"kind":"api_key","value":"…"}` or `{"kind":"env","name":"VAR"}`. The `env` form reads that variable from CodeGG's environment at registration; the credential is never persisted. |
| `models` | no | At most 64 entries. Optional entries supply model metadata; an empty list is legal and CodeGG tries model discovery at the endpoint. |
| `models[].id` | yes | Any model id the endpoint accepts. It is **not** checked against a built-in list. |
| `models[].contextWindow` | no | Tokens. |
| `models[].maxOutputTokens` | no | Tokens reserved for the response. |
| `models[].supportsTools` | no | Defaults to `true` when absent. |

Rules that a host must not work around:

- **A descriptor declares its wire surface.** CodeGG never guesses one. Built-in
  providers keep the reviewed-wire-mapping rule; only an explicit descriptor
  declaration bypasses it.
- **A descriptor is per session.** It is never installed globally, never merged
  into configuration and never reused by a second session.
- **The model id is passed unchanged.** CodeGG does not remap it to a catalogue
  entry.
- **Descriptor errors do not echo secret values.** Shape errors are generic;
  field-validation errors identify the invalid field. A host may log the error
  safely.

## Worked example

A complete session: handshake, descriptor, one streamed turn that writes a file,
a cancelled turn, and a close. Frames are shown one per line; ids are the host's.

```text
-> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"clientName":"example"}}
<- {"jsonrpc":"2.0","id":1,"result":{"apiVersion":1,"codeggVersion":"0.1.0","capabilities":["descriptor","streaming","cancellation","usage"]}}

-> {"jsonrpc":"2.0","id":2,"method":"session/open","params":{"workspace":"/workspace","model":"example/deepseek-v4.1-flash","agent":"build","approval":"auto","sandbox":"workspace-write","provider":{"id":"example","baseUrl":"https://example.invalid/v1","protocol":"chat_completions","headers":[{"name":"x-tenant","value":"demo"}],"credential":{"kind":"env","name":"EXAMPLE_API_KEY"},"models":[{"id":"deepseek-v4.1-flash","contextWindow":128000,"maxOutputTokens":8192,"supportsTools":true}]}}}
<- {"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1","model":"example/deepseek-v4.1-flash","agent":"build","workspace":"/workspace"}}

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
- **Request bodies are not logged by the host request trace.** At debug level,
  it records the method name and request id, not the frame body.
- **stderr is redacted.** A host may capture stderr for diagnostics without
  risking a secret.
- **The descriptor is not persisted.** No session, connection, model or header
  from a descriptor is written to disk.
- **The workspace root scopes the turn's workspace access.** For child
  processes, `read-only` and `workspace-write` request OS filesystem
  containment. `full-host` explicitly disables CodeGG filesystem containment
  and gives the process the OS user's host authority.

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
- **Golden-frame tests protect key wire shapes.** The protocol crate checks
  representative initialization, notification, and error frames. Update those
  tests when changing the wire.
- **Behaviour is not the wire.** Which tools exist, which models a built-in
  provider accepts, and how the loop plans are not part of this contract.

## Operational bounds

| Bound | Value |
|---|---|
| Inbound frame size | 8 MiB |
| Prompt length | 1 MiB |
| Descriptor headers | 16 pairs, 8 KiB total |
| Descriptor models | 64 |
| Tool output in `turn/tool` | 8 KiB, then `truncated: true` |
| Cancellation | Cooperative; completes when the loop observes the request |
| Concurrent turns per session | 1 |
| Concurrent sessions per process | 1 |

A provider or turn failure is reported without closing the process. End-of-file
and malformed or oversized inbound frames end the process; operating-system or
I/O failures can also terminate it.

## Source verification

Checked 2026-11-15 against `src/host/mod.rs`, `src/host/session.rs`,
`src/host/turn.rs`, `crates/codegg-host-protocol/src/lib.rs`,
`crates/codegg-providers/src/inline.rs`, `src/headless.rs`, and
`crates/codegg-core/src/approval.rs`.

- The host's one-session lifecycle, config selection, framing, event flow, and
  termination behavior are implemented in `src/host/mod.rs`,
  `src/host/session.rs`, and `src/host/turn.rs`.
- Wire limits, request types, errors, and notifications are defined in
  `crates/codegg-host-protocol/src/lib.rs`; the frame-size limit is enforced
  on inbound frames.
- Descriptor validation and secret redaction are in
  `crates/codegg-providers/src/inline.rs`.
- Corrected the opening to distinguish descriptor-backed sessions from
  configuration-backed sessions, clarified that `full-host` disables
  filesystem containment, and removed the unsupported fixed cancellation
  deadline. The sample now uses `auto` approval with `workspace-write`.
