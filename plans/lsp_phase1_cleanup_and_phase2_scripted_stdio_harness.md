# LSP Phase 1 Cleanup and Phase 2 Scripted Stdio Harness

## Purpose

Close the remaining Phase 1 cleanup items after:

```text
bbfbe5b1dff71c57bd8d90e0f9d5807c83be2cf0
```

and then implement Phase 2 of the LSP roadmap: a deterministic, scripted, child-process LSP server harness that exercises Codegg’s real stdio transport, Content-Length framing, request routing, notification handling, server-originated requests, cancellation, failure propagation, and shutdown behavior.

The current LSP runtime is now structurally mature:

- bidirectional JSON-RPC message classification;
- server-originated request handling;
- shared serialized writer;
- timeout cancellation;
- deterministic document ownership;
- explicit single-flight initialization;
- lifecycle-safe publication;
- authoritative task-completion receivers;
- start-registration barriers;
- race-free shutdown observation;
- bounded quiescent shutdown;
- compact semantic and security context layers.

The missing evidence is end-to-end protocol execution through an actual child process. Most current tests exercise pure classifiers, in-memory writers, injected factories, and internal state machines. Phase 2 should prove that the complete launch → initialize → interleaved protocol traffic → shutdown path behaves correctly under realistic byte streams and process lifecycle events.

## Combined Outcome

At completion:

1. Phase 1 documentation and tests accurately describe what is actually exercised.
2. Misleading “forced abort” tests are replaced or renamed.
3. Aggregate cancellation timing is tested across multiple independent initialization tasks.
4. A deterministic fake LSP server executable is available only for tests.
5. Integration tests launch that executable through the same process path used for real servers.
6. The harness can script framing, delays, interleaving, server requests, diagnostics, malformed messages, EOF, crashes, and shutdown.
7. Default tests remain network-free and require no external language server installation.
8. Failures produce useful transcripts rather than opaque timeouts.
9. The repository is ready for a later opt-in real-server compatibility matrix.

## Scope

Likely files and modules:

```text
crates/egglsp/Cargo.toml
crates/egglsp/src/client.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/service.rs
crates/egglsp/src/server.rs
crates/egglsp/src/writer.rs
crates/egglsp/src/error.rs
crates/egglsp/tests/protocol_stdio.rs
crates/egglsp/tests/common/mod.rs
crates/egglsp/tests/common/harness.rs
crates/egglsp/test-fixtures/fake_lsp_server.rs
```

Alternative test-binary placement is acceptable:

```text
crates/egglsp/src/bin/egglsp-test-server.rs
```

or a dedicated workspace crate:

```text
crates/egglsp-test-server/
```

Prefer the smallest structure that guarantees Cargo can build and locate the test server deterministically across macOS, Linux, and CI.

Documentation:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not implement in this phase:

- automatic restart supervision;
- production server health dashboards;
- pull diagnostics;
- incremental document synchronization;
- multi-root workspace support;
- automatic installation of additional real servers;
- broad real-server CI matrices;
- new model-facing LSP tools;
- direct edit application through `workspace/applyEdit`;
- semantic hunk clustering;
- long-running fuzz infrastructure.

The fake server may support test-only protocol methods, but production behavior must remain standards-oriented.

# Part A — Final Phase 1 Cleanup

## A1. Correct Stale Ownership Documentation

Update the `LspService::active_init_tasks` field comment and related documentation.

Current behavior:

```text
InitTaskControl owns:
- CancellationToken
- AbortHandle
- authoritative oneshot Receiver<InitTaskExit>
```

It does not own the real `JoinHandle`.

Ensure all documentation consistently states:

- the wrapper task owns the terminal sender;
- the service owns the terminal receiver and abort handle;
- channel resolution or closure is the authoritative task-terminal observation;
- no forwarding task wraps a real initialization `JoinHandle`.

Audit:

