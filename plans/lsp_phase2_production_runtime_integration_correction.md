# LSP Phase 2 Production-Runtime Integration Correction

## Purpose

Correct the Phase 2 test architecture after:

```text
64a0b7bd32d7412eb86494d113556f50beffeeff
13ac82b0227275b76c8ec82daf0d8b4e3ee8305e
a9d916afd637ce4c835cb292c16883fa50cebba8
9ef11b2b4f98175b5198df747e98ac4f2f67205a
fabce0dc13a5634fc9501e418c0c0e1c5fc8f4c7
4f7c4519c061c0902551557de650c1a3d36a8058
b2d86db00734dd460bf209c689a5110cc3ab7156
```

The existing work established a strong deterministic fake-server fixture, a broad scenario corpus, transcript output, configurable request timeouts, malformed-input fixtures, framing limits, and substantial Phase 1 lifecycle coverage.

The central Phase 2 defect is architectural: most current integration tests launch the fake server directly and act as a hand-written LSP client through test-local wire helpers. They therefore validate the fake server and fixture payloads, but bypass Codegg’s production:

- `LspClient` constructor;
- `LspService` initialization and routing;
- `launch.rs` process path;
- `LspWriter`;
- background stdout reader;
- JSON-RPC classifier;
- pending request map;
- server-request dispatcher;
- diagnostics cache;
- dynamic registration state;
- semantic DTO conversion;
- workspace-edit preview conversion;
- shutdown coordinator.

This pass should preserve the existing scenario corpus but rewire it through the production runtime. The corrected Phase 2 contract is:

> The fake server runs as a real child process, while Codegg’s production `LspClient` and `LspService` generate, receive, classify, dispatch, cache, convert, cancel, and shut down all protocol traffic under test.

## Outcome

At completion:

1. The primary integration suites instantiate production `LspClient` or `LspService`.
2. Test-local wire helpers are restricted to fixture self-tests and exceptional byte-level server setup.
3. Server-originated malformed frames are emitted toward Codegg’s production reader.
4. Strict scenarios validate IDs and selected request/notification parameters.
5. Server-request tests verify Codegg’s production dispatcher and state mutations.
6. Semantic tests call typed Codegg APIs and assert typed outputs.
7. Composite tests invoke actual semantic/security/hunk collectors rather than replaying similar raw calls manually.
8. The fake-server binary is built and discovered hermetically by Cargo.
9. Production framing gains bounded header size and duplicate-header policy.
10. Documentation stops claiming coverage that does not pass through production code.

## Scope

Primary production files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/server.rs
crates/egglsp/src/server_request.rs
crates/egglsp/src/error.rs
crates/egglsp/src/lib.rs
```

Fake-server fixture:

```text
crates/egglsp-test-server/Cargo.toml
crates/egglsp-test-server/src/main.rs
```

Integration tests:

```text
crates/egglsp/tests/common/harness.rs
crates/egglsp/tests/common/scenario.rs
crates/egglsp/tests/common/transcript.rs
crates/egglsp/tests/common/wire.rs
crates/egglsp/tests/protocol_stdio.rs
crates/egglsp/tests/semantic_stdio.rs
```

Possible package-layout changes:

```text
crates/egglsp/src/bin/egglsp-test-server.rs
```

or a small dedicated test package wired through Cargo artifact discovery.

Documentation:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
plans/lsp_phase1_cleanup_and_phase2_scripted_stdio_harness.md
```

## Non-Goals

Do not implement in this pass:

- automatic restart supervision;
- production real-server matrix execution;
- pull diagnostics;
- incremental sync;
- multi-root workspaces;
- new model-facing tools;
- new semantic operations solely to increase test counts;
- general fuzzing infrastructure;
- direct application of `workspace/applyEdit`;
- broad public API redesign unrelated to harnessability.

## Preserve Existing Work

Retain and reuse:

- the fake-server scenario corpus;
- transcript JSONL output;
- `LspClientOptions` configurable timeouts;
- the 64 MiB body limit;
- genuine forced-abort Phase 1 tests;
- aggregate initialization grace coverage;
- deterministic writer-failure unit seams;
- read/write lock discipline;
- authoritative initialization task completion;
- lifecycle-safe publication and shutdown;
- existing semantic DTO implementations;
- preview-only edit safety boundaries.

# Part A — Establish a Production-Client Test Harness

## A1. Add a Testable Owned Server Definition

The production runtime currently expects an `LspServerDef` with static fields. The integration harness needs to supply:

```text
executable path
arguments
environment variables
server ID
language/extensions
optional initialization configuration
client timeout options
```

Introduce a narrowly scoped owned launch specification, for example:

```rust
#[derive(Debug, Clone)]
pub struct LspLaunchSpec {
    pub id: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub languages: Vec<String>,
    pub extensions: Vec<String>,
}
```

Possible integration approaches:

1. Add `LspClient::new_with_launch_spec(...)` and retain `new(...)` as the registry-backed adapter.
2. Generalize `LspServerDef` to support owned values without changing registry semantics.
3. Introduce an internal `ResolvedLspServer` used by `launch.rs` and construct it from both static definitions and tests.

Preferred design:

```text
static registry definition
        -> resolve/download
        -> ResolvedLspLaunch

test harness owned definition
        -> ResolvedLspLaunch

both
        -> LspClient::spawn_resolved(...)
```

Avoid leaking test scenario paths into global process environment.

## A2. Per-Child Environment

The fake server receives:

```text
CODEGG_FAKE_LSP_SCENARIO
CODEGG_FAKE_LSP_TRANSCRIPT
```

Pass these through the production child launch environment for that client only.

Do not call `std::env::set_var()` in parallel integration tests.

## A3. Production Client Harness Type

Create:

```rust
pub struct ProductionLspHarness {
    tempdir: TempDir,
    root: PathBuf,
    source_path: PathBuf,
    scenario_path: PathBuf,
    transcript_path: PathBuf,
    client: Option<Arc<LspClient>>,
    service: Option<LspService>,
}
```

Suggested constructors:

```rust
async fn start_client(
    scenario: Scenario,
    options: LspClientOptions,
    configuration: Value,
) -> Result<Self, LspError>;

async fn start_service(
    scenario: Scenario,
    options: LspClientOptions,
    configuration: LspConfig,
) -> Result<Self, LspError>;
```

Responsibilities:

- create synthetic project root and root marker;
- write source fixtures;
- write scenario file;
- resolve hermetic fake-server path;
- launch through production code;
- perform production initialize/initialized handshake;
- retain transcript/stderr diagnostics;
- perform bounded production shutdown;
- kill child on panic/drop through existing kill-on-drop semantics.

## A4. No Hand-Written Client in Primary Suites

The following helpers must not be used to drive primary Phase 2 behavior:

```text
send_initialize
send_request
send_notification
send_response
send_error_response
manual child.stdin/stdout ownership
```

Retain them only for:

- fake-server binary self-tests;
- testing the fixture parser independently;
- unusual setup where the fake server itself is the subject under test.

Rename test-local modules to make the boundary obvious:

```text
common/fixture_wire.rs
common/production_harness.rs
```

## A5. Acceptance Criteria

- A production `LspClient` can launch the fake server with scenario-specific env.
- A production `LspService` can discover/own the client for a synthetic file.
- No primary protocol or semantic integration test manually implements LSP client behavior.

# Part B — Make Fake-Server Scenarios Strict and Expressive

## B1. Add Parameter Matchers

Extend scenario steps:

```rust
ExpectRequest {
    method: String,
    #[serde(default)]
    id: IdMatcher,
    #[serde(default)]
    params: ValueMatcher,
    #[serde(default)]
    then: Vec<Action>,
}

ExpectNotification {
    method: String,
    #[serde(default)]
    params: ValueMatcher,
    #[serde(default)]
    then: Vec<Action>,
}

ExpectResponse {
    id: IdMatcher,
    #[serde(default)]
    result: Option<ValueMatcher>,
    #[serde(default)]
    error: Option<ErrorMatcher>,
    #[serde(default)]
    then: Vec<Action>,
}
```

Recommended matchers:

```rust
#[derive(Default, Deserialize)]
enum IdMatcher {
    #[default]
    Any,
    Exact(Value),
    Number,
    String,
}

#[derive(Default, Deserialize)]
enum ValueMatcher {
    #[default]
    Any,
    Exact(Value),
    Null,
    ObjectContains(BTreeMap<String, ValueMatcher>),
    ArrayLen(usize),
    String(String),
    Number(i64),
    Bool(bool),
}
```

Support nested `ObjectContains` so scenarios can assert only load-bearing fields without duplicating entire payloads.

## B2. Strict Mode Must Fail Immediately

Current behavior records unexpected messages and keeps reading. In strict mode:

- wrong method;
- wrong message category;
- parameter mismatch;
- ID mismatch;
- unexpected extra message;

must produce:

```text
transcript StepMismatch
concise stderr diagnostic
nonzero exit
no indefinite wait
```

Non-strict mode may skip allow-listed messages only.

Add explicit allowances:

```rust
AllowNotification { method: String }
AllowRequest { method: String }
```

Do not make `strict: false` the default escape hatch for most production tests.

## B3. Transcript Improvements

Record:

```rust
sequence
step_index
expected_summary
actual_category
actual_method
actual_id
match_result
mismatch_reason
```

The transcript should make failures diagnosable without rerunning under tracing.

## B4. Test Scenario Self-Validation

Before spawning the server, validate scenario consistency:

- duplicate exact response IDs where impossible;
- empty method names;
- malformed matcher definitions;
- action requiring a request ID attached to a notification step;
- unsupported raw output combinations.

## B5. Acceptance Criteria

- Document lifecycle scenarios assert URI, language ID, versions, and content changes generated by production Codegg.
- Initialization scenarios assert root URI, process ID shape, capabilities, and initialization options.
- Unexpected messages fail strict scenarios immediately.

# Part C — Add Server-to-Client Raw Output Actions

## C1. Required Actions

Extend the fake server with actions that write raw bytes to stdout:

```rust
SendRawBytes {
    bytes_base64: String,
}

SendRawFrame {
    body: String,
}

SendJsonWithDeclaredLength {
    value: Value,
    declared_length: usize,
}

SendHeaderOnly {
    header: String,
}

SendBodyChunks {
    header: String,
    chunks: Vec<String>,
    delay_millis: u64,
}

CloseStdout

Exit {
    code: i32,
}
```

Also support multiple frames in one write:

```rust
SendFramesTogether { messages: Vec<Value> }
```

## C2. Malformed Input Direction

Rewrite C12/C13 primary integration tests so malformed bytes travel:

```text
fake server stdout -> Codegg LspClient background reader
```

Not:

```text
test client stdin -> fake server parser
```

Fixture-parser self-tests may retain the old direction but should be named accordingly and moved out of the production integration matrix.

## C3. Required Production-Reader Cases

Test Codegg receiving:

- missing `Content-Length`;
- nonnumeric length;
- negative textual length;
- oversized body declaration;
- unbounded/oversized header;
- body shorter than declaration then EOF;
- body longer than declaration;
- duplicate `Content-Length`;
- LF-only separator according to explicit policy;
- multiple frames in one write;
- header split across writes;
- body split across writes;
- invalid UTF-8 body;
- malformed JSON;
- ID-only JSON-RPC object;
- result without ID;
- non-string method;
- fractional error code;
- response containing both result and error;
- batch array;
- primitive/null JSON.

## C4. Recovery Policy

Define and test which failures are recoverable:

