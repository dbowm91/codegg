# LSP Phase 2 Production Test Migration — Remaining Work

## Purpose

Complete the remaining Phase 2 migration after:

```text
b153e11475d10cdd78bc5d2a7fed0e49f6896d73
```

The current repository has most of the required infrastructure:

- `LspLaunchSpec` and `LspClient::new_with_launch_spec`;
- per-child command, argument, and environment configuration;
- strict scenario request/notification/response matching;
- server-originated raw frame and malformed-output actions;
- production header/body framing limits;
- duplicate `Content-Length` rejection;
- fake-server scenario-engine self-tests;
- configurable production request timeouts.

The remaining issue is execution coverage. The primary protocol and semantic suites still use a hand-written wire client that launches the fake server directly and manually performs JSON-RPC operations. They therefore do not yet exercise Codegg’s production `LspClient`, `LspService`, server-request dispatcher, pending map, diagnostics cache, semantic DTO conversion, preview conversion, or composite collectors.

This plan is intentionally focused. Do not add more fixture features unless a migrated production test requires one.

## Target State

At completion:

1. Core protocol tests create a real `LspClient`.
2. Service lifecycle tests create a real `LspService` backed by the fake server.
3. The production background reader receives all server responses, requests, notifications, malformed frames, and EOF events.
4. The production writer emits initialize, initialized, requests, notifications, cancellation, shutdown, and exit traffic.
5. Production server-request handlers generate configuration, workspace-folder, registration, progress, unknown-method, invalid-params, and apply-edit responses.
6. Production semantic APIs return typed values validated by tests.
7. Production preview APIs validate safe non-mutating workspace-edit handling.
8. Actual semantic, security, and hunk composite collectors are invoked.
9. The fake-server binary is provided through one deterministic Cargo target without nested builds or duplicate binary ownership.
10. Raw-wire tests are clearly classified as fake-server self-tests rather than production integration tests.

## Scope