```text
crates/egglsp/src/service.rs
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## A2. Fix Misleading Forced-Abort Test Names and Coverage

The injected factory path is wrapped in an outer cancellation `select!`. A future blocked on `Notify::notified()` or `pending()` is therefore cooperatively dropped when cancellation wins. Existing tests named as forced-abort/global-deadline tests do not necessarily reach the abort branch.

Separate tests into explicit categories.

### Cooperative cancellation tests

Rename or retain tests that use ordinary async pending/notified futures as:

```text
cooperative_cancellation_drops_factory_future
cooperative_shutdown_resolves_waiters
cooperative_shutdown_cleans_task_control
```

Assertions:

- cancellation token wins;
- RAII exit probe runs;
- no client publishes;
- task completion receiver resolves;
- service reaches `Stopped`.

### Forced-abort test

Add one test that genuinely survives cancellation past the 300 ms grace interval.

Safe options:

1. Run the Tokio test on a multi-thread runtime and use `spawn_blocking` or a bounded blocking primitive inside the wrapper path.
2. Add a test-only hook in the wrapper after cancellation observation but before completion send, allowing the wrapper to remain pending until abort.
3. Add a test-only `InitTaskBehavior::IgnoreCancellationUntilAbort` branch around the task wrapper, rather than trying to create an unsafe CPU loop.

Preferred approach: a test-only wrapper hook.

Example:

```rust
#[cfg(test)]
struct InitTaskTestBehavior {
    ignore_cancellation_until_abort: bool,
    entered_uncooperative_region: watch::Sender<bool>,
}
```

The production initialization function remains cancellation-aware. The test wrapper deliberately waits forever after the start barrier and can only terminate when its Tokio task is aborted.

Assert:

- grace deadline expires;
- abort handle is invoked;
- authoritative completion receiver closes/resolves;
- RAII wrapper/future exit probe fires before `shutdown_all()` returns;
- lifecycle reaches `Stopped`;
- no external release signal is needed after shutdown.

## A3. Test Aggregate Grace Across Multiple Independent Tasks

The existing “many tasks” test uses one single-flight leader with several waiters. It does not test aggregate waiting across multiple task controls.

Create several independent initialization keys. Options:

- distinct temporary project roots containing Rust files;
- multiple test-only server definitions;
- a direct test helper that starts controlled attempts under unique keys.

Preferred test:

```text
root-a/src/lib.rs -> rust-analyzer
root-b/src/lib.rs -> rust-analyzer
root-c/src/lib.rs -> rust-analyzer
root-d/src/lib.rs -> rust-analyzer
```

The fake/injected factory should block for each root. Confirm `active_init_tasks.len() == N`, call shutdown, and assert elapsed time is bounded near one aggregate grace period rather than `N × grace`.

Use a tolerant upper bound, for example:

```text
< 2 seconds for N = 8
```

Do not assert exact millisecond timing.

## A4. Clarify Deadline-Fallback Coverage

Either implement a true deadline-fallback test using a test-only completion channel that intentionally never resolves even after abort, or rename current tests to avoid claiming this branch is covered.

A direct test-only harness around `await_init_task_completions()` is acceptable:

- construct `InitTaskControl` values with receivers whose senders are intentionally retained;
- use inert abort handles from test tasks;
- drive the helper to the global deadline;
- verify unresolved controls are logged/returned and state finalization continues.

Do not alter production shutdown semantics solely to make this easy to test.

## A5. Remove Redundant Test Releases and Sleep-Based Ordering

Remove post-shutdown `Notify::notify_waiters()` calls where RAII probes already prove the future was dropped.

Replace avoidable sleeps in shutdown coordination tests with:

- `watch` state observation;
- barriers;
- explicit test hooks;
- lifecycle subscriptions.

A small timing assertion is acceptable only where duration itself is the subject of the test.

## A6. Resolve or Quarantine the Flaky Transport Test

The commit notes a pre-existing flaky test:

```text
client::tests::timeout_cancel_failure_marks_transport_failed_and_writes_writer_closed
```

Investigate whether it depends on OS pipe-buffer behavior after child termination.

Preferred correction:

- inject a deterministic failing writer rather than relying on kernel pipe state;
- use an in-memory writer that returns a controlled error on the cancellation write;
- assert transport failure and pending-drain behavior without process timing.

If the flaky test duplicates deterministic unit coverage, replace it. Do not merely mark it ignored unless there is a linked issue and retained deterministic coverage.

## Phase 1 Cleanup Acceptance Criteria

- Documentation accurately describes completion receiver ownership.
- Cooperative and forced-abort tests are semantically honest.
- At least one test reaches the real abort-after-grace path.
- Aggregate grace is tested with multiple active task controls.
- No test needs a post-shutdown release to let a supposedly aborted task finish.
- The flaky cancellation-write test is deterministic or replaced.

# Part B — Phase 2 Scripted Stdio Fake Server

## B1. Harness Architecture

Build a test-only LSP server executable that communicates exclusively through stdin/stdout using real LSP/JSON-RPC framing.

The executable should:

- read `Content-Length` framed messages from stdin;
- parse JSON-RPC objects;
- execute a deterministic script;
- write framed responses/requests/notifications to stdout;
- optionally write diagnostic traces to stderr;
- exit with configured status;
- never access the network;
- never mutate repository files;
- have no dependence on a real language-server implementation.

Recommended architecture:

```text
Integration test
    |
    | creates Scenario JSON/TOML file or environment payload
    v