```text
structurally invalid JSON-RPC frame
    -> log/ignore and continue

invalid JSON syntax
invalid framing
oversized frame
EOF mid-frame
    -> transport failure, fail pending, terminate reader
```

Document this policy.

# Part D — Production Framing Hardening

## D1. Add Header Size Limit

The production reader currently accumulates bytes until `\r\n\r\n` with no bound.

Add:

```rust
const MAX_LSP_HEADER_BYTES: usize = 16 * 1024;
```

A value between 8 and 64 KiB is acceptable. LSP headers are normally tiny.

Before pushing a new byte or after each push:

```rust
if header_buf.len() > MAX_LSP_HEADER_BYTES {
    return Err(LspError::Protocol(...));
}
```

## D2. Duplicate Content-Length Policy

Replace `parse_content_length() -> Option<usize>` with a stricter parser:

```rust
fn parse_lsp_headers(header: &[u8]) -> Result<LspHeaders, LspError>
```

Required behavior:

- case-insensitive header names if desired for interoperability;
- exactly one `Content-Length`;
- reject duplicate lengths, even if equal;
- reject malformed numeric value;
- reject signed/fractional value;
- ignore recognized optional headers such as `Content-Type`;
- reject or bound individual header-line length.

## D3. Error Variants

Prefer `LspError::Protocol` for framing/parser violations rather than `RequestFailed`.

Do not dump full malformed bodies into error messages. Include bounded metadata:

```text
header bytes read
claimed length
maximum
JSON parse line/column
```

## D4. Test Parser Independently

Extract framing reader over a generic `AsyncRead + Unpin` if practical:

```rust
async fn read_framed_message<R: AsyncRead + Unpin>(reader: &mut R)
```

This permits deterministic duplex/unit tests without child processes.

The child-process tests remain necessary to prove launch and reader integration.

# Part E — Replace Manual Protocol Tests with Production Tests

## E1. Initialization Handshake

Launch via `LspClient::new_with_launch_spec` or equivalent.

The fake server scenario expects:

```text
initialize request with matched params
initialized notification
shutdown request
exit notification
```

Assertions from production side:

- client constructed successfully;
- capabilities stored;
- configuration passed;
- no manual response handling;
- production `shutdown()` succeeds.

## E2. Server Requests During Initialization

The fake server sends before initialize completes:

```text
workspace/configuration
workspace/workspaceFolders
window/workDoneProgress/create
client/registerCapability
```

Production Codegg must respond.

Assertions:

- scenario `ExpectResponse` matches returned payload;
- dynamic registration state contains all registrations;
- workspace folder result matches root;
- progress creation returns null;
- initialization completes afterward.

## E3. Apply-Edit Refusal

The fake server sends `workspace/applyEdit` after initialization and expects:

```json
{
  "result": {
    "applied": false,
    "failureReason": "..."
  }
}
```

Assertions:

- response is a success result, not error;
- synthetic file content remains unchanged;
- client remains usable with a subsequent hover/request;
- scenario verifies response shape.

Delete the manual error-response behavior from the old test.

## E4. Dynamic Registration

Send multiple registrations in one server request.

After Codegg responds, inspect production registration state through a test-only read accessor or public capability snapshot where appropriate.

Then send:

```text
client/unregisterCapability with unregisterations
```

and verify state removal.

Include atomic over-cap failure as unit coverage; a smaller integration case is sufficient.

## E5. Timeout and Cancellation

Use production `LspClientOptions` with a 50–250 ms request timeout.

Sequence:

1. Call a real production client request method.
2. Fake server receives request and intentionally does not respond.
3. Codegg timeout removes pending entry.
4. Codegg sends `$/cancelRequest`.
5. Fake server scenario matches the cancel notification and ID.
6. Fake server sends a late response.
7. Codegg ignores it.
8. A subsequent request succeeds.

Add test-only pending count accessor if needed:

```rust
#[cfg(any(test, feature = "lsp-test-support"))]
pub async fn pending_request_count(&self) -> usize
```

Prefer a private integration-support feature over public production API pollution.

## E6. Interleaving and Out-of-Order Responses

Issue real concurrent production calls:

```rust
tokio::join!(
    client.hover(...),
    client.definition(...),
    client.references(...),
    client.document_symbols(...),
)
```

The fake server returns responses in reverse order with notifications interleaved.

Assertions:

- typed outputs map correctly;
- diagnostics cache updates;
- pending count returns to zero;
- no caller receives another request’s result.

## E7. EOF and Exit

Use production requests while the fake server:

- exits before response;
- exits nonzero;
- closes stdout;
- closes stdout mid-frame.

Assertions:

- current pending requests fail promptly;
- later requests fail fast;
- transport state is failed;
- no 30-second timeout wait;
- service shutdown remains bounded.

# Part F — Service-Level Integration

## F1. Production `LspService` Harness

A subset of tests must go through `LspService`, not only direct `LspClient`.

Required scenarios:

- cold initialization from a synthetic file;
- same-key concurrent first use launches one child;
- document ownership routing;
- `open_file`/`update_file`/`save_file`/`close_file`;
- diagnostics retrieval by client key;
- service shutdown while a request is in flight;
- service shutdown while initialization is delayed.

## F2. Test Server Selection

Avoid modifying the global production registry.

Options:

1. Add a test-only server resolver to `LspService`.
2. Add an injectable `LspClientFactory` that uses the real `LspClient` and fake launch spec.
3. Add an owned rules/config path where test rules carry a command override.

Preferred:

```rust
trait LspLaunchResolver: Send + Sync {
    async fn resolve(
        &self,
        server: &'static LspServerDef,
        root: &Path,
    ) -> Result<ResolvedLspLaunch, LspError>;
}
```

Production resolver downloads/resolves normal binaries. Test resolver returns the fake server launch with scenario env.

Do not revert to the in-memory fake client factory for these Phase 2 tests.

## F3. Child Count Observation

The fake server transcript or an optional startup marker should permit asserting exactly one child was launched for same-key concurrent service calls.

Use per-scenario startup records rather than process-table inspection.

# Part G — Typed Semantic Operation Tests

## G1. Basic Operations

Replace manual raw calls with production methods:

```rust
client.hover(...)
client.definition(...)
client.references(...)
client.document_symbols(...)
client.workspace_symbols(...)
client.implementation(...)
client.type_definition(...)
```

Assert typed return values and normalized paths/ranges.

Scenarios should validate request parameters:

- document URI;
- line/character;
- reference `includeDeclaration`;
- workspace symbol query.

## G2. Hierarchy

Call production hierarchy APIs:

```text
prepare call hierarchy
incoming calls
outgoing calls
prepare type hierarchy
supertypes
subtypes
```

Assert:

- decoded item metadata;
- stable range/selection range;
- bounded traversal at collector layer;
- cycles do not cause unbounded recursion.

Only test currently supported operations.

## G3. Workspace Edit Preview

Call actual production rename/format/code-action methods and preview conversion.

Required fixtures:

- `changes` map;
- `documentChanges` text edits;
- multi-file edits;
- overlapping edits;
- out-of-root URI;
- command-only action;
- resource operation;
- malformed range.

Assertions:

- valid edit previews are typed correctly;
- no files are modified;
- unsupported/unsafe operations are rejected clearly;
- root authorization is enforced;
- truncation/budget limits are preserved.

## G4. Diagnostics

Have the fake server publish diagnostics while production client is running.

Assert through production cache APIs:

- diagnostics stored by URI;
- empty publication clears prior diagnostics;
- version/source metadata retained;
- invalidation/freshness changes after document update;
- two URIs remain separate;
- malformed notification does not terminate transport.

# Part H — Actual Composite API Tests

## H1. Semantic Context

