# LSP Protocol Peer Hardening Plan

## Purpose

Harden Codegg's existing LSP runtime so it behaves as a correct bidirectional JSON-RPC/LSP peer rather than only as a client that sends requests and consumes responses and diagnostics.

This is Phase 1 of the current LSP roadmap. The repository already has a substantial agent-facing semantic layer: compact diagnostics, semantic context, security context, hierarchy queries, preview-only workspace edits, overlays, and hunk-aware source navigation. This pass must not add more model-facing LSP operations. It should strengthen the lower-level protocol and service runtime that those features depend on.

The target architecture remains:

```text
model / agent / security workflow
             |
             v
       src/tool/lsp.rs
             |
             v
   semantic/context collectors
             |
             v
      egglsp::LspService
             |
             v
       egglsp::LspClient
       - outgoing requests
       - outgoing notifications
       - incoming responses
       - incoming notifications
       - incoming server requests
       - cancellation
             |
             v
   external language server process
```

The authoritative implementation remains `crates/egglsp`. Do not create a second LSP client under `src/lsp`, move raw protocol handling into the TUI, or bypass the existing read-only preview/apply-patch boundary.

## Baseline

Plan against `main` after commit:

```text
e9a857e3accfcc051a0f2648f57b8d8759d6f2c6
```