LspService / LspClient
    |
    | launches normal child process through launch.rs
    v
fake-lsp-server binary
    |
    | reads scenario
    | exchanges real framed messages
    v
transcript + assertions
```

The fixture must exercise the same `LspProcess`, stdout reader, `LspWriter`, initialization, and shutdown code used in production.

## B2. Scenario Representation

Use a typed scenario model, serializable through JSON or MessagePack. JSON is preferred for debuggability.

Suggested types:

```rust
#[derive(Serialize, Deserialize)]
struct Scenario {
    name: String,
    steps: Vec<ServerStep>,
    exit: ExitBehavior,
    strict: bool,
}

enum ServerStep {
    ExpectRequest {
        method: String,
        id: IdMatcher,
        params: ValueMatcher,
        then: Vec<ServerAction>,
    },
    ExpectNotification {
        method: String,
        params: ValueMatcher,
        then: Vec<ServerAction>,
    },
    ExpectResponse {
        id: IdMatcher,
        result: Option<ValueMatcher>,
        error: Option<ErrorMatcher>,
    },
    AllowAnyNotification {
        methods: Vec<String>,
    },
    Delay {
        millis: u64,
    },
    Barrier {
        name: String,
    },
    ExitNow {
        code: i32,
    },
}

enum ServerAction {
    RespondResult { result: Value },
    RespondError { code: i64, message: String, data: Option<Value> },
    SendNotification { method: String, params: Value },
    SendRequest { id: JsonRpcId, method: String, params: Value },
    SendRawFrame { bytes_base64: String },
    SendJsonWithDeclaredLengthOffset { value: Value, offset: i64 },
    CloseStdout,
    Exit { code: i32 },
    Sleep { millis: u64 },
}
```

Keep matching intentionally small:

```rust
enum ValueMatcher {
    Any,
    Exact(Value),
    ObjectContains(Map<String, ValueMatcher>),
    Null,
}
```

Avoid building a general-purpose test DSL.

## B3. Scenario Delivery

The fake server needs a scenario without changing production CLI arguments for real servers.

Acceptable mechanisms:

1. Environment variable pointing to a temporary scenario file:
   ```text
   CODEGG_FAKE_LSP_SCENARIO=/tmp/.../scenario.json
   ```
2. Test-only command arguments:
   ```text
   fake-lsp-server --scenario /tmp/.../scenario.json
   ```
3. Base64 JSON environment variable for very small scenarios.

Prefer a temporary file plus environment variable. It avoids command-length limits and preserves readable failure artifacts.

The existing server rule/config launch path should be able to specify executable, args, and environment without production-only special cases.

## B4. Test Binary Discovery

Cargo integration tests need a deterministic executable path.

Preferred options, in order:

1. Dedicated workspace binary crate and `CARGO_BIN_EXE_<name>`.
2. `src/bin/egglsp-test-server.rs` with Cargo-provided binary env path.
3. Build script that exports the fixture binary path.

Avoid invoking `cargo run` from tests. That creates nested builds, slow execution, and lock contention.

If `CARGO_BIN_EXE_*` is unavailable for package-local integration tests in the chosen layout, create a tiny workspace crate.

## B5. Harness Controller

Create an integration-test helper:

```rust
struct FakeLspHarness {
    tempdir: TempDir,
    scenario_path: PathBuf,
    transcript_path: PathBuf,
    server_def: &'static LspServerDef or owned test definition,
    service: LspService,
}
```

Responsibilities:

- create a minimal temporary project root;
- write test source files and root markers;
- write scenario JSON;
- configure the fake server executable and environment;
- launch through `LspService` or `LspClient`;
- expose transcript and server stderr after failure;
- guarantee shutdown/child cleanup in test teardown;
- use unique directories per test;
- avoid global environment mutation where tests can run concurrently.

Do not require static registry pollution for the fake server if an owned/injected server definition can be supported cleanly. A narrowly scoped test-only server definition is acceptable.

## B6. Transcript Model

The fake server should write a machine-readable transcript to a path supplied by the harness.

Suggested record:

```rust
struct TranscriptEvent {
    sequence: u64,
    timestamp_micros: u64,
    direction: Direction,
    event: TranscriptKind,
}