Invoke the real `SemanticContextCollector` or service API used by the model-facing handler.

The fake server supplies constituent responses.

Assert the resulting typed `SemanticContextResponse` or packet contains:

- source excerpt;
- diagnostics evidence and freshness metadata;
- symbols;
- hover;
- definitions;
- references;
- hierarchy summaries when requested;
- errors/notes for unsupported optional operations;
- budget/truncation markers.

Do not manually issue the constituent JSON-RPC calls in the test.

## H2. Security Context

Invoke the actual security-context collection path.

Assert:

- shared diagnostic evidence is reused;
- call expansion obeys depth and node bounds;
- unsupported operations degrade according to policy;
- deterministic ordering;
- no unbounded raw payload reaches the packet.

## H3. Hunk Source Context

Invoke the actual `HunkSourceNavigator`/collector or model-facing service path with a synthetic diff.

Assert:

- hunk anchor mapping;
- symbol/definition/reference collection;
- stable source excerpts;
- bounded request counts;
- current first-anchor behavior is preserved.

Hunk clustering remains out of scope.

# Part I — Hermetic Fake-Server Binary Discovery

## I1. Preferred Cargo Layout

The current harness searches `target/debug` and `target/release`, which can fail on clean checkout or select a stale binary.

Preferred solution: make the fake server a binary target available to the integration-test package so Cargo provides:

```text
CARGO_BIN_EXE_egglsp-test-server
```

Options:

1. Move the binary to `crates/egglsp/src/bin/egglsp-test-server.rs` behind a test-support feature.
2. Configure an explicit `[[bin]]` target in `crates/egglsp/Cargo.toml`.
3. Retain the dedicated package but add a CI/build wrapper that obtains the Cargo JSON artifact path and exports it.

Prefer options 1 or 2 for simplicity and hermeticity.

## I2. No Target Directory Search

Remove fallback scanning of:

```text
target/debug
target/release
```

An explicit override env var may remain for debugging:

```text
EGGLSP_TEST_SERVER
```

but normal tests should use Cargo’s path.

## I3. Clean-Checkout Test

Add/document verification:

```bash
cargo clean
cargo test -p egglsp --test protocol_stdio production_initialization_handshake
```

This must build and locate the fixture automatically.

# Part J — Test Organization

## J1. Separate Fixture Self-Tests from Production Integration

Recommended layout:

```text
crates/egglsp-test-server/tests/scenario_engine.rs
    fake-server parser and matcher self-tests

crates/egglsp/tests/production_protocol_stdio.rs
    real LspClient/LspService protocol tests

crates/egglsp/tests/production_semantic_stdio.rs
    typed semantic/edit/context tests
```

The old raw-wire tests may be:

- migrated into fake-server self-tests;
- retained under names such as `fixture_accepts_multiple_frames`;
- deleted if redundant.

Do not count fixture self-tests as production LSP integration coverage.

## J2. Naming

Use names that state the subject:

```text
production_client_apply_edit_refusal
production_client_timeout_sends_cancel
production_reader_rejects_oversized_frame
fixture_parser_rejects_missing_content_length
```

## J3. Test Totals

Stop using total test count as a completion metric.

Document coverage by invariant and production path.

# Part K — Failure Diagnostics

## K1. Harness Error Wrapper

Provide a helper that augments failures with:

- scenario name;
- transcript tail;
- stderr tail;
- child exit status;
- production client transport state;
- pending count;
- lifecycle state for service tests.

## K2. Automatic Diagnostics

Where practical, wrap test bodies:

```rust
let result = run_test(&mut harness).await;
if let Err(err) = result {
    panic!("{}\n{}", err, harness.diagnostics().await);
}
```

Assertions that panic directly will bypass async diagnostics. Prefer result-returning helpers for complex scenarios.

## K3. Bound Output

Limit transcript/stderr tails to a reasonable number of events/bytes.

# Part L — Documentation Corrections

