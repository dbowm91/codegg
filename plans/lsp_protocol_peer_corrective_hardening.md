# LSP Protocol Peer Corrective Hardening Plan

## Purpose

Complete Phase 1 of the LSP roadmap by correcting the protocol and concurrency gaps remaining after:

```text
c4fa3e630a20248f6c3bf98b147e736d42b8a561
```

The existing implementation is directionally correct and should be preserved. This pass is not a redesign. It should tighten standards compliance, deterministic client routing, failure propagation, and initialization/shutdown semantics so the repository is ready for the later scripted fake-LSP-server integration harness.

## Scope

Primary files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/server_request.rs
crates/egglsp/src/service.rs
crates/egglsp/src/writer.rs
crates/egglsp/src/error.rs
crates/egglsp/src/config.rs
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Do not add new model-facing LSP operations, automatic restart supervision, pull diagnostics, incremental synchronization, multi-file overlays, or the full child-process fake-server harness in this pass.

## Preserve These Existing Improvements

Do not regress:

- the single background stdout reader;
- the shared serialized `LspWriter`;
- distinct response/error/server-request/notification routing;
- string and numeric JSON-RPC ID preservation;
- best-effort `$/cancelRequest` on timeout;
- read-only LSP tool categorization;
- explicit refusal of implicit server-driven edits;
- compact model-facing DTOs;
- `Arc<LspClient>` ownership;
- release of the global client map lock before normal client I/O;
- bounded dynamic registration storage;
- server-scoped workspace configuration.

## Required Invariants

1. `workspace/applyEdit` is rejected with a valid successful LSP response body, never by implicitly mutating files.
2. Registration and unregistration requests process all array entries.
3. Malformed JSON-RPC messages are never accepted as successful responses.
4. The global client map lock is never held while awaiting a per-client lock or process I/O.
5. Document ownership is deterministic and does not depend on `HashMap` iteration order.
6. `close_file` and `save_file` remain idempotent for non-open files unless a documented caller contract requires otherwise.
7. All callers waiting on one initialization attempt observe the same attempt result.
8. Initialization failure and cancellation do not leak permanent coordination entries.
9. Shutdown cannot race with initialization and allow a client to be installed after shutdown begins.
10. A broken protocol writer causes the client transport to fail explicitly rather than continuing in a half-alive state.

## Phase 1 — Correct `workspace/applyEdit` Semantics

In `crates/egglsp/src/server_request.rs`, return a normal result:

```rust
ServerRequestReply::Result(serde_json::json!({
    "applied": false,
    "failureReason": "Codegg does not permit implicit language-server edits; request a preview and apply it through the authorized patch path"
}))
```

Do not return JSON-RPC error `-32600`. The request itself is valid; Codegg is declining the requested edit.

Required tests:

- result envelope contains `applied: false`;
- result contains a non-empty `failureReason`;
- no `error` envelope is produced;
- request body may contain `changes`, `documentChanges`, or both without changing the refusal behavior;
- malformed params may still return `-32602` if validation is intentionally enforced, but valid edit requests must return a normal result.

Acceptance criteria:

- A conforming server can distinguish “client declined edit” from “client protocol failure.”
- No path writes files or invokes Codegg mutation tools.

## Phase 2 — Implement Full Registration and Unregistration Arrays

### `client/registerCapability`

Parse the complete `registrations` array and apply every valid entry.

Recommended approach:

1. Deserialize or validate all entries first.
2. Reject the whole request with `-32602` if any entry is malformed.
3. Apply the set atomically after validation.
4. Preserve `id`, `method`, and optional `registerOptions`.

If atomic application would require disproportionate complexity, partial application is acceptable only if explicitly documented and tested, but all entries must still be processed.

The registration cap must distinguish new IDs from replacement IDs:

- replacing an existing ID is allowed at the cap;
- adding a new ID beyond the cap fails;
- duplicate IDs within one request have deterministic last-write-wins or explicit invalid-params behavior.

### `client/unregisterCapability`

Support the LSP request shape:

```json
{
  "unregisterations": [
    {"id": "...", "method": "..."}
  ]
}
```

The misspelled `unregisterations` field is part of the protocol shape and must be supported. Also accept `unregistrations` as a compatibility alias if practical.

Process every array item. Unknown IDs remain tolerated. Validate that each item contains an ID; method may be retained for diagnostics but removal should be keyed by ID.

Required tests:

- multiple registrations in one request;
- multiple unregistrations in one request;
- protocol spelling `unregisterations`;
- optional compatibility spelling `unregistrations`;
- replacement at exact cap succeeds;
- new registration above cap fails;
- malformed second entry does not silently leave ambiguous state;
- duplicate IDs have documented behavior.

Acceptance criteria:

- A conforming server can register and unregister multiple capabilities in one request.
- State remains bounded and deterministic.

## Phase 3 — Make Incoming JSON-RPC Classification Strict

In `client.rs`, require explicit structural fields:

```text
id + method         => server request
id + valid error    => error response
id + result field   => success response
method without id   => notification
otherwise           => unknown
```

Do not treat an object containing only an ID as a success response with `result: null`.