enum TranscriptKind {
    FrameReceived { content_length: usize, body: Value },
    FrameSent { content_length: usize, body: Value },
    RawBytesSent { len: usize },
    StepMatched { index: usize },
    StepMismatch { index: usize, reason: String },
    StdinEof,
    StdoutClosed,
    ProcessExit { code: i32 },
}
```

On test failure, include a compact transcript tail in the panic message. Preserve the full transcript in the temp directory for local debugging where practical.

The transcript must not contain arbitrary real repository source in production tests. These fixtures use synthetic source only.

# Part C — Core Protocol Scenarios

## C1. Initialization Handshake

Test the actual startup sequence:

```text
client -> initialize request
server -> initialize result
client -> initialized notification
```

Assert:

- root URI/path is correct;
- process ID and client info fields are structurally valid;
- advertised capabilities are expected;
- initialization options are passed through;
- server capabilities are stored;
- no request is sent before initialization completes except allowed protocol traffic;
- `initialized` is a notification with no ID.

Variants:

- `initialize` result with minimal capabilities;
- full capabilities used by semantic operations;
- initialize error response;
- initialize response delayed near timeout;
- server exits before initialize response.

## C2. Server Request During Initialization

Script:

```text
client -> initialize
server -> workspace/configuration request
client -> configuration response
server -> initialize result
client -> initialized
```

Also test:

- `workspace/workspaceFolders`;
- `client/registerCapability` with multiple registrations;
- `window/workDoneProgress/create`;
- unknown server request returns `-32601`;
- malformed known request returns `-32602`.

This verifies server requests are not misrouted into the pending client-response map.

## C3. Explicit Apply-Edit Refusal

After initialization, send:

```text
server -> workspace/applyEdit
```

Assert Codegg replies with a successful JSON-RPC result:

```json
{
  "applied": false,
  "failureReason": "..."
}
```

Assert:

- no JSON-RPC error envelope;
- no file mutation;
- no patch tool invocation;
- client remains usable afterward.

## C4. Notifications Interleaved with Responses

Script several concurrent client requests. Send:

```text
publishDiagnostics notification
logMessage notification
response for request 2
progress notification
response for request 1
```

Assert:

- diagnostics cache updates;
- request responses reach correct callers despite reversed order;
- notifications never consume pending entries;
- no response is sent to notifications;
- unknown benign notifications do not break the reader loop.

## C5. Concurrent Out-of-Order Responses

Issue N requests concurrently, for example:

```text
textDocument/hover
textDocument/definition
textDocument/references
textDocument/documentSymbol
workspace/symbol
```

The server should return responses in reverse or randomized deterministic order.

Assert:

- every caller receives its own response;
- no pending entries leak;
- string and signed numeric server-request IDs remain distinct from client request IDs;
- duplicate response IDs are ignored/logged after the first completion.

Use a seeded deterministic order, not runtime randomness.

## C6. Diagnostics Lifecycle

Scenarios:

1. diagnostics before any client request is pending;
2. diagnostics interleaved with hover response;
3. empty diagnostics publication clears previous diagnostics;
4. diagnostics for two URIs;
5. versioned diagnostics;
6. diagnostics arriving immediately before shutdown;
7. malformed diagnostics params do not terminate transport.

Assert freshness metadata and cache invalidation behavior where currently implemented.

## C7. Dynamic Registration Lifecycle

Send a registration request with several entries, then an unregistration request using:

```text
unregisterations
```

and a compatibility variant using:

```text
unregistrations
```

Assert:

- all registrations are stored;
- replacements at cap are allowed;
- over-cap batch is atomically rejected;
- unregistration removes all requested IDs;
- runtime operational capability claims do not expand merely because a method was dynamically registered.

## C8. Request Timeout and Remote Cancellation

Script a client request that the server intentionally leaves unanswered.

Assert:

- client timeout occurs;
- pending entry is removed;
- client sends `$/cancelRequest` with the original request ID;
- late response after cancellation is ignored;
- the transport remains healthy when cancellation write succeeds;
- a subsequent request succeeds.

The production timeout is currently long. Add a test-only configurable request timeout rather than waiting 30 seconds.

Recommended design:

```rust
struct LspClientOptions {
    request_timeout: Duration,
    server_request_timeout: Duration,
}
```

Production defaults remain unchanged. Tests use 50–250 ms.

Do not scatter `#[cfg(test)]` timeout constants throughout request logic.

## C9. Cancellation Write Failure

Configure the fake server to close stdin-reading/process state or exit just before Codegg writes `$/cancelRequest`.

This scenario may be timing-sensitive at the OS pipe level. Prefer one of:

- use the deterministic failing-writer unit test for exact cancellation-write behavior;
- use the child-process test only to assert transport failure after process exit.

Do not reintroduce a flaky pipe-buffer assertion.

## C10. Graceful Shutdown

Script:

```text
client -> shutdown request
server -> shutdown result
client -> exit notification
server process exits 0
```

Assert:

- shutdown request has an ID;
- exit notification has no ID;
- child exits;
- reader terminates cleanly;
- pending requests are empty;
- transport state no longer reports usable/running;
- `shutdown_all()` is idempotent.

## C11. Ungraceful Shutdown and EOF

Variants:

- server closes stdout without responding;
- server exits with code 1;
- server exits while request pending;
- server exits after initialize but before `initialized`;
- server closes stdout but keeps process alive briefly;
- stderr contains a diagnostic message.

Assert:

- reader marks transport failed;
- all pending requests fail promptly rather than timing out;
- later requests fail fast;
- initialization callers receive the actual failure category;
- child cleanup remains bounded.

## C12. Malformed Framing

Test actual byte-level parser behavior:

- missing `Content-Length`;
- nonnumeric content length;
- negative content length string;
- content length larger than body then EOF;
- content length smaller than body;
- duplicated headers;
- LF-only versus CRLF framing according to parser policy;
- extremely large declared frame rejected before allocation;
- valid Unicode JSON with byte length differing from character count;
- malformed JSON with valid framing;
- multiple frames in one write;
- header split across writes;
- body split across writes.

Add or confirm a maximum inbound frame size:

```rust
const MAX_LSP_FRAME_BYTES: usize = ...;
```

Choose a value sufficient for real LSP responses, such as 32–64 MiB, and return a clear protocol error when exceeded.

If no size limit currently exists, this is part of Phase 2 hardening.

## C13. Malformed JSON-RPC Shapes

Send framed JSON values that are syntactically valid but structurally invalid:

- ID only;
- result without ID;
- method of non-string type;
- error with fractional code;
- error without message;
- array batch message if unsupported;
- primitive JSON value;
- null;
- request with object ID;
- response with both result and error.

Assert:

- malformed messages do not resolve pending requests;
- reader continues for recoverable malformed messages;
- terminal policy is explicit for unrecoverable protocol violations;
- logs contain bounded metadata, not full large payloads.

## C14. Server-Request Response Write Failure

Have the fake server issue a request and then terminate/close its input path before Codegg writes the response.

Assert:

- response write failure marks transport failed;
- pending client requests are drained;
- reader exits;
- later operations fail fast.

If OS behavior makes exact EPIPE nondeterministic, retain a deterministic writer unit test and use the process fixture to verify the broader exit/failure path.

## C15. Process Stderr Handling

Have the fake server write:

- ordinary log line;
- long line;
- invalid UTF-8 bytes if the drain supports byte streams;
- many lines before exit.

Assert stderr cannot deadlock the child due to a full pipe. Verify logs are bounded/truncated according to policy.

# Part D — Representative LSP Feature Scenarios

The fake server does not need semantic intelligence. It should return deterministic fixtures proving operation mapping and DTO conversion.

## D1. Document Lifecycle

Exercise:

```text
textDocument/didOpen
textDocument/didChange
textDocument/didSave
textDocument/didClose
```

Assert:

- correct URI;
- language ID;
- monotonically increasing versions;
- full-content change payload;
- save text behavior matches advertised sync policy;
- duplicate close is idempotent at service layer;
- ownership map updates correctly.

## D2. Basic Semantic Operations

Provide deterministic responses for:

```text
textDocument/hover
textDocument/definition
textDocument/references
textDocument/documentSymbol
workspace/symbol
textDocument/implementation
textDocument/typeDefinition
```

Only include methods currently implemented by Codegg. Do not add new production operations solely for fixture completeness.

Assert:

- capability gating;
- location/range decoding;
- URI normalization;
- compact DTO conversion;
- result truncation/budget behavior where applicable.

## D3. Hierarchy Operations

If currently supported:

```text
textDocument/prepareCallHierarchy
callHierarchy/incomingCalls
callHierarchy/outgoingCalls
textDocument/prepareTypeHierarchy
typeHierarchy/supertypes
typeHierarchy/subtypes
```

Return multiple nodes and cycles. Assert bounded traversal and stable ordering.

## D4. Preview-Only Edit Operations

Return deterministic `WorkspaceEdit` values for:

```text
textDocument/rename
textDocument/formatting
textDocument/codeAction
```

Include:

- simple `changes` map;
- `documentChanges` text edits;
- multiple files;
- unsupported resource operations;
- command-only code actions;
- overlapping edits;
- out-of-root URI;
- invalid range.

Assert:

- valid edits become previews;
- command-only/unsafe actions are rejected;
- no file is mutated;
- root authorization remains enforced;
- unsupported workspace operations produce clear errors.

## D5. Semantic Context Composite

Script all underlying requests needed by `semanticContext`:

- symbols;
- hover;
- definition;
- references;
- diagnostics;
- hierarchy where enabled.

Assert:

- one end-to-end composite result is produced;
- evidence provenance is consistent;
- interleaved diagnostics do not corrupt request routing;
- truncation markers and freshness metadata survive serialization.

## D6. Security Context Composite

Script call expansion and diagnostics used by `securityContext`.

Assert:

- bounded expansion;
- deterministic ordering;
- timeout/failure of one optional operation degrades gracefully;
- required evidence failures remain explicit;
- no raw unbounded LSP payload enters model context.

## D7. Hunk Source Context

Use a synthetic diff and deterministic symbol/definition/reference fixtures.

This phase should verify the current first-anchor behavior, not redesign it. Assert:

- hunk parsing and line mapping;
- request selection;
- source excerpt collection;
- stable output ordering;
- bounded request counts.

Hunk clustering remains a later roadmap phase.

# Part E — Harness Reliability and Failure Diagnostics

## E1. Deterministic Timeouts

Every integration test must have a total timeout, for example 5–10 seconds.

Individual protocol waits should use shorter bounds.

Avoid bare `.await` on:

- process exit;
- scenario barriers;
- response arrival;
- transcript completion.

When a timeout occurs, report:

- scenario name;
- current expected step;
- transcript tail;
- child exit status;
- stderr tail;
- pending request IDs if accessible.

## E2. Cleanup Guard

Use an RAII harness guard or `Drop` strategy that kills the fake server if a test panics.

Because async cleanup cannot reliably run in `Drop`, combine:

- child `kill_on_drop(true)`;
- explicit `shutdown().await` in normal test completion;
- tempdir cleanup;
- no orphan processes.

## E3. Parallel Test Safety

Tests must run safely under multiple test threads.

Requirements:

- unique temp directories;
- no process-global environment mutation;
- scenario path passed per child through command-specific env;
- unique transcript paths;
- no fixed ports;
- no shared mutable static scenario state.

## E4. Platform Portability

Support at minimum:

- macOS Apple Silicon;
- Linux x86_64/aarch64 CI.

Avoid shell scripts as the server executable. Use a Rust binary.

Normalize only platform-dependent paths in assertions. Do not assert raw `/tmp` strings where URI encoding may differ.

Windows compatibility is desirable, but if current Codegg CI does not support Windows, document untested assumptions around process termination and file URIs.

## E5. Scenario Strictness

Strict mode should fail on unexpected client messages.

Allow explicit exceptions for nondeterministic benign notifications, such as log or cancellation messages, through scenario allow-lists.

A mismatch should cause the fake server to:

1. write a structured mismatch event to transcript;
2. write a concise message to stderr;
3. exit nonzero or send a test-specific protocol error;
4. avoid hanging.

## E6. No Production Backdoors

Do not add production environment variables that alter protocol semantics unless they are generally useful configuration.

Test-only launch injection should be:

- behind `#[cfg(test)]` where possible;
- exposed through owned server definitions/configuration;
- unavailable to model-facing callers.

# Part F — Integration-Test Organization

Recommended files:

```text
crates/egglsp/tests/protocol_stdio.rs
    initialization
    server requests
    interleaving
    cancellation
    malformed framing
    EOF/crash
    shutdown

crates/egglsp/tests/document_lifecycle.rs
    didOpen/didChange/didSave/didClose
    diagnostics

crates/egglsp/tests/semantic_operations.rs
    hover/definition/references/symbols
    hierarchy
    preview edits

crates/egglsp/tests/composite_context.rs
    semanticContext
    securityContext
    hunkSourceContext

crates/egglsp/tests/common/harness.rs
crates/egglsp/tests/common/scenario.rs
crates/egglsp/tests/common/transcript.rs
```

If compilation overhead becomes excessive, consolidate into fewer integration-test crates because each file under `tests/` is a separate binary. A reasonable initial choice is:

```text
protocol_stdio.rs
semantic_stdio.rs
```

with internal modules.

# Part G — CI Strategy

## G1. Default CI

The fake-server integration suite should run in normal workspace CI because it is:

- local;
- deterministic;
- network-free;
- self-contained;
- fast enough for routine changes.