Relevant files currently include:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/capability.rs
crates/egglsp/src/config.rs
crates/egglsp/src/error.rs
crates/egglsp/src/lib.rs
src/tool/lsp.rs
src/lsp/mod.rs
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
```

The current implementation already has a dedicated background stdout reader, a pending-request map keyed by numeric request ID, asynchronous notification dispatch for `textDocument/publishDiagnostics`, request timeouts, and a client-per-root/server service map. Preserve those foundations.

## Current Problems

### 1. Server-originated requests are misclassified

`classify_json_rpc_message` currently distinguishes responses, error responses, notifications, and unknown messages using the presence of `id` and `method`. A valid server request contains both an `id` and a `method`, but the current response branch wins whenever an ID is present. Server requests can therefore be treated as responses to nonexistent pending client requests and silently dropped.

Typical server-originated requests include:

```text
workspace/configuration
workspace/workspaceFolders
client/registerCapability
client/unregisterCapability
window/workDoneProgress/create
workspace/applyEdit
```

Some servers tolerate a minimal client. Others require valid responses during initialization or normal operation.

### 2. The background reader cannot send JSON-RPC responses

The background reader owns stdout and can route messages, while normal request/notification methods serialize writes through the process/stdin mutex. There is no explicit path for the reader-side server-request dispatcher to construct and send success or error responses.

### 3. Dynamic registrations are not represented

The initialize request advertises mostly static capability behavior, but servers may still send registration requests. Codegg currently has no runtime record of dynamic registrations and no explicit acknowledgement policy.

### 4. Timed-out requests are abandoned locally but not cancelled remotely

`send_request` removes timed-out entries from `pending`, so late responses are safely ignored. The server may nevertheless continue expensive work. Codegg should issue `$/cancelRequest` on timeout and on explicit caller cancellation where practical.

### 5. Client creation is not single-flight

`LspService::get_or_create_client` checks for an existing key, releases the map lock, initializes a process, and then inserts it. Concurrent first requests for the same `{project_root}:{server_id}` key can launch duplicate language-server processes.

### 6. The global client map is held across asynchronous client work

Several service methods acquire the `clients` write lock and retain it while awaiting operations on one client. This serializes unrelated roots and language servers behind process I/O and makes future lifecycle supervision harder.

## Non-Goals

Do not implement the deterministic fake-server integration harness in this pass. That is Phase 2. Add focused unit and component tests using in-memory writers, recording handlers, and controlled async seams, but do not build a full child-process fixture yet.

Do not add automatic server restart/supervision. This pass may introduce lifecycle-friendly handles or state needed for later work, but restart policy belongs to a later phase.

Do not add pull diagnostics, incremental synchronization, multi-file overlays, hunk clustering, or new semantic operations.

Do not execute server-supplied commands or edits.

Do not accept `workspace/applyEdit` as an implicit mutation path. It must be rejected safely.

Do not expose completion, arbitrary code actions, semantic tokens, progress payloads, or dynamic capability details directly to the model.

Do not weaken root authorization, preview-only edit semantics, or normal Codegg permission handling.

## Required Invariants

The implementation must preserve these invariants:

1. Exactly one task owns and reads a server's stdout.
2. All writes to server stdin are serialized.
3. A JSON-RPC request ID belongs to one side: client-originated IDs are tracked in `pending`; server-originated IDs are answered but never inserted into `pending`.
4. Every recognized server request receives exactly one JSON-RPC response.
5. Unknown server requests receive a JSON-RPC `MethodNotFound` error response rather than being dropped.
6. Notifications never receive responses.
7. Timed-out client requests are removed from `pending` before cancellation is sent.
8. Late responses after timeout or cancellation are ignored without affecting a newer request.
9. `workspace/applyEdit` never writes files and never invokes Codegg mutation tools implicitly.
10. Concurrent initialization for the same service key launches at most one language-server process.
11. Operations against independent clients are not serialized by the global service map lock.
12. Shutdown cannot deadlock behind a request or notification write.

## Phase 1 — Establish a Typed Incoming JSON-RPC Model

Refactor the message classifier in `crates/egglsp/src/client.rs` to distinguish all protocol directions explicitly.

Recommended shape:

```rust
pub enum IncomingJsonRpcMessage {
    Response {
        id: JsonRpcId,
        result: serde_json::Value,
    },
    ErrorResponse {
        id: JsonRpcId,
        code: Option<i64>,
        message: String,
        data: Option<serde_json::Value>,
    },
    ServerRequest {
        id: JsonRpcId,
        method: String,
        params: serde_json::Value,
    },
    Notification {
        method: String,
        params: serde_json::Value,
    },
    Unknown,
}
```

Use a JSON-RPC ID representation that can preserve both numeric and string IDs if the current `lsp-types`/serde stack permits it cleanly. LSP conventionally uses integer IDs, but JSON-RPC permits strings. Do not coerce unknown string IDs to zero or discard them.

Classification order must be structural:

```text
id + method                  => server request
id + error                   => error response
id + result (or null result) => success response
method without id            => notification
otherwise                    => unknown
```

Do not classify a message as a successful response merely because it contains an ID. Require an explicit response shape.

Add pure tests for:

- numeric success response;
- string-ID success response if supported;
- error response with code/message/data;
- server request with ID and method;
- notification with method and no ID;
- malformed object with ID only;
- malformed object with method of the wrong type;
- request with omitted params, normalized to `null` or an empty object according to one documented convention;
- response with `result: null`.

Acceptance criteria:

- Valid server requests can no longer enter the pending-response branch.
- Existing diagnostics notification tests remain valid.
- Existing request response/error routing remains behaviorally compatible.

## Phase 2 — Introduce a Shared Serialized Writer

Extract stdin writing from the process-wide mutex into a clearly owned writer abstraction. The exact representation may be:

```rust
pub struct LspWriter {
    stdin: tokio::sync::Mutex<tokio::process::ChildStdin>,
}
```

or an equivalent `Arc<Mutex<...>>` shared by the client and background server-request dispatcher.

The process handle should retain child lifecycle ownership, but protocol writes should not require mutable access to the entire `LspProcess`.

Provide helpers for:

```rust
async fn send_raw_message(&self, value: &serde_json::Value) -> Result<(), LspError>;
async fn send_request_message(...);
async fn send_notification_message(...);
async fn send_response_result(id, result);
async fn send_response_error(id, code, message, data);
```

Continue using Content-Length framing through one canonical implementation. Do not duplicate framing logic in the server-request handler.

Ensure errors include sufficient context to identify whether the failed write was a request, notification, success response, or error response without logging potentially large or sensitive payloads.

Add component tests around a generic async writer or in-memory duplex stream. The tests should verify:

- correct `Content-Length` bytes;
- exact JSON-RPC envelope for result responses;
- exact JSON-RPC envelope for error responses;
- concurrent writes do not interleave frames;
- Unicode body length uses encoded byte length, not character count;
- write failure is surfaced as `LspError`.

Acceptance criteria:

- Normal client requests and notifications use the shared writer.
- The background reader can answer server requests without accessing stdout or taking a process-wide mutable lock.
- Two concurrent writes always produce two valid, non-interleaved frames.

## Phase 3 — Add a Bounded Server-Request Dispatcher

Create a dedicated dispatcher in `client.rs` or a focused new module such as:

```text
crates/egglsp/src/server_request.rs
```

Recommended interface:

```rust
pub struct ServerRequestContext {
    pub server_id: String,
    pub root: PathBuf,
    pub configuration: serde_json::Value,
    pub workspace_folders: Vec<WorkspaceFolder>,
    pub dynamic_registrations: Arc<RwLock<DynamicRegistrationState>>,
}