Strengthen error-response parsing:

- require `error` to be an object;
- require a numeric `code` and string `message` for a valid error response;
- preserve optional `data`;
- malformed error objects become `Unknown` or a dedicated malformed variant, not a synthetic “unknown error” response.

Consider changing numeric IDs from `u64` to `i64` so server-originated negative integral IDs remain representable. Client-generated IDs may remain nonnegative.

Do not accept floating-point IDs.

Required tests:

- ID-only object is `Unknown`;
- response with explicit `result: null` is valid;
- malformed error object is not routed to pending as a normal error;
- negative integer server-request ID is preserved if signed support is implemented;
- floating-point and object IDs are rejected;
- string IDs remain supported.

Acceptance criteria:

- Only valid response-shaped messages can complete pending client requests.

## Phase 4 — Remove Remaining Global Map Lock Awaits

`close_file` and `save_file` currently hold the `clients` read guard while awaiting each client's `opened_files` mutex. Remove this pattern.

Preferred design: add deterministic document ownership tracking to `LspService`:

```rust
open_document_owners: Arc<RwLock<HashMap<String, String>>>
```

where:

```text
URI -> client key
```

Update ownership only after successful `didOpen`; remove ownership after successful or intentionally idempotent `didClose`.

If introducing the index in this pass:

- `open_file` records ownership;
- `update_file` verifies or resolves ownership;
- `close_file` resolves directly by URI;
- `save_file` resolves directly by URI;
- shutdown clears the ownership map;
- duplicate ownership attempts are deterministic and logged;
- no global map guard is retained while awaiting a client operation.

A less preferred fallback is:

1. clone all client handles under the map lock;
2. release the map lock;
3. inspect per-client document state;

This removes lock inversion but still leaves ambiguous ownership. Use it only as an intermediate step.

Restore idempotent behavior:

- closing a non-open file returns `Ok(())`;
- saving a non-open file should remain a no-op unless existing callers require an error;
- repeated close does not fail.

Required tests:

- close/save do not hold the client map lock while waiting on per-client state;
- close/save route to the exact owner;
- repeated close succeeds;
- close of never-opened file succeeds;
- duplicate URI ownership has deterministic behavior;
- shutdown clears ownership state.

Acceptance criteria:

- No service method holds the global client map lock across any await other than acquiring/releasing that lock itself.
- Routing never depends on map iteration order.

## Phase 5 — Strengthen Single-Flight Initialization Semantics

The current `OnceCell` implementation shares success, but concurrent callers may trigger sequential retries after one failed attempt. Replace or wrap it with an explicit per-attempt result-sharing slot.

Recommended shape:

```rust
enum ClientSlotState {
    Starting {
        waiters: Vec<oneshot::Sender<Result<Arc<LspClient>, SharedInitError>>>,
        generation: u64,
    },
    Ready(Arc<LspClient>),
}
```

or an equivalent shared future/channel design.

Required behavior:

- exactly one initializer runs for one key and one attempt;
- all callers already waiting on that attempt receive the same success or failure;
- after failure, the slot is removed or transitioned so a later call can retry;
- cancellation of the initiating caller does not abandon the shared attempt if another owned task can complete it;
- cancellation or panic cannot leave a permanent `Starting` entry;
- different keys initialize concurrently.

Use an owned spawned initialization task if needed so the attempt lifetime is not tied to one caller future. If spawning, retain explicit cleanup and error propagation.

Required tests with an injected factory/seam:

- N same-key callers invoke the factory exactly once on success;
- N same-key callers invoke the factory exactly once on failure and all receive the same failure;
- a later call after failure initiates one new attempt;
- initiator cancellation does not deadlock or duplicate work;
- different keys initialize concurrently;
- no stale initialization-map entries remain after success, failure, or cancellation.

Acceptance criteria:

- “single-flight” applies to failure as well as success.

## Phase 6 — Coordinate Shutdown with Initialization

Add explicit service lifecycle state, for example:

```rust
enum ServiceLifecycle {
    Running { generation: u64 },
    ShuttingDown { generation: u64 },
    Stopped { generation: u64 },
}
```

Minimum required semantics:

- once shutdown begins, new client acquisition/initialization is rejected;
- an in-flight initializer cannot install a client after shutdown has drained clients;
- if an in-flight client finishes during shutdown, it is immediately shut down or discarded safely;
- shutdown drains both ready clients and coordination state;
- repeated shutdown is idempotent;
- no lock is held while awaiting client shutdown.

A generation token is a practical implementation:

1. acquisition captures the current running generation;
2. initialization completes;
3. before installation, verify lifecycle is still running at the same generation;
4. otherwise shut down the just-created client and return `InitializationCancelled`.

Required tests:

- shutdown racing successful initialization leaves no installed client;
- shutdown racing failed initialization leaves no stale slot;
- acquisition after shutdown returns a stable error;
- repeated shutdown succeeds;
- restart/re-enable semantics remain out of scope unless already required.

Acceptance criteria:

- no language-server process can appear in the service after shutdown has completed.

## Phase 7 — Propagate Writer Failure as Transport Failure

When the background reader cannot send a result or error response to a server request, do not only log and continue.