Target total runtime:

```text
< 15 seconds for egglsp integration tests on a normal CI runner
```

## G2. Test Profiles

Use normal tests for deterministic scenarios.

Mark longer stress/repetition tests with an opt-in feature or ignored category only if necessary:

```text
cargo test -p egglsp --features lsp-test-stress
```

Default coverage must still include at least one instance of every critical protocol failure mode.

## G3. Real-Server Smoke Tests Deferred

Do not require rust-analyzer, pyright, gopls, clangd, or TypeScript server in default CI during Phase 2.

At the end of Phase 2, document the next optional matrix:

```text
rust-analyzer
pyright
typescript-language-server
gopls
clangd
```

That is Phase 3/compatibility work.

# Part H — Observability and Error Contracts

## H1. Protocol Errors

Ensure integration tests can distinguish:

```text
framing error
JSON parse error
JSON-RPC structural error
server request invalid params
server request method not found
transport EOF
child exit
request timeout
writer closed
initialization cancelled
```

Avoid collapsing all failures into `RequestFailed(String)` if a narrow existing variant is available.

Do not undertake a broad public error-enum redesign unless tests reveal ambiguity that materially harms callers.

## H2. Pending Request Introspection

A test-only accessor may expose:

```rust
#[cfg(test)]
async fn pending_request_ids(&self) -> Vec<JsonRpcId>
```

or pending count.

Do not expose pending internals through the public model-facing API.

## H3. Transcript Redaction

Synthetic fixtures need no redaction. Still, structure the transcript system so future real-server tests can bound payload size and avoid dumping full source contents by default.

# Part I — Suggested Implementation Order

## Pass 1: Phase 1 cleanup

1. Correct stale task-ownership comments.
2. Rename cooperative cancellation tests honestly.
3. Add a test-only true forced-abort wrapper hook.
4. Add multi-key aggregate grace test.
5. Replace the flaky cancellation-write pipe test with deterministic writer injection.
6. Run all Phase 1 tests under single- and multi-thread scheduling.

## Pass 2: Fixture binary and scenario core

1. Add fake-server binary/crate.
2. Implement Content-Length frame reader/writer.
3. Implement typed scenario parsing.
4. Implement strict step matching.
5. Implement transcript output.
6. Add binary self-tests independent of Codegg client.

## Pass 3: Harness controller

1. Add temp project/scenario setup.
2. Add executable discovery.
3. Route server configuration through the normal launch path.
4. Add cleanup guard and transcript-on-failure diagnostics.
5. Add test-only configurable request/server-request timeouts.

## Pass 4: Core protocol integration tests

1. Initialization handshake.
2. Server requests during initialization.
3. Apply-edit refusal.
4. Interleaved diagnostics/responses.
5. Concurrent out-of-order responses.
6. Timeout and cancellation.
7. Graceful shutdown.
8. EOF/crash.
9. Malformed framing and JSON-RPC shapes.

## Pass 5: Representative feature tests

1. Document lifecycle.
2. Basic semantic operations.
3. Hierarchy operations.
4. Preview-only edits.
5. Semantic context composite.
6. Security context composite.
7. Hunk source context.

## Pass 6: CI and documentation

1. Keep test runtime bounded.
2. Add CI invocation if workspace CI does not automatically include integration tests.
3. Update architecture and user docs.
4. Record unsupported cases and next real-server compatibility phase.

# Part J — File-Level Guidance

## `crates/egglsp/src/service.rs`

Expected cleanup:

- correct `active_init_tasks` comments;
- add test-only forced-abort behavior hook;
- improve aggregate grace tests;
- avoid production logic changes unless fixture tests reveal defects.

## `crates/egglsp/src/client.rs`

Possible Phase 2 changes:

- injectable timeout options;
- test-only pending-count accessor;
- inbound frame-size limit if framing reader lives here;
- clearer transport/EOF errors if needed.

## `crates/egglsp/src/launch.rs`

Possible Phase 2 changes:

- allow owned/test server executable configuration through existing launch abstractions;
- capture bounded stderr tail for integration diagnostics;
- preserve child exit status;
- no test-specific branch in production launch behavior unless `#[cfg(test)]`.

## `crates/egglsp/src/writer.rs`

Expected:

- no major redesign;
- reuse canonical framing in fixture where practical without creating circular dependencies;
- retain Unicode byte-length tests.

## Fake server binary/crate

Expected:

- own frame parser/writer;
- typed scenarios;
- transcript writer;
- strict mismatch behavior;
- deterministic exit codes.