pub enum ServerRequestReply {
    Result(serde_json::Value),
    Error {
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
}

pub async fn dispatch_server_request(
    context: &ServerRequestContext,
    method: &str,
    params: serde_json::Value,
) -> ServerRequestReply;
```

Keep the dispatcher deterministic, bounded, and free of model calls or filesystem mutation.

### `workspace/configuration`

Parse the requested `ConfigurationItem[]` and return one value per item, preserving order.

Configuration sources should be limited to existing Codegg LSP configuration and server initialization/configuration data. If no matching section is configured, return `null` for that item. Do not expose the entire Codegg configuration or secrets/environment values.

A minimal first-pass mapping is acceptable:

```text
section matches configured server section => configured JSON value
unknown section                           => null
scope URI outside root                    => null
```

Document exactly which existing config field is used. If the current schema only has `initialization`, either reuse it conservatively or add a separate optional `settings`/`workspace_configuration` field with backward-compatible deserialization. Do not silently reinterpret arbitrary environment overrides as workspace settings.

### `workspace/workspaceFolders`

Return the client's current root as a one-element workspace-folder array, or the explicit set carried by the context if the implementation is made future-ready for multi-root workspaces.

### `client/registerCapability`

Validate and record registrations in `DynamicRegistrationState` keyed by registration ID. Preserve method and register options as opaque JSON. Acknowledge with `null` result.

Recording a registration does not mean Codegg must immediately implement the feature. Capability snapshots should not claim operational support solely because the server registered a method unless Codegg has a corresponding implementation.

### `client/unregisterCapability`

Remove registrations by ID. Unknown IDs should be tolerated and acknowledged. Be liberal in parsing the protocol's historical field spelling differences if required by the current `lsp-types` version, but keep the compatibility code localized and tested.

### `window/workDoneProgress/create`

Acknowledge with `null`. Optionally record the token in a bounded progress-token set for future observability, but do not expose progress to the model in this pass.

### `workspace/applyEdit`

Always return a valid `ApplyWorkspaceEditResponse` equivalent to:

```json
{
  "applied": false,
  "failureReason": "Codegg does not permit implicit language-server edits; request a preview through the LSP tool and apply it through the authorized patch path"
}
```

Do not parse and execute the edit as a side effect. Logging should identify that an edit request was rejected, without dumping full replacement text at normal verbosity.

### Unknown methods

Return JSON-RPC error code `-32601` (`Method not found`) with a concise method-specific message.

### Invalid params

Return `-32602` (`Invalid params`) when a known request has malformed parameters. Do not panic or return an unrelated transport error.

Add pure dispatcher tests for every supported method, unknown methods, malformed params, out-of-root configuration scope, registration replacement/removal, and explicit apply-edit rejection.

Acceptance criteria:

- Every server request receives one success or error response.
- No handler writes files, spawns commands, invokes tools, or calls a model.
- Unknown methods are observable and standards-compliant.
- Dynamic registration state is bounded or otherwise protected against unbounded growth from a misbehaving server.

## Phase 4 — Wire Server Requests into the Background Reader

Extend the background reader to receive all required shared state:

```text
pending client requests
shared writer
server-request context
notification dispatcher state
server identity/generation metadata if available
```

Routing should be:

```text
Response / ErrorResponse
    -> remove matching client pending entry
    -> deliver through oneshot
    -> if no pending entry, log as late/unmatched at debug level

Notification
    -> dispatch asynchronously or inline according to bounded handler policy
    -> never send a response

ServerRequest
    -> dispatch to bounded handler
    -> send exactly one result/error response with original ID

Unknown
    -> debug log only, with payload size/method metadata rather than full body
```

Avoid allowing a slow server request to stop stdout consumption indefinitely. The supported handlers should be fast and local, but still put an explicit timeout around dispatch/response if useful. A timeout must produce an internal-error JSON-RPC response rather than silently abandoning the request.

Do not spawn an unbounded task for every incoming message. Either process bounded local handlers inline or use a bounded queue/semaphore. Preserve response routing latency for normal client requests.

Add tests using recording writer/dispatcher seams that prove:

- a server request is not delivered to a pending client request;
- a server request emits a response with the same ID;
- unknown server request emits `-32601`;
- malformed known request emits `-32602`;
- notification emits no response;
- unmatched late response is ignored;
- handler failure does not terminate the reader loop;
- response write failure causes an explicit reader/client failure path rather than a panic.

Acceptance criteria:

- The background reader remains the sole stdout owner.
- Server-request handling does not regress diagnostics delivery or concurrent response routing.
- Reader termination still fails all pending client requests immediately.

## Phase 5 — Add Client Request Cancellation

Refactor request execution so timeout cleanup and cancellation are explicit and testable.

On timeout:

1. Remove the request ID from `pending`.
2. Send `$/cancelRequest` with the original ID as a best-effort notification.
3. Return the existing `LspError::RequestTimeout` behavior.
4. Ignore any late response.

Cancellation notification failure must not replace the primary timeout error, but it should be logged at debug/warn level with server and method metadata.

Consider a small pending-request guard:

```rust
struct PendingRequestGuard {
    id: JsonRpcId,
    pending: PendingMap,
    armed: bool,
}
```

or equivalent cleanup logic so early write failure, timeout, receiver drop, and future explicit cancellation all remove the pending entry exactly once.

If adding caller-driven cancellation is straightforward, expose an internal cancellable request primitive for future use, but do not widen the public model-facing API solely for this pass.

Add tests proving:

- timeout removes pending state;
- timeout emits `$/cancelRequest` with the correct ID;
- write failure after pending insertion removes the entry;
- late response after timeout is ignored;
- cancellation failure does not mask timeout;
- successful requests do not emit cancellation;
- cancellation and response racing cannot deliver twice or leak an entry.

Acceptance criteria:

- No timeout path leaks pending entries.
- Expensive abandoned requests receive best-effort remote cancellation.
- Existing request timeout error contracts remain compatible.

## Phase 6 — Make Client Initialization Single-Flight

Refactor `LspService` so only one initialization can occur for a `{project_root}:{server_id}` key.

Preferred shape:

```rust
struct ClientSlot {
    state: ClientSlotState,
}

enum ClientSlotState {
    Starting(SharedInitialization),
    Ready(Arc<LspClient>),
    Failed { error: String, failed_at: Instant },
}
```

A simpler per-key mutex or `OnceCell`-style design is acceptable if it supports retry after failure and does not leave a permanently poisoned slot.

Required behavior:

- First caller creates or claims the starting slot.
- Concurrent callers await the same initialization result.
- Successful initialization installs one shared client.
- Failed initialization removes or transitions the slot so a later call can retry according to a documented policy.
- A cancelled initiating task cannot leave a permanent `Starting` tombstone.
- Shutdown and initialization races are deterministic.

Do not hold the global client-map lock while downloading a binary, spawning a process, performing initialize, or sending `initialized`.

Add concurrency tests with an injected/fake client factory or initialization seam. Do not spawn a real language server. Verify:

- N concurrent calls for one key invoke the factory once;
- all callers receive the same logical client/handle;
- two different keys initialize concurrently;
- initialization failure reaches all current waiters;
- a later retry can succeed;
- initiating task cancellation does not deadlock future calls.

Acceptance criteria:

- At most one process can be launched per key by concurrent first-use requests.
- Independent keys are not serialized during initialization.

## Phase 7 — Stop Holding the Global Service Lock Across I/O

Change the service map to store cloneable handles, preferably:

```rust
HashMap<ClientKey, Arc<LspClient>>
```

or an `Arc<ClientHandle>` if the single-flight design introduces a wrapper.

For every service operation:

1. Acquire the map lock.
2. Clone the relevant `Arc`.
3. Release the map lock.
4. Await the client operation.

Audit at least:

```text
open_file
update_file
close_file
save_file
ensure_file_open_from_disk
send_request
capability lookup
diagnostic lookup
shutdown_all
client-key/root-hint lookup
```

Remove index-based selection through `clients.values_mut().nth(index)`. Resolve the exact client key associated with the open URI or maintain an explicit URI-to-client-key index. HashMap iteration order must not participate in client routing.

For `close_file` and `save_file`, choose one deterministic approach:

- derive the root/server key from the path and language; or
- maintain an open-document ownership index; or
- search cloned handles without retaining the map lock, then act on the exact matched handle.

Avoid nested lock-order inversions between the map, per-client document maps, writer, capabilities, diagnostics, and dynamic registrations. Document the intended lock ordering in code.

Add tests proving:

- a slow request on client A does not block lookup/use of client B;
- close/save target the correct client deterministically;
- concurrent open/update operations do not deadlock;
- shutdown obtains a stable handle snapshot, clears/prevents new acquisition according to a documented policy, then awaits clients without retaining the map lock.

Acceptance criteria:

- No service method retains the global map lock across filesystem or process I/O.
- Client routing never depends on HashMap iteration order.
- Existing public service APIs remain compatible where practical.

## Phase 8 — Capability and Configuration Semantics

Review initialize capability advertisement after server-request support lands.

The client currently advertises static synchronization behavior and workspace-folder support. Keep advertisements conservative and truthful.

Required checks:

- Do not advertise dynamic registration support for a feature unless the client can at least accept and track the registration correctly.
- Do not mark an operation supported in `LspCapabilitySnapshot` merely because it was dynamically registered if Codegg lacks an implementation.
- Preserve real initialized server capabilities as the primary source for normal capability gating.
- Keep `workspace/applyEdit` disabled from a mutation perspective even if `workspace.applyEdit` capability fields are later advertised for protocol compatibility.
- Keep configuration values scoped to the selected server/root.

Add a compact internal representation for dynamic registrations if one does not already exist:

```rust
pub struct DynamicRegistration {
    pub id: String,
    pub method: String,
    pub register_options: Option<serde_json::Value>,
}
```

Cap total registrations per client at a generous fixed number, such as 256, or use another bounded policy. On overflow, return an explicit request error or replace according to registration ID; do not permit unbounded memory growth.

Acceptance criteria:

- Initialize capabilities, runtime registrations, and Codegg operation availability have clearly separated semantics.
- Configuration responses cannot leak unrelated Codegg settings or environment secrets.

## Phase 9 — Error Model and Observability

Add structured errors only where they improve routing and tests. Avoid a broad error refactor unrelated to this pass.

Potential additions:

```rust
LspError::Protocol(String)
LspError::ServerRequest(String)
LspError::WriterClosed(String)
LspError::InitializationCancelled(String)
```

Alternatively, retain existing variants but use consistent contextual messages.

Logging requirements:

- `debug`: recognized server request method and outcome;
- `debug`: late/unmatched response ID;
- `debug` or `warn`: cancellation notification failure;
- `warn`: malformed known server request;
- `warn`: server-request response write failure;
- no full workspace edits, source replacements, configuration values, or arbitrary server payloads at normal verbosity;
- include server ID and root/key where available;
- include payload byte size when useful.

Expose enough state for later doctor/lifecycle work, but do not implement the full status subsystem in this pass. Useful internal counters may include:

```text
server_requests_received
server_requests_succeeded
server_requests_failed
unknown_server_requests
client_requests_timed_out
cancellations_sent
dynamic_registration_count
```

Counters are optional if adding them would distort this pass; correct protocol behavior and tests take priority.

## Phase 10 — Documentation

Update:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
```

Document:

- Codegg is a bidirectional JSON-RPC peer;
- the incoming message taxonomy;
- supported server-originated requests;
- explicit `workspace/applyEdit` rejection and why;
- dynamic registration tracking versus actual Codegg operation support;
- timeout cancellation semantics;
- single-flight client initialization;
- global map lock discipline;
- remaining limitations;
- Phase 2 will add the deterministic fake-server integration harness.

Keep documentation aligned with actual implementation. Do not claim protocol-complete LSP support; this pass supports a bounded, explicit subset of common server requests.

## Test Strategy

The default suite must remain independent of installed language servers and network access.

Required test categories:

### Pure parser/classifier tests

- Incoming message classification.
- JSON-RPC ID preservation.
- Invalid response/request shapes.

### Writer tests

- Framing and body byte length.
- Result and error response envelopes.
- Concurrent write serialization.

### Dispatcher tests

- Configuration request.
- Workspace folders.
- Register/unregister capability.
- Work-done progress creation.
- Apply-edit rejection.
- Unknown method and invalid params.

### Reader-routing tests

Use recording seams rather than a real subprocess:

- response to pending request;
- server request to dispatcher/writer;
- notification to notification handler;
- unmatched response;
- handler error and response-write error behavior.

### Cancellation tests

- timeout cleanup;
- `$/cancelRequest` emission;
- race with late response;
- write failure cleanup.

### Service concurrency tests

- single-flight same-key initialization;
- independent-key parallelism;
- retry after failure/cancellation;
- no global lock held across slow client operation;
- deterministic close/save routing.

### Existing regression suite

Run at minimum:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test -p egglsp
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

If the repository's full clippy baseline has unrelated failures, record them precisely and ensure modified crates/files are warning-free. Do not hide new warnings with broad `allow` attributes.

## Suggested Implementation Order

Use this sequence to minimize overlapping refactors:

1. Typed incoming message classifier and tests.
2. Shared serialized writer and framing tests.
3. Pure server-request dispatcher and dynamic registration state.
4. Background-reader routing integration.
5. Timeout cancellation and pending-entry guard.
6. `Arc` client handles and global-lock release discipline.
7. Single-flight initialization.
8. Capability/configuration cleanup.
9. Documentation and final regression pass.

The writer and dispatcher should land before service ownership changes so protocol behavior can be reviewed independently. The single-flight and map-lock work may share a commit if the handle representation makes separating them artificial, but keep the conceptual boundaries explicit in commit messages.

## File-Level Guidance

### `crates/egglsp/src/client.rs`

Expected changes:

- replace/extend `JsonRpcMessage` classifier;
- preserve numeric/string IDs;
- route server requests;
- share a serialized writer;
- issue cancellation notifications;
- remove pending entries on every failure path;
- add focused parser/routing/cancellation tests.

If this file becomes too large, extract protocol types and server-request dispatch rather than adding another large inline section.

### `crates/egglsp/src/launch.rs`

Expected changes:

- expose or relocate canonical framed-write logic;
- allow stdin ownership to be separated cleanly from child lifecycle ownership;
- preserve stderr drain and termination behavior.

### `crates/egglsp/src/service.rs`

Expected changes:

- store cloneable client handles;
- implement single-flight startup;
- release map locks before awaits;
- eliminate HashMap-index-based routing;
- add fake-factory/concurrency test seams.

### `crates/egglsp/src/config.rs`

Possible changes:

- add optional server-scoped workspace configuration/settings if existing initialization options are not semantically appropriate;
- preserve backward compatibility and defaults.

### `crates/egglsp/src/capability.rs`

Possible changes:

- keep dynamic registrations distinct from normalized operational capability snapshots;
- add helpers only if required by the dispatcher or documentation.

### `crates/egglsp/src/error.rs`

Possible changes:

- add narrowly scoped protocol/writer errors if current variants cannot represent failures clearly.

### `crates/egglsp/src/lib.rs`

Expected changes:

- export only the public types needed by Codegg consumers;
- keep low-level dispatcher internals crate-private.

### `src/tool/lsp.rs`

No major feature changes expected. Confirm existing model-facing operations and read-only categorization remain unchanged. Update tests only if lower-level error/output behavior requires it.

## Review Checklist

Before considering this phase complete, review specifically for:

- server requests accidentally entering `pending`;
- responses being sent to notifications;
- missing response on invalid/unknown requests;
- duplicate responses caused by timeout/error paths;
- request IDs truncated or coerced;
- pending entries leaked on stdin write failure;
- cancellation sent before pending removal;
- recursive writer locking;
- background reader blocked by long handler work;
- unbounded task spawning;
- unbounded dynamic registration growth;
- configuration/environment secret leakage;
- implicit application of `workspace/applyEdit`;
- global service lock retained across awaits;
- duplicate client process launch;
- retry poisoned after initialization failure;
- HashMap iteration order used for routing;
- shutdown/init deadlocks;
- documentation overstating protocol support.

## Completion Criteria

Phase 1 is complete when all of the following are true:

1. Incoming JSON-RPC responses, error responses, notifications, and server requests are classified distinctly.
2. Codegg can answer the supported bounded set of server requests over the same serialized stdin writer.
3. Unknown server requests receive `-32601`; malformed known requests receive `-32602`.
4. `workspace/applyEdit` is explicitly rejected with `applied: false` and performs no mutation.
5. Dynamic registrations can be acknowledged, recorded, removed, and bounded without falsely expanding model-facing capability claims.
6. Timed-out client requests are cleaned up and send best-effort `$/cancelRequest`.
7. Concurrent first use of one root/server key launches one client process.
8. Independent clients can perform work without being serialized by the global service map lock.
9. Client routing does not depend on HashMap iteration order.
10. The default unit/workspace test suite requires no external language server or network.
11. Existing semantic context, security context, preview-only edits, diagnostics, hierarchy, and hunk-source workflows remain behaviorally compatible.
12. Architecture and user-facing LSP documentation describe the implemented behavior and remaining limitations accurately.

## Handoff Result

At the end of this pass, Codegg should still expose the same high-level LSP feature set, but those features will run on a materially stronger foundation: a bidirectional protocol peer with safe server-request handling, explicit cancellation, deterministic client ownership, and concurrency behavior suitable for the later fake-server harness, lifecycle supervision, version-aware diagnostics, and broader real-server interoperability work.
