# LSP Phase 1 Final Corrective Hardening Plan

## Purpose

Complete the remaining correctness work after:

```text
89b844e6ed46d18c47781393f47a76fbe0a786b9
```

The previous pass fixed most protocol and ownership issues, but it introduced a release-blocking initialization deadlock and left several failure-path semantics incomplete. This plan is intentionally narrow. It should finish Phase 1 without expanding the LSP feature surface.

## Primary Goal

Make cold-start LSP initialization, concurrent initialization, shutdown coordination, and transport failure behavior correct and testable.

The repository should emerge from this pass with:

- a functioning leader/waiter single-flight initializer;
- no self-wait deadlock;
- exact failure sharing across waiters;
- no post-shutdown client installation;
- unified transport-failure state transitions;
- atomic dynamic-registration batches;
- strict integral JSON-RPC error-code validation;
- concurrency tests that exercise real coordinator behavior rather than enum shape.

## Scope

Primary files:

```text
crates/egglsp/src/service.rs
crates/egglsp/src/client.rs
crates/egglsp/src/server_request.rs
crates/egglsp/src/error.rs
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Possible new test-support module:

```text
crates/egglsp/src/test_support.rs
```

or private test-only helpers inside `service.rs` and `client.rs`.

## Non-Goals

Do not implement:

- the scripted child-process fake LSP server;
- automatic restart supervision;
- pull diagnostics;
- version-aware diagnostic waiting;
- incremental text synchronization;
- multi-root workspace support;
- hunk clustering;
- new model-facing LSP operations;
- direct mutation through `workspace/applyEdit`.

## Preserve Existing Correct Behavior

Do not regress:

- application-level `workspace/applyEdit` refusal with `applied: false`;
- full registration/unregistration arrays;
- signed and string JSON-RPC IDs;
- ID-only messages classified as `Unknown`;
- deterministic document ownership through URI → client key mapping;
- idempotent close/save behavior;
- global client-map lock release before client I/O;
- shared serialized writer;
- timeout cancellation via `$/cancelRequest`;
- bounded server-request dispatch;
- server-response writer failure draining pending requests;
- read-only model-facing LSP semantics.

## Required Invariants

1. The first caller for an uninitialized client key must execute initialization.
2. The first caller must never wait on a channel that only it can resolve.
3. All concurrent callers for one initialization attempt observe the same result.
4. A failed attempt is removed so a later independent call can retry.
5. Cancellation or panic cannot leave a permanent `Starting` slot.
6. Shutdown prevents installation of a client created by an older lifecycle generation.
7. A client created after shutdown begins is immediately disposed and never published.
8. Any terminal stdin or stdout transport failure transitions the client to `Failed` exactly once.
9. Once transport is failed, later requests and notifications fail immediately.
10. Dynamic registration requests either apply the entire batch or none of it.
11. JSON-RPC error codes must be integral values representable by the chosen ID/error type.
12. Tests must execute the actual asynchronous coordination paths.

# Phase 1 — Replace the Broken Leader/Waiter Election

## Current Defect

The current first-caller branch creates a oneshot sender/receiver pair, stores the sender in the waiter list, returns the receiver, and then waits on it. No branch elects a caller to proceed directly into `init_client_inner`.

This creates a self-wait deadlock on cold initialization.

## Required Design

Use an explicit election result:

```rust
enum InitRole {
    Leader {
        attempt_id: u64,
    },
    Waiter {
        receiver: oneshot::Receiver<Result<Arc<LspClient>, SharedInitError>>,
        attempt_id: u64,
    },
    Ready(Arc<LspClient>),
}
```

The first caller must receive `Leader`, not a receiver.

Recommended slot:

```rust
struct InitSlot {
    attempt_id: u64,
    state: InitSlotState,
}