Until the production-runtime migration is complete, update documentation to say:

```text
Phase 2 fixture infrastructure complete
Production-runtime end-to-end migration in progress
```

Remove claims that current raw-wire tests exercise:

- `LspService`;
- production `LspClient` background reader;
- production server-request dispatcher;
- production diagnostics cache;
- semantic DTO conversion;
- composite collectors.

After completion, document separate counts/categories:

```text
fixture self-tests
production-client integration tests
production-service integration tests
semantic/composite production integration tests
```

# Suggested Implementation Order

## Pass 1 — Harnessability and hermetic binary

1. Introduce `ResolvedLspLaunch`/owned launch specification.
2. Add production `LspClient` constructor using it.
3. Pass per-child scenario/transcript env through production launch.
4. Make Cargo expose the fake-server binary hermetically.
5. Build `ProductionLspHarness`.
6. Add one production initialize/shutdown smoke test.

## Pass 2 — Strict scenario engine

1. Add ID and parameter matchers.
2. Make strict mode fail immediately.
3. Add mismatch transcript records.
4. Add server raw-output actions.
5. Add fake-server self-tests.

## Pass 3 — Core production protocol migration

1. Initialization handshake.
2. Server requests during initialization.
3. Apply-edit refusal.
4. Dynamic registration/unregistration.
5. Interleaving/out-of-order responses.
6. Timeout/cancel/late response.
7. EOF/nonzero exit.
8. Graceful shutdown.

## Pass 4 — Framing hardening

1. Add header size cap.
2. Add strict header parser.
3. Rewrite malformed tests server-to-client.
4. Define recoverable versus terminal reader policy.
5. Add child-process and duplex unit coverage.

## Pass 5 — Service-level tests

1. Production service cold start.
2. Same-key single-flight child launch.
3. Document lifecycle and ownership.
4. Diagnostics access.
5. Initialization/request shutdown races.

## Pass 6 — Typed semantic migration

1. Basic operations.
2. Hierarchy.
3. Edit preview conversion.
4. Diagnostics freshness.
5. Semantic context.
6. Security context.
7. Hunk source context.

## Pass 7 — Remove misleading raw-wire coverage claims

1. Move fixture tests to fixture package.
2. Rename remaining tests.
3. Update docs and coverage matrix.
4. Run clean-checkout CI verification.

# File-Level Guidance

## `crates/egglsp/src/launch.rs`

Expected changes:

- accept resolved owned command/args/env;
- preserve production static-definition adapter;
- retain stderr drainage and kill-on-drop;
- expose exit metadata needed by tests without leaking child internals broadly.

## `crates/egglsp/src/client.rs`

Expected changes:

- add owned/resolved launch constructor;
- retain configurable options;
- generic/bounded frame reader;
- add header cap and strict header parser;
- possibly add test-support pending/transport accessors;
- no test-only protocol behavior.

## `crates/egglsp/src/service.rs`

Expected changes:

- injectable launch resolver that still creates a real `LspClient`;
- test-support lifecycle/client-key accessors if needed;
- no regression to single-flight/shutdown architecture.

## `crates/egglsp-test-server/src/main.rs`

Expected changes:

- parameter/ID matchers;
- strict mismatch behavior;
- raw stdout actions;
- scenario validation;
- improved transcript events;
- independent frame-size guard for fixture safety.

## Integration tests

Expected changes:

- production harness owns `LspClient`/`LspService`;
- no manual client-side framing in production suites;
- typed API assertions;
- raw-wire helpers moved to fixture self-tests.

# Verification

Run targeted production integration:

```bash
cargo test -p egglsp --test production_protocol_stdio -- --test-threads=1
cargo test -p egglsp --test production_protocol_stdio -- --test-threads=8
cargo test -p egglsp --test production_semantic_stdio -- --test-threads=1
cargo test -p egglsp --test production_semantic_stdio -- --test-threads=8
```

Run fixture self-tests:

```bash
cargo test -p egglsp-test-server
```

Run clean-checkout binary verification:

```bash
cargo clean
cargo test -p egglsp --test production_protocol_stdio production_initialization_handshake
```

Run crate verification:

```bash
cargo fmt --check
cargo check -p egglsp --all-targets
cargo test -p egglsp
cargo clippy -p egglsp --all-targets -- -D warnings
cargo clippy -p egglsp-test-server --all-targets -- -D warnings
```

Then workspace verification:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

# Review Checklist

## Production harness

- [ ] Primary tests construct real `LspClient` or `LspService`.
- [ ] Production launch path starts the child.
- [ ] Scenario env is per-child, not process-global.
- [ ] Fake-server path is supplied hermetically by Cargo.
- [ ] No target-directory scanning is required.

## Scenario engine

- [ ] Strict steps validate category, method, ID, and selected params.
- [ ] Unexpected messages fail immediately in strict mode.
- [ ] Mismatches are recorded in transcript.
- [ ] Raw stdout/framing actions exist.

## Protocol runtime

- [ ] Production initialization handshake.
- [ ] Production server-request dispatch.
- [ ] Apply-edit returns `applied:false` result.
- [ ] Registration state changes through production dispatcher.
- [ ] Production timeout sends cancel with correct ID.
- [ ] Late response is ignored.
- [ ] Pending map drains.
- [ ] EOF/crash marks transport failed.
- [ ] Graceful shutdown uses production methods.

## Framing

- [ ] Body size cap remains.
- [ ] Header size cap added.
- [ ] Duplicate Content-Length rejected.
- [ ] Malformed frames flow server-to-client.
- [ ] Recoverable/terminal policy documented and tested.

## Service

- [ ] Same-key first use launches one real fake-server child.
- [ ] Document ownership routes through service.
- [ ] Diagnostics cache is inspected through production API.
- [ ] Shutdown races use real child tasks.

## Semantic and composite

- [ ] Typed semantic methods are called.
- [ ] Typed DTOs are asserted.
- [ ] Workspace edit preview conversion is exercised.
- [ ] No file mutation occurs.
- [ ] Real semantic context collector invoked.
- [ ] Real security context path invoked.
- [ ] Real hunk source context path invoked.

## Documentation

- [ ] Fixture self-tests and production integration tests are distinguished.
- [ ] Phase 2 is not marked complete until production runtime is exercised.
- [ ] Coverage matrix describes invariants, not only counts.

# Completion Criteria

This corrective pass is complete when:

1. The fake server is launched by Codegg’s production LSP process path.
2. Core protocol tests run through a real `LspClient` background reader and writer.
3. Service tests run through real `LspService` initialization/routing/shutdown.
4. Server requests are handled by the production dispatcher.
5. Diagnostics are stored by the production cache.
6. Timeout cancellation is generated by production code.
7. Malformed frames are emitted by the server toward the production reader.
8. Header and body memory bounds are enforced.
9. Semantic operations assert typed production outputs.
10. Rename/format/code-action tests assert production preview conversion and no mutation.
11. Semantic/security/hunk composite tests invoke actual collector paths.
12. Scenario strictness validates IDs and params.
13. The fake-server binary is hermetically built/discovered on a clean checkout.
14. Raw fixture self-tests are no longer represented as production runtime coverage.
15. Documentation accurately marks Phase 2 complete only after these conditions pass.

## Handoff Result

After this pass, the existing fake-server scenario breadth will become genuine end-to-end evidence for Codegg’s LSP implementation rather than primarily evidence for the fixture itself. The resulting suite will validate the entire production chain:

```text
LspService / LspClient
    -> production process launch
    -> production framed writer
    -> scripted fake server
    -> adversarial/normal stdout traffic
    -> production background reader
    -> response routing / server-request dispatch / diagnostics
    -> typed semantic and preview APIs
    -> production shutdown coordinator
```