Primary production files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/server_request.rs
crates/egglsp/src/lib.rs
```

Integration support:

```text
crates/egglsp/tests/common/harness.rs
crates/egglsp/tests/common/wire.rs
crates/egglsp/tests/protocol_stdio.rs
crates/egglsp/tests/semantic_stdio.rs
```

Recommended replacement layout:

```text
crates/egglsp/tests/common/production_harness.rs
crates/egglsp/tests/production_protocol_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
crates/egglsp/tests/production_semantic_stdio.rs
```

Fixture self-tests:

```text
crates/egglsp-test-server/tests/scenario_engine.rs
```

Documentation:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not implement:

- real-server compatibility CI;
- server restart supervision;
- new LSP operations only for test breadth;
- pull diagnostics;
- incremental synchronization;
- multi-root workspaces;
- direct application of workspace edits;
- broad refactors of the semantic context architecture;
- additional scenario syntax unrelated to production migration.

# Phase 1 — Consolidate Fake-Server Binary Ownership

## Current Problem

The repository currently has both:

```text
crates/egglsp/Cargo.toml [[bin]] egglsp-test-server
crates/egglsp-test-server package
```

The harness still executes a nested `cargo build -p egglsp-test-server` and parses Cargo JSON output. Documentation claims `CARGO_BIN_EXE_egglsp-test-server` discovery, but the harness does not use it.

## Required Decision

Choose exactly one binary owner.

Preferred option:

- keep the `egglsp-test-server` package for isolated scenario-engine tests;
- remove the duplicate `[[bin]]` target from `egglsp`;
- add a small build/test wrapper only if Cargo cannot expose the workspace-member binary directly to `egglsp` integration tests.

Alternative simpler option:

- make the fake server a binary target of `egglsp`;
- move scenario engine code into a reusable module under that package;
- remove the separate package.

Do not retain two binaries with the same name.

## Hermetic Path Requirements

Normal integration tests must resolve the binary without:

- searching `target/debug` or `target/release`;
- launching nested Cargo builds from test code;
- depending on a stale prebuilt executable.

Acceptable mechanisms:

1. `env!("CARGO_BIN_EXE_egglsp-test-server")` when the binary belongs to the integration-test package.
2. A test runner script or xtask that builds the dedicated package and exports an exact artifact path before invoking integration tests.
3. Cargo artifact dependencies if adopted by the workspace toolchain.

An `EGGLSP_TEST_SERVER` override may remain for manual debugging.

## Acceptance Criteria

- `cargo clean && cargo test -p egglsp --test production_protocol_stdio` works.
- No nested Cargo process is launched by test code.
- Only one Cargo target produces `egglsp-test-server`.

# Phase 2 — Build a Production `LspClient` Harness

## Harness Structure

Replace direct child stdin/stdout ownership in primary tests with:

```rust
struct ProductionClientHarness {
    tempdir: TempDir,
    root: PathBuf,
    source_path: PathBuf,
    scenario_path: PathBuf,
    transcript_path: PathBuf,
    client: Arc<LspClient>,
}
```

Constructor:

```rust
async fn start(
    scenario: serde_json::Value,
    options: LspClientOptions,
    configuration: serde_json::Value,
) -> Result<Self, LspError>
```

Required behavior:

1. Create a synthetic Rust project root.
2. Write the scenario and transcript paths.
3. Build `LspLaunchSpec` with per-child environment:
   - `CODEGG_FAKE_LSP_SCENARIO`;
   - `CODEGG_FAKE_LSP_TRANSCRIPT`.
4. Construct a real `LspClient` through `new_with_launch_spec`.
5. Call the production `initialize()` method.
6. Call the production `send_initialized()` method.
7. Return the initialized client.

## Teardown

Provide an explicit bounded teardown:

```rust
async fn shutdown(mut self)
```

It should call production `client.shutdown()` and include transcript diagnostics on error.

Existing `kill_on_drop` behavior remains the panic fallback.

## Diagnostics Helper

Add a bounded diagnostic renderer that includes:

```text
scenario name
transcript tail
client transport state
pending request count
child exit status if available
```

Avoid direct assertion panics before diagnostics can be collected in complex tests. Prefer helpers returning `Result`.

## Test-Support Accessors

Under a non-production test-support feature or `cfg(test)`-compatible integration feature, expose only what tests need:

```rust
pending_request_count()
transport_state_snapshot()
dynamic_registration_snapshot()
```

Do not expose mutable internals.

## Dynamic Registration Ownership Fix

The production `ServerRequestContext` currently owns the registration-state `Arc`, but `LspClient` does not retain it.

Move the shared handle into `LspClient`:

```rust
pub(crate) dynamic_registrations: Arc<RwLock<DynamicRegistrationState>>
```

Pass the same handle into `ServerRequestContext`.

This permits production integration assertions and avoids duplicate state ownership.

## Acceptance Criteria

- The first migrated initialize/shutdown test contains no manual framing code.
- A production client test can inspect capabilities, pending count, transport state, diagnostics, and registration snapshot.

# Phase 3 — Migrate Core Protocol Tests

Migrate existing scenarios rather than creating new parallel raw-wire equivalents.

## 3.1 Initialization

Use the production harness and strict scenario matching.

Scenario expects:

```text
initialize request
initialized notification
shutdown request
exit notification
```

Assert:

- capabilities were stored by `LspClient`;
- root URI and initialization options match;
- production shutdown completes;
- transcript contains no mismatch.

## 3.2 Server Requests During Initialization

Have the fake server send before initialize completes:

```text
workspace/configuration
workspace/workspaceFolders
window/workDoneProgress/create
client/registerCapability
```

The scenario must use `ExpectResponse` to validate Codegg’s replies.

Assert production registration state after initialization.

Add separate cases for:

- unknown method -> `-32601`;
- malformed known request -> `-32602`.

## 3.3 Apply-Edit Refusal

Delete the manual `send_error_response` behavior.

Scenario sends `workspace/applyEdit` and expects a successful result:

```json
{
  "applied": false,
  "failureReason": {"type": "String"}
}
```

Assert:

- file contents unchanged;
- client remains usable afterward;
- no JSON-RPC error response was sent.

## 3.4 Dynamic Registration

Send multiple registrations through the fake server.

Assert the production registration snapshot contains all entries.

Then send `client/unregisterCapability` using both accepted field spellings in separate tests and verify removal.

Keep over-cap atomicity as a unit test; integration only needs a representative batch.

## 3.5 Interleaving and Out-of-Order Responses

Issue real concurrent production calls:

```rust
tokio::join!(
    client.hover(...),
    client.definition(...),
    client.references(...),
    client.document_symbols(...),
)
```

The fake server returns responses in reverse order and interleaves diagnostics/progress/log notifications.

Assert:

- each call receives the correct typed response;
- diagnostics cache updates;
- pending count reaches zero.

## 3.6 Timeout and Cancellation

Use a short `LspClientOptions::request_timeout`.

Sequence:

1. Call production `send_request()` or a typed method.
2. Server intentionally withholds response.
3. Production client times out.
4. Scenario expects `$/cancelRequest` with the original ID.
5. Server emits a late response.
6. Production client ignores it.
7. A later request succeeds.

Assert pending count returns to zero.

## 3.7 Graceful and Ungraceful Exit

Production tests must cover:

- normal `shutdown` result and `exit` notification;
- process exit before response;
- nonzero exit while request pending;
- stdout closure;
- EOF mid-frame.

Assert transport failure and fail-fast subsequent behavior through production state.

# Phase 4 — Reverse Malformed Traffic Direction

## Current Problem

Existing malformed tests write invalid frames to fake-server stdin, testing the fixture parser rather than Codegg’s production reader.

## Required Migration

Use fake-server output actions:

```text
SendRawBytes
SendRawFrame
SendJsonWithDeclaredLength
SendHeaderOnly
SendBodyChunks
SendFramesTogether
CloseStdout
```

All primary malformed cases must flow:

```text
fake server stdout -> production LspClient background reader
```

## Required Cases

At minimum migrate:

- missing `Content-Length`;
- invalid numeric length;
- duplicate length;
- oversized header;
- oversized body declaration;
- EOF mid-header;
- EOF mid-body;
- invalid UTF-8 body;
- malformed JSON;
- ID-only object;
- result without ID;
- non-string method;
- fractional error code;
- response containing both result and error;
- batch array;
- multiple frames in one write;
- split header/body writes.

## Policy Assertions

Transport-terminal cases should:

- mark transport failed;
- drain pending requests;
- fail subsequent operations immediately.

Structurally unknown but valid JSON cases should:

- not resolve pending requests;
- follow the documented ignore/log policy;
- leave transport usable when intended.

## Fixture Self-Tests

Move old client-to-fixture malformed tests to `egglsp-test-server` self-tests and rename them clearly:

```text
fixture_reader_rejects_missing_content_length
fixture_reader_rejects_invalid_json
```

Do not count them as production integration coverage.

# Phase 5 — Add Real `LspService` Integration

## Service Construction

Use `LspConfig::Rules` with a command override pointing to the fake-server binary and per-process scenario environment.

The service must still instantiate a real `LspClient` through `resolve_launch_spec`.

If one scenario per client cannot be expressed through static service config, add a narrowly scoped injectable launch resolver:

```rust
trait LspLaunchResolver: Send + Sync {
    async fn resolve(
        &self,
        server: &'static LspServerDef,
        root: &Path,
    ) -> Result<LspLaunchSpec, LspError>;
}
```

Production uses the existing resolver. Integration tests use a resolver returning the fake-server launch spec.

Do not use the in-memory `test_init_fn` for Phase 2 service tests.

## Required Service Tests

### Same-key single flight with a real child

Start many concurrent calls for the same file.

The fake server transcript should record one process startup/initialize sequence.

Assert one client key and one child initialization.

### Document lifecycle and ownership

Call:

```text
service.open_file
service.update_file
service.save_file
service.close_file
```

Strict scenario matchers validate URI, language ID, versions, and full content.

Assert ownership routing and idempotent close.

### Diagnostics

Server publishes diagnostics after `didOpen`/`didChange`.

Assert through:

```text
get_diagnostics_for_key
get_diagnostic_snapshot_for_key
diagnostics_may_still_be_warming
```

### Shutdown during delayed initialization

Use a delayed initialize response or pre-initialize server request.

Call `shutdown_all()` while the real child is active.

Assert:

- callers receive cancellation;
- process exits or is killed;
- no client publishes;
- lifecycle reaches stopped.

### Shutdown with in-flight request

Issue a production request through `LspService::send_request`, then shut down.

Assert bounded completion and no pending leak.

# Phase 6 — Migrate Semantic Tests to Typed APIs

## 6.1 Basic Semantic Methods

Replace raw JSON requests with production methods:

```text
hover
definition
references
document_symbols
workspace_symbols
implementation
type_definition
```

Assert typed locations, ranges, symbols, hover contents, and normalized paths.

Strict scenarios validate position and URI parameters.

## 6.2 Hierarchy

Call production call/type hierarchy methods.

Assert typed item conversion and bounded behavior.

Only include operations already supported by the client.

## 6.3 Preview-Only Edits

Call actual rename, formatting, and code-action APIs plus preview conversion.

Required fixtures:

```text
changes map
documentChanges
multi-file edit
overlapping edits
out-of-root URI
resource operation
command-only code action
invalid range
```

Assert:

- valid previews are typed correctly;
- unsupported or unsafe edits are rejected;
- no file content changes;
- root boundaries remain enforced.

## 6.4 Diagnostics Cache

Use server notifications and inspect production cache behavior:

- initial diagnostics;
- empty publication clearing diagnostics;
- versioned diagnostics;
- two URIs;
- stale/possibly-stale classification after update;
- malformed diagnostics ignored without killing transport.

# Phase 7 — Exercise Actual Composite Collectors

## Semantic Context

Invoke the real semantic-context collector/service path used by Codegg.

Do not manually reproduce its constituent requests.

Assert the final response contains expected:

```text
source excerpt
diagnostics and freshness metadata
symbols
hover
definitions
references
hierarchy summaries
notes/errors
truncation markers
```

## Security Context

Invoke the actual security-context path.

Assert:

- diagnostic evidence reuse;
- capability gating;
- bounded call expansion;
- deterministic ordering;
- graceful degradation of optional operations.

## Hunk Source Context

Invoke the real hunk navigator/collector with a synthetic diff.

Assert:

- hunk-to-source position mapping;
- symbol/definition/reference evidence;
- stable excerpts;
- bounded request count;
- current first-anchor behavior.

Do not implement hunk clustering here.

# Phase 8 — Reclassify and Remove Manual Wire Tests

## Required Classification

After migration, every test should be one of:

```text
fixture self-test
production LspClient integration
production LspService integration
production semantic/composite integration
```

## Manual Wire Helpers

Move `tests/common/wire.rs` into the fake-server package or rename it to `fixture_wire.rs`.

Primary production suites must not directly own child stdin/stdout.

Delete redundant raw-wire tests once equivalent production coverage exists.

## Documentation

Replace raw test-count claims with an invariant matrix.

Phase 2 should not be marked complete until all completion criteria below pass.

# Suggested Implementation Order

1. Consolidate fake-server binary ownership and eliminate nested Cargo builds.
2. Add `ProductionClientHarness` and one initialize/shutdown smoke test.
3. Retain dynamic registration state on `LspClient` and add read-only test snapshot.
4. Migrate apply-edit and server-request tests.
5. Migrate timeout/cancel and out-of-order response tests.
6. Migrate malformed server-output tests.
7. Add real `LspService` harness and lifecycle tests.
8. Migrate typed semantic and edit-preview tests.
9. Invoke actual composite collectors.
10. Move/delete raw-wire tests and correct documentation.

# Verification Commands

Binary and clean-checkout verification:

```bash
cargo clean
cargo test -p egglsp --test production_protocol_stdio production_initialization_handshake
```

Production client tests:

```bash
cargo test -p egglsp --test production_protocol_stdio -- --test-threads=1
cargo test -p egglsp --test production_protocol_stdio -- --test-threads=8
```

Production service tests:

```bash
cargo test -p egglsp --test production_service_stdio -- --test-threads=1
cargo test -p egglsp --test production_service_stdio -- --test-threads=8
```

Semantic/composite tests:

```bash
cargo test -p egglsp --test production_semantic_stdio -- --test-threads=1
cargo test -p egglsp --test production_semantic_stdio -- --test-threads=8
```

Fixture self-tests:

```bash
cargo test -p egglsp-test-server
```

Full verification:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

# Review Checklist

## Binary and harness

- [ ] Exactly one fake-server binary target exists.
- [ ] No test launches nested Cargo.
- [ ] Clean checkout builds and resolves the fixture automatically.
- [ ] Production harness constructs `LspClient::new_with_launch_spec`.
- [ ] Scenario and transcript env are per child.

## Production protocol

- [ ] Initialize/initialized use production methods.
- [ ] Server requests use production dispatcher.
- [ ] Apply-edit returns `applied: false` as a result.
- [ ] Dynamic registration state is mutated and inspectable.
- [ ] Timeout emits production `$/cancelRequest`.
- [ ] Late responses are ignored.
- [ ] Pending state returns to zero.
- [ ] EOF/crash causes fail-fast transport state.
- [ ] Production shutdown sends shutdown/exit.

## Malformed traffic

- [ ] Malformed frames are sent by server stdout to production reader.
- [ ] Header/body limits are exercised end to end.
- [ ] Duplicate `Content-Length` rejection is exercised.
- [ ] Recoverable unknown messages do not resolve pending requests.
- [ ] Terminal framing/JSON errors drain pending requests.

## Service

- [ ] Same-key concurrent first use launches one real child.
- [ ] Document ownership uses real service routing.
- [ ] Diagnostics are retrieved through service APIs.
- [ ] Delayed initialization shutdown is tested with a real child.
- [ ] In-flight request shutdown is bounded.

## Semantic and composite

- [ ] Typed semantic methods are called.
- [ ] Typed result conversion is asserted.
- [ ] Edit previews are generated by production code.
- [ ] No file mutation occurs.
- [ ] Semantic context collector is invoked directly.
- [ ] Security context path is invoked directly.
- [ ] Hunk source context path is invoked directly.

## Cleanup

- [ ] Raw-wire tests are moved, renamed, or deleted.
- [ ] Fixture self-tests are not represented as production coverage.
- [ ] Documentation no longer overstates Phase 2 completion.

# Completion Criteria

This remaining-work pass is complete when:

1. The fake server is launched through Codegg’s production process path in all primary integration suites.
2. Core protocol tests use a real production `LspClient`.
3. Lifecycle/routing tests use a real production `LspService`.
4. Production server-request handlers are validated through child stdio.
5. Production timeout logic emits and matches `$/cancelRequest`.
6. Production diagnostics cache behavior is tested end to end.
7. Malformed server output reaches the production reader.
8. Production semantic methods and typed conversions are asserted.
9. Production preview conversion and no-mutation guarantees are asserted.
10. Actual semantic, security, and hunk composite collectors are exercised.
11. The fake-server binary is hermetically resolved without nested builds.
12. Manual wire tests are clearly isolated as fixture self-tests.
13. Documentation accurately marks Phase 2 complete only after these conditions pass.

## Handoff Result

The repository already contains the fixture capabilities required for this work. The next implementation should primarily replace test drivers, not expand the fake-server language. Once migrated, the suite will validate the real production chain:

```text
LspService / LspClient
    -> LspLaunchSpec / production spawn
    -> LspWriter
    -> fake server
    -> production background reader
    -> pending routing / server-request dispatch / diagnostics
    -> typed semantic and preview APIs
    -> production shutdown
```