Do not import the production client parser in a way that causes both sides to share the same bug invisibly. Shared low-level framing helpers are acceptable only if independently tested; independent fixture framing provides stronger cross-validation.

# Part K — Verification Commands

Run Phase 1 cleanup tests:

```bash
cargo fmt --check
cargo check -p egglsp --all-targets
cargo test -p egglsp --lib
cargo test -p egglsp service::tests -- --test-threads=1
cargo test -p egglsp service::tests -- --test-threads=8
cargo clippy -p egglsp --all-targets -- -D warnings
```

Run Phase 2 integration tests:

```bash
cargo test -p egglsp --test protocol_stdio
cargo test -p egglsp --test semantic_stdio
cargo test -p egglsp --tests -- --test-threads=1
cargo test -p egglsp --tests -- --test-threads=8
```

Then workspace verification:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Where workspace-wide failures are pre-existing, record exact diagnostics. All modified LSP crates and test targets must be clean.

# Part L — Review Checklist

## Phase 1 cleanup

- [ ] `active_init_tasks` documentation names abort handle + completion receiver, not real `JoinHandle` ownership.
- [ ] Cooperative cancellation tests are named accurately.
- [ ] A deterministic test reaches grace expiry and real `AbortHandle::abort()`.
- [ ] Forced-abort test needs no post-shutdown release.
- [ ] Aggregate grace is tested with multiple independent task controls.
- [ ] Deadline-fallback claims match actual coverage.
- [ ] Flaky cancellation-write test is deterministic or replaced.

## Fixture architecture

- [ ] Fake server is a Rust child-process executable.
- [ ] It uses real stdin/stdout Content-Length framing.
- [ ] Scenario input is per-process and parallel-safe.
- [ ] Unexpected messages fail fast with transcript output.
- [ ] No network or external server dependency.
- [ ] Child cleanup is guaranteed on test failure.

## Protocol coverage

- [ ] Initialize/initialized handshake.
- [ ] Server requests during initialization.
- [ ] Configuration/workspace folders/dynamic registration.
- [ ] Apply-edit refusal result.
- [ ] Notifications interleaved with responses.
- [ ] Out-of-order concurrent responses.
- [ ] Diagnostics lifecycle.
- [ ] Timeout and `$/cancelRequest`.
- [ ] Graceful shutdown/exit.
- [ ] EOF and nonzero process exit.
- [ ] Malformed framing.
- [ ] Malformed JSON-RPC structures.
- [ ] Server-response write failure path or deterministic equivalent.
- [ ] Stderr drainage.

## Feature coverage

- [ ] Document lifecycle.
- [ ] Hover/definition/references/symbols.
- [ ] Hierarchy operations currently supported.
- [ ] Rename/format/code-action previews.
- [ ] No edit mutation.
- [ ] Semantic context composite.
- [ ] Security context composite.
- [ ] Hunk source context current behavior.

## Reliability

- [ ] Every test has a total timeout.
- [ ] Failure output includes transcript and stderr tail.
- [ ] Tests run in parallel without global state collisions.
- [ ] Test suite runtime is acceptable for default CI.
- [ ] macOS and Linux path/URI differences are normalized.

# Completion Criteria

This combined phase is complete when:

1. Phase 1 shutdown/task tests and comments accurately reflect implementation semantics.
2. The real forced-abort path is deterministically tested.
3. Multiple independent initialization tasks share one aggregate grace deadline.
4. The cancellation-write transport test is no longer OS-pipe flaky.
5. A scripted fake LSP server launches through Codegg’s real child-process path.
6. Real framed stdio traffic is tested end to end.
7. Initialization, server requests, interleaving, diagnostics, cancellation, malformed input, EOF/crash, and shutdown scenarios pass deterministically.
8. Representative semantic and preview operations are exercised through the child process.
9. Composite semantic/security/hunk context paths have at least one end-to-end fixture scenario each.
10. No default test requires network access or an installed language server.
11. Failures provide actionable transcripts and bounded stderr output.
12. The default egglsp test suite is stable under single- and multi-thread execution.
13. Documentation identifies Phase 2 as complete and describes the next opt-in real-server compatibility matrix.

## Handoff Result

At the end of this work, Codegg’s LSP integration will no longer rely primarily on internal unit seams for protocol confidence. It will have deterministic end-to-end evidence that the production process launcher, frame parser, background reader, server-request dispatcher, pending map, diagnostics cache, cancellation logic, semantic adapters, and shutdown coordinator operate correctly against a real child process under both normal and adversarial protocol sequences.