At minimum:

1. record an explicit transport failure reason;
2. fail all pending client requests;
3. terminate the reader loop;
4. allow process drop/termination behavior to clean up the child.

If a shared client-health state is easy to introduce, add:

```rust
enum ClientTransportState {
    Running,
    Failed { reason: String },
}
```

Subsequent requests should fail immediately rather than waiting for timeout after a known writer failure.

Also audit normal request/notification write failures:

- pending entry cleanup remains correct;
- client transport becomes failed when stdin is broken;
- later requests fail fast;
- error messages retain server/method context without dumping payloads.

Required tests:

- server-response write failure fails all pending requests;
- reader exits after response write failure;
- later client request fails fast against failed transport;
- normal request write failure removes pending state;
- cancellation write failure does not mask the original timeout.

Acceptance criteria:

- Codegg does not continue presenting a broken stdin/stdout pair as a usable client.

## Phase 8 — Tighten Tests Around Real Invariants

The current unit coverage is broad but should be adjusted to test the exact protocol and concurrency contracts rather than the previous incorrect assumptions.

Required focused suites:

### `server_request.rs`

- apply-edit normal result refusal;
- full registration arrays;
- full unregistration arrays;
- protocol spelling compatibility;
- cap replacement semantics;
- malformed array atomicity or documented partial semantics.

### `client.rs`

- strict classifier shapes;
- signed/string IDs as supported;
- malformed response does not resolve pending;
- writer failure propagates transport failure;
- timeout/cancel race remains leak-free.

### `service.rs`

Introduce a fake client factory or equivalent seam. Test:

- shared success;
- shared failure;
- retry after failure;
- cancellation cleanup;
- independent-key parallelism;
- shutdown race;
- exact URI ownership routing;
- idempotent close/save;
- no global lock held across slow client operations.

Do not require any installed language server, network access, or subprocess fixture yet.

## Phase 9 — Documentation Corrections

Update:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Correct any statements that currently imply:

- apply-edit refusal is a JSON-RPC error;
- registration support is complete when only the first item is processed;
- all service operations release the map lock before awaits;
- failure-path single-flight semantics are complete;
- shutdown/init races are already handled.

After implementation, document:

- apply-edit application-level refusal;
- exact registration/unregistration array behavior;
- document ownership index or chosen deterministic routing mechanism;
- initialization attempt semantics;
- shutdown generation/lifecycle behavior;
- transport failure behavior;
- remaining Phase 2 limitation: no scripted stdio fake server yet.

## Suggested Implementation Order

1. Correct apply-edit response and tests.
2. Correct registration/unregistration parsing and cap replacement.
3. Tighten JSON-RPC classifier.
4. Add deterministic document ownership and idempotent close/save.
5. Replace success-only `OnceCell` semantics with shared per-attempt results.
6. Add shutdown lifecycle/generation coordination.
7. Propagate writer failure to client transport failure.
8. Add concurrency/failure-path tests.
9. Update documentation and run the full regression suite.

## Verification

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test -p egglsp
cargo test --workspace
cargo clippy -p egglsp --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
```

If workspace-wide tests or clippy retain unrelated baseline failures, record the exact test names and diagnostics. Modified LSP crates and files must be clean.

## Review Checklist

- [ ] `workspace/applyEdit` returns `result`, not `error`.
- [ ] Every registration in `registrations` is processed.
- [ ] Every item in `unregisterations` is processed.
- [ ] Existing registration replacement works at the cap.
- [ ] ID-only messages are not successful responses.
- [ ] Malformed error objects cannot resolve pending requests.
- [ ] Signed integral IDs are handled or the limitation is explicitly justified.
- [ ] No client map guard survives an await on client state or I/O.
- [ ] URI ownership is exact and deterministic.
- [ ] Duplicate/nonexistent close is idempotent.
- [ ] All waiters share one initialization failure result.
- [ ] Initialization coordination state is cleaned on every terminal path.
- [ ] Shutdown prevents post-shutdown installation.
- [ ] Writer failure fails pending requests and exits the transport loop.
- [ ] Existing semantic, security, overlay, hierarchy, and hunk workflows remain compatible.
- [ ] No external language server is required by the default tests.

## Completion Criteria

This corrective pass is complete when:

1. The supported server-request subset is protocol-correct for valid request shapes.
2. Apply-edit refusal is represented as a valid `ApplyWorkspaceEditResponse` result.
3. Dynamic registration arrays are fully processed and bounded correctly.
4. Malformed JSON-RPC cannot complete pending requests.
5. Client-map lock discipline is actually true for every service path.
6. File lifecycle routing is deterministic and idempotent.
7. One initialization attempt produces one shared result for all current callers.
8. Failed/cancelled attempts leave no stale coordination entries.
9. Shutdown and initialization cannot install a client after shutdown.
10. Broken writer state becomes explicit transport failure.
11. Focused unit/concurrency tests cover these failure paths without external servers.
12. Documentation accurately reflects the implemented behavior.

After these criteria are satisfied, Phase 1 can be considered complete and the next handoff should implement the deterministic scripted stdio fake-LSP-server harness.