enum InitSlotState {
    Starting {
        waiters: Vec<oneshot::Sender<Result<Arc<LspClient>, SharedInitError>>>,
    },
    Ready(Arc<LspClient>),
}
```

Alternative shared-future or watch-channel designs are acceptable if they preserve the same semantics.

## Election Algorithm

Under the initialization-map lock:

1. If no slot exists:
   - allocate a new attempt ID;
   - insert `Starting { waiters: [] }`;
   - return `InitRole::Leader`.
2. If a `Starting` slot exists:
   - create a oneshot pair;
   - append sender to waiters;
   - return `InitRole::Waiter`.
3. If a `Ready` slot exists:
   - return the client immediately.

Do not encode leader election through `waiters.is_empty()`. An empty waiter list is valid while a leader is already running.

## Completion Path

Leader success:

1. Produce `Arc<LspClient>`.
2. Recheck lifecycle generation before publication.
3. Insert into `clients` only if still valid.
4. Transition or remove the initialization slot.
5. Send the same `Arc<LspClient>` to every waiter.
6. Return success to the leader.

Leader failure:

1. Capture a cloneable shared failure representation.
2. Remove the slot only if its attempt ID still matches.
3. Send the same failure representation to every waiter.
4. Return an equivalent error to the leader.

## Acceptance Criteria

- Cold first-use executes `init_client_inner` exactly once.
- No self-wait is possible by construction.
- Same-key concurrent callers do not duplicate initialization.
- Different keys can initialize concurrently.

# Phase 2 — Add a Cloneable Shared Initialization Error

## Problem

The current leader receives the original `LspError`, while waiters receive a generic `InitializationCancelled("init failed")` error. This loses the actual failure cause.

## Required Design

Introduce a cloneable representation, for example:

```rust
#[derive(Debug, Clone)]
pub struct SharedInitError {
    pub kind: SharedInitErrorKind,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum SharedInitErrorKind {
    ServerNotFound,
    DownloadFailed,
    LaunchFailed,
    InitializeFailed,
    Timeout,
    Cancelled,
    Protocol,
    Other,
}
```

Provide:

```rust
impl From<&LspError> for SharedInitError
fn into_lsp_error(self) -> LspError
```

Exact variant preservation is preferred where practical. At minimum, preserve the original message and a stable error category.

Do not require `LspError: Clone` if that would force broad unrelated refactoring.

## Acceptance Criteria

- Leader and all waiters receive the same category and message.
- Logs and UI errors retain the real startup cause.
- Retry behavior remains unchanged after failure notification.

# Phase 3 — Make Initialization Cancellation-Safe

## Problem

If leader initialization remains tied directly to the requesting future, dropping that future can abandon the attempt and strand waiters.

## Preferred Design

Run the initialization attempt in an owned task after leader election:

```rust
let task = tokio::spawn(run_initialization_attempt(...));
```

The task owns:

- server definition;
- root;
- client key;
- attempt ID;
- lifecycle generation;
- initialization slot/map handles;
- client map handle;
- waiter completion.

The original caller can await the task result through the same shared completion mechanism as other waiters, or the leader can await the join handle while the owned task guarantees cleanup.

If not spawning, use a drop guard that:

- removes the matching attempt slot;
- notifies all waiters with cancellation;
- cannot accidentally remove a newer retry slot.

## Attempt Identity

Every slot must have an attempt ID or generation. Cleanup must use compare-and-remove semantics:

```text
remove only if current slot attempt_id == completing attempt_id
```

This prevents a cancelled old attempt from deleting a newer retry.

## Acceptance Criteria

- Dropping the initiating caller does not deadlock waiters.
- Waiters receive either the completed result or an explicit cancellation result.
- No stale `Starting` slot remains.
- A later retry is not removed by stale cleanup from an older attempt.

# Phase 4 — Coordinate Lifecycle Generation with Publication

## Current Defect

Lifecycle is checked before initialization begins but not before the client is inserted. Shutdown can drain current clients while an in-flight initializer later publishes a new one.

## Required Design

Track a lifecycle generation:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LifecycleState {
    phase: ServiceLifecycle,
    generation: u64,
}
```

Suggested transitions:

```text
Running(g) -> ShuttingDown(g + 1) -> Stopped(g + 1)
```

At election time, capture the current running generation.

Before client publication:

1. Read lifecycle state.
2. Confirm `phase == Running`.
3. Confirm `generation == captured_generation`.
4. Only then insert into `clients`.

If validation fails:

- do not insert the client;
- gracefully shut it down if initialization completed far enough;
- notify waiters with `InitializationCancelled` or a shared lifecycle error;
- remove the matching slot.

## Shutdown Semantics

`shutdown_all()` must:

1. atomically transition to `ShuttingDown` and increment generation;
2. prevent new elections;
3. collect ready clients without holding locks across shutdown I/O;
4. invalidate in-flight attempts by generation mismatch;
5. clear document ownership;
6. wait for or cancel in-flight initialization tasks according to a documented policy;
7. transition to `Stopped`;
8. remain idempotent.

Simply clearing the initialization map is insufficient if tasks still hold slot Arcs.

## Acceptance Criteria

- No client can appear in `clients` after shutdown completes.
- A client finishing during shutdown is disposed rather than published.
- Repeated shutdown remains safe.

# Phase 5 — Centralize Transport Failure Handling

## Current Gap

Transport state is marked failed only when a server-request response write fails. Other terminal failures still leave the state marked `Running`:

- stdout EOF;
- framing failure;
- malformed/terminal reader failure;
- normal request write failure;
- normal notification write failure.

## Required Design

Add one helper:

```rust
async fn fail_transport(
    transport_state: &Arc<Mutex<ClientTransportState>>,
    pending: &PendingMap,
    reason: impl Into<String>,
)
```

Behavior:

1. Transition `Running -> Failed { reason }`.
2. Preserve the first terminal failure reason.
3. Drain all pending requests exactly once.
4. Be idempotent if already failed.

Use it for:

- stdout reader termination;
- server-response result write failure;
- server-response error write failure;
- request write failure;
- notification write failure;
- any explicit writer-closed path.

## Request/Notification Behavior

Before writing:

- fail fast if transport is failed.

After write failure:

- remove the current request from pending if necessary;
- call `fail_transport`;
- return `WriterClosed` or a transport-specific error.

Timeout cancellation write failure should not replace the original timeout, but if the write indicates the pipe is broken, it should still transition transport state to failed.

## Reader Exit

Unexpected stdout EOF is terminal. It must mark transport failed before exiting.

Graceful shutdown may use a distinct expected-closure path if needed to avoid noisy warnings, but later requests must still not treat the transport as running.

## Acceptance Criteria

- Every terminal pipe failure produces one stable failed state.
- Pending requests fail promptly.
- Later operations fail immediately.
- No known-dead transport waits for the 30-second request timeout.

# Phase 6 — Make Dynamic Registration Batches Atomic

## Current Gap

Entry shape validation is atomic, but capacity failure can occur after earlier entries have already been inserted.

## Required Algorithm

After validation and deduplication, while holding the registration-state write lock:

1. Count IDs not already present.
2. Compute:

```text
state.count() + new_id_count
```

3. If the total exceeds `MAX_REGISTRATIONS`, reject the whole request before mutation.
4. Otherwise apply all replacements and additions.

Expose a method such as:

```rust
pub fn register_batch(
    &mut self,
    registrations: Vec<DynamicRegistration>,
) -> Result<(), String>
```

Keep cap logic inside `DynamicRegistrationState` rather than duplicating it in the dispatcher.

## Tests

- count 255 + two new registrations rejects and leaves count/state unchanged;
- count 256 + replacement succeeds;
- mixed replacement + one new at 255 succeeds;
- duplicate IDs remain last-write-wins before capacity accounting;
- malformed input leaves state unchanged.

## Acceptance Criteria

- Server receives success only if the entire registration batch is applied.
- Failure leaves registration state byte-for-byte equivalent to its prior logical state.

# Phase 7 — Require Integral JSON-RPC Error Codes

## Current Gap

`is_structural_error` accepts any JSON number, including floating-point values, then `as_i64()` may yield `None`.

## Required Fix

Replace:

```rust
c.is_number()
```

with:

```rust
c.as_i64().is_some()
```

Then simplify `ErrorResponse` if appropriate:

```rust
code: i64
```

rather than `Option<i64>`, because a structurally valid error must have an integral code.

Do not broaden this into a larger public API change unless needed.

## Tests

- `code: -32601` is valid;
- `code: 1.5` is `Unknown`;
- string code is `Unknown`;
- missing code is `Unknown`;
- missing message is `Unknown`.

# Phase 8 — Add Real Coordinator Test Seams

The existing tests inspect state shapes but do not execute the asynchronous control flow. Introduce an injectable initialization seam.

## Suggested Factory Trait

```rust
#[async_trait]
trait LspClientFactory: Send + Sync {
    async fn initialize(
        &self,
        server: &'static LspServerDef,
        root: &Path,
    ) -> Result<Arc<LspClient>, LspError>;
}
```

Production uses the real factory. Tests use a fake result type or test client handle if constructing a full `LspClient` is too expensive.

An alternative generic internal `ClientHandle` or closure-based initializer is acceptable.

Avoid requiring a real language-server process.

## Required Tests

### Leader/waiter

- first caller reaches initializer;
- 20 concurrent same-key callers invoke initializer once;
- all receive the same client identity;
- no caller waits indefinitely;
- different keys invoke independent initializers concurrently.

### Failure sharing

- 20 same-key callers invoke one failing attempt;
- all receive the same error category and message;
- slot is removed afterward;
- later call retries and can succeed.

### Cancellation

- leader caller is dropped while initialization remains in progress;
- waiters still resolve or receive explicit cancellation;
- no stale slot remains;
- retry is possible.

### Shutdown race

- initialization blocks on a test barrier;
- shutdown begins;
- initializer completes;
- client is not installed;
- disposal/shutdown callback is observed;
- service ends stopped with empty client and ownership maps.

### Transport failure

- reader EOF marks failed;
- request write failure marks failed;
- notification write failure marks failed;
- all pending requests are drained;
- later request fails immediately;
- repeated failure preserves the first reason or follows documented overwrite semantics.

### Registration atomicity

- over-cap batch leaves no partial state.

### Classifier

- fractional error code is rejected.

## Time-Bounded Tests

Use short `tokio::time::timeout` guards in concurrency tests so deadlocks fail quickly and explicitly.

Do not rely on sleeps when barriers, notifies, semaphores, or channels can provide deterministic ordering.

# Phase 9 — Audit Lock Ordering

Document and enforce a simple lock order:

```text
lifecycle
initialization map
individual init slot
clients map
document owners
client-local state
transport state
pending map
writer
```

Prefer not holding more than one lock at a time.

Specific audit targets:

- no `clients` lock while awaiting client shutdown;
- no lifecycle write lock while awaiting process I/O;
- no init-slot lock while awaiting `init_client_inner`;
- no pending-map lock while writing;
- no transport-state lock while draining pending;
- no document-owner lock while invoking client methods.

Use scoped blocks and cloned handles to make lock release obvious.

# Phase 10 — Documentation Corrections

Update documentation only after behavior is correct.

Correct claims around:

- custom `InitSlot` behavior;
- shared failure results;
- cancellation safety;
- shutdown coordination;
- transport failure coverage;
- registration atomicity.

Do not claim Phase 1 complete until the coordinator tests pass.

Document the remaining boundary:

> The default suite validates protocol and concurrency behavior through in-memory test seams. The next roadmap phase adds an actual scripted stdio child-process LSP fixture.

# Suggested Implementation Order

1. Fix leader/waiter election with explicit `InitRole`.
2. Add injected initializer seam and a cold-start test immediately.
3. Add shared initialization error type.
4. Make attempt cleanup cancellation-safe with attempt IDs.
5. Add lifecycle generation recheck before publication.
6. Add shutdown-race tests.
7. Centralize transport failure handling.
8. Make registration batches atomic.
9. Tighten integral error-code classification.
10. Audit locks, update docs, run full verification.

# File-Level Guidance

## `crates/egglsp/src/service.rs`

Expected changes:

- replace current waiter-empty leader detection;
- add explicit leader/waiter role;
- add attempt IDs;
- add cloneable shared failure propagation;
- add lifecycle generation;
- recheck generation before publication;
- prevent stale attempt cleanup from deleting newer slots;
- add injected initializer/factory test seam;
- add real async concurrency tests.

## `crates/egglsp/src/client.rs`

Expected changes:

- centralize transport failure;
- mark failed on reader EOF and normal write failures;
- tighten structural error validation;
- add transport and classifier tests.

## `crates/egglsp/src/server_request.rs`

Expected changes:

- add atomic batch registration method;
- preflight capacity before mutation;
- add rollback/unchanged-state tests.

## `crates/egglsp/src/error.rs`

Possible changes:

- add shared initialization error conversion helpers;
- avoid broad public error API churn.

# Verification Commands

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test -p egglsp
cargo test --workspace
cargo clippy -p egglsp --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
```

For deadlock-prone tests, also run targeted repetitions where practical:

```bash
cargo test -p egglsp service::tests -- --test-threads=1
cargo test -p egglsp service::tests -- --test-threads=8
```

If a stress-style loop is added, keep it deterministic and bounded.

# Review Checklist

- [ ] Cold first-use reaches the initializer.
- [ ] Leader never waits on its own completion channel.
- [ ] Same-key concurrent calls initialize exactly once.
- [ ] Different keys initialize concurrently.
- [ ] Failure category/message are identical for leader and waiters.
- [ ] Cancelled leader cannot strand waiters.
- [ ] Attempt cleanup is guarded by attempt ID.
- [ ] Shutdown invalidates in-flight publication.
- [ ] No client is installed after shutdown begins.
- [ ] Newly created invalidated clients are disposed.
- [ ] Reader EOF marks transport failed.
- [ ] Request write failure marks transport failed.
- [ ] Notification write failure marks transport failed.
- [ ] Pending requests are drained exactly once.
- [ ] Registration batches are atomic.
- [ ] Fractional JSON-RPC error codes are rejected.
- [ ] No lock is held across unrelated I/O.
- [ ] Tests exercise actual asynchronous coordination.
- [ ] No external language server is required.

# Completion Criteria

This pass is complete when:

1. Cold LSP startup works.
2. The initialization coordinator cannot self-deadlock.
3. Same-key initialization is exactly-once per attempt.
4. All current callers share one success or failure result.
5. Failed/cancelled attempts clean up safely and allow retry.
6. Shutdown prevents all post-shutdown publication.
7. All terminal transport failures produce fail-fast client behavior.
8. Dynamic registration batches are atomic.
9. Error response classification requires integral codes.
10. Deterministic concurrency tests cover leader, waiters, failure, cancellation, retry, and shutdown races.
11. Documentation matches the implementation.
12. The repository is ready to proceed to the scripted stdio fake-LSP-server harness.
