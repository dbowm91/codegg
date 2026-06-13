# LSP Phase 1 Coordinator Race Corrections

## Purpose

Finish the remaining Phase 1 correctness work after:

```text
1ffe100b3e9047e309960d5797932a45c533ba2f
```

The previous pass removed the original self-wait deadlock and added substantial protocol, lifecycle, and transport hardening. Two release-blocking coordinator races remain:

1. leadership is still inferred from an empty waiter list, allowing multiple same-key callers to become leaders;
2. lifecycle validation and client publication are separated, allowing shutdown to race with publication.

Several related completion and testing issues also remain. This pass should correct those items without expanding the LSP feature surface.

## Scope

Primary files:

```text
crates/egglsp/src/service.rs
crates/egglsp/src/client.rs
crates/egglsp/src/error.rs
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Potential test-only support may remain private to `service.rs` or be extracted into a small `#[cfg(test)]` module.

## Non-Goals

Do not implement:

- the scripted stdio child-process fake LSP server;
- server restart supervision;
- pull diagnostics;
- incremental synchronization;
- multi-root workspaces;
- new semantic/model-facing LSP operations;
- direct `workspace/applyEdit` execution;
- broader service lifecycle restart semantics.

## Preserve Existing Correct Behavior

Do not regress:

- signed/string JSON-RPC IDs;
- strict ID-only response rejection;
- integral JSON-RPC error-code validation;
- application-level `workspace/applyEdit` refusal;
- full registration/unregistration array handling;
- atomic dynamic registration capacity checks;
- deterministic URI-to-client ownership;
- idempotent close/save behavior;
- global client-map lock release before normal client I/O;
- shared serialized writer;
- centralized transport failure for stdout EOF and ordinary request/notification writes;
- timeout cancellation behavior;
- lifecycle generation tracking;
- `SharedInitError` category/message preservation for waiter paths.

## Required Invariants

1. Exactly one leader exists per `{project_root}:{server_id}` initialization attempt.
2. Leader identity is explicit state, never inferred from waiter count.
3. All callers for one attempt receive the same completion result.
4. The leader and all waiters receive equivalent success or failure information.
5. Client publication is atomic with lifecycle-generation validation.
6. Shutdown either observes and drains a published client or invalidates publication before insertion.
7. A successful client that cannot be published is explicitly disposed.
8. Integrated concurrency tests exercise `get_or_create_client()` rather than manually bypassing election.
9. Timeout cancellation write failure marks the transport failed without replacing the current timeout result.
10. No lock is held across process shutdown or other client I/O.

# Phase 1 — Encode Leadership Explicitly

## Current Defect

The slot currently stores:

```rust
enum InitSlotState {
    Starting { waiters: Vec<...> },
    Ready(Arc<LspClient>),
}
```

A caller becomes leader when `waiters.is_empty()`. After the first caller elects itself leader, the slot remains `Starting { waiters: [] }`. A second caller arriving before any waiter is registered sees the same state and may also become leader.

## Required State Model

Use explicit running state. Recommended shape:

```rust
struct InitSlot {
    attempt_id: u64,
    state: InitSlotState,
}

enum InitSlotState {
    Running {
        waiters: Vec<InitCompletionSender>,
    },
    Ready(Arc<LspClient>),
}
```

Creation itself establishes the leader. No later caller may infer leadership from slot contents.

A cleaner alternative is to remove `Ready` from the initialization map entirely and keep ready clients only in `clients`:

```rust
enum InitSlotState {
    Running {
        waiters: Vec<InitCompletionSender>,
    },
}
```

The initialization map then represents only active attempts. This is preferred if it simplifies cleanup and eliminates duplicate authoritative state.

## Atomic Election

Perform slot lookup, insertion, and role election in one initialization-map write-lock scope:

```text
clients contains key:
    return Ready from clients map

initializing contains key:
    append completion sender
    return Waiter

otherwise:
    allocate attempt ID
    insert Running slot
    return Leader
```

Do not:

- create a slot under one lock and infer leadership later under the slot lock;
- use `waiters.is_empty()` as a leader marker;
- allow a `Starting`/`Running` slot without an already elected leader.

Suggested result:

```rust
enum InitRole {
    Leader {
        attempt_id: u64,
        completion: InitCompletionReceiver,
    },
    Waiter {
        completion: InitCompletionReceiver,
    },
    Ready(Arc<LspClient>),
}
```

Both leader and waiter should receive completion receivers. The distinction controls who spawns the attempt, not how the result is consumed.

## Acceptance Criteria

- Twenty concurrent calls for the same key invoke the factory once.
- A second caller arriving immediately after slot creation always becomes a waiter.
- No state transition depends on waiter vector length.

# Phase 2 — Use One Completion Path for Leader and Waiters

## Current Defect

The leader awaits the spawned task, inspects the clients map/slot, and returns generic `InitializationCancelled("init failed")` when no client was published. Waiters receive the actual `SharedInitError`.

## Required Design

Create a completion channel for every caller, including the elected leader.

Recommended aliases:

```rust
type InitResult = Result<Arc<LspClient>, SharedInitError>;
type InitCompletionSender = oneshot::Sender<InitResult>;
type InitCompletionReceiver = oneshot::Receiver<InitResult>;
```

Election behavior:

- new slot: create leader completion channel, store sender in the slot, return leader receiver;
- existing running slot: create waiter channel, append sender, return waiter receiver.

The initialization task publishes the same logical result to every sender. A cloned `Arc<LspClient>` or cloned `SharedInitError` is delivered to each caller.

The leader should:

1. spawn the owned task;
2. await its completion receiver;
3. return the received result;
4. not infer result from the clients map;
5. not ignore task panic silently.

The spawned task handle may be detached after spawning because the completion channel is authoritative. If retained, a join monitor should convert panic/cancellation into a shared error and clean the matching slot.

## Panic Handling

Wrap or monitor the spawned initialization task so panic cannot strand callers:

```text
task panic / unexpected cancellation
    -> compare-and-remove matching attempt
    -> notify all completion senders with Cancelled/Internal error
```

A small supervisor task around `run_initialization_attempt()` is acceptable.

## Acceptance Criteria

- Leader and waiters receive the same error kind and message.
- Success returns the same logical client identity to all callers.
- A task panic resolves all waiting callers and cleans the slot.

# Phase 3 — Make Lifecycle Validation and Publication Atomic

## Current Defect

The initializer checks lifecycle state/generation, releases the lifecycle lock, and later inserts into the client map. Shutdown can transition lifecycle and drain clients between validation and insertion.

## Required Publication Sequence

Follow the documented lock ordering:

```text
lifecycle -> clients
```

On successful initialization:

1. Acquire lifecycle read guard.
2. Verify:
   - phase is `Running`;
   - generation matches the captured generation.
3. While retaining the lifecycle read guard, acquire clients write guard.
4. Insert the client.
5. Release clients guard.
6. Release lifecycle guard.
7. Complete slot cleanup and notify callers.

This gives two legal outcomes:

- publication obtains lifecycle read first, inserts client, then shutdown obtains lifecycle write and drains it;
- shutdown obtains lifecycle write first, increments generation, and publication later fails validation.

There must be no interval where validation has passed but shutdown can drain before insertion.

## Avoid Locking I/O

Do not hold lifecycle, clients, initialization-map, or slot locks while calling:

```rust
client.shutdown().await
```

When publication is invalidated:

1. collect completion senders;
2. remove the matching slot;
3. release all coordinator locks;
4. explicitly dispose the client;
5. notify callers with lifecycle cancellation.

Whether disposal occurs before or after caller notification should be documented. Prefer disposal first if bounded; otherwise ensure process termination is guaranteed and notify promptly.

## Slot Missing After Successful Initialization

If the initialization task completes successfully but its slot no longer exists, treat it as invalidated publication:

- explicitly shut down/terminate the client;
- do not simply return and rely on drop semantics;
- do not publish to `clients`;
- log attempt ID, key, and lifecycle state at debug level.

## Acceptance Criteria

- No client exists in `clients` after `shutdown_all()` returns.
- Successful initialization racing shutdown is either published then drained, or never published and explicitly disposed.
- No process I/O occurs under coordinator locks.

# Phase 4 — Simplify Attempt Ownership and Cleanup

## Attempt Identity

Retain monotonically increasing attempt IDs. Every completion path must compare the current slot attempt ID before removal.

## Single Cleanup Helper

Introduce a helper that atomically detaches the matching attempt and returns its senders:

```rust
async fn take_attempt(
    initializing: &InitMap,
    key: &str,
    attempt_id: u64,
) -> Option<Vec<InitCompletionSender>>;
```

Behavior:

1. Look up slot under map lock.
2. Confirm attempt ID matches.
3. Remove map entry.
4. Extract sender vector.
5. Return senders after releasing locks.

Avoid acquiring an initialization-map write lock and then awaiting a slot mutex if possible. Prefer storing slot state directly under one map mutex/RwLock or otherwise restructure to prevent nested async lock complexity.

A simpler design is acceptable:

```rust
type InitMap = Arc<Mutex<HashMap<String, InitSlot>>>;
```

Since operations on this map are short and no initialization I/O is performed while holding it, a single mutex may be easier to reason about than a map RwLock plus per-slot mutexes.

## Retry Safety

Stale task cleanup must not remove a newer attempt:

```text
old attempt A fails late
new attempt B already installed
A cleanup compares attempt_id and leaves B untouched
```

## Acceptance Criteria

- All success, error, panic, lifecycle invalidation, and channel-cancellation paths use one cleanup mechanism.
- No stale attempt can remove a newer slot.

# Phase 5 — Add Integrated Public-Path Concurrency Tests

The current tests often pre-create slots and call `run_initialization_attempt()` directly. Those tests may remain as unit tests, but they cannot serve as proof of coordinator correctness.

## Test Factory Requirements

Add a controllable factory with:

- invocation counter;
- “entered” notification;
- release barrier;
- configurable success/failure result;
- optional fake-client disposal observation.

Use `Notify`, `Barrier`, oneshot channels, or semaphores rather than timing sleeps.

If constructing `Arc<LspClient>` without a process is difficult, introduce an internal generic/test client handle abstraction or an injected publication/disposal callback. Do not weaken production types solely for test convenience.

## Required Test: Real Same-Key Single Flight

Call the public path concurrently:

```rust
for _ in 0..20 {
    tokio::spawn(service.get_or_create_client(same_file))
}
```

Test sequence:

1. First factory call enters and blocks.
2. Wait until all or a known majority of caller tasks are pending.
3. Assert factory invocation count remains 1.
4. Release the factory.
5. Assert every caller receives equivalent result.
6. Assert initialization map is empty.

Do not manually pre-create the slot.

## Required Test: Arrival Before First Waiter

Create a deterministic hook/barrier immediately after leader election but before the leader task is spawned. Start a second caller during that window. Assert:

- second caller is waiter;
- factory invoked once.

This directly targets the current defect.

## Required Test: Shared Failure Fidelity

Use 20 public-path callers with a blocked failing factory. Assert every caller, including the leader, receives identical:

```text
SharedInitErrorKind / mapped LspError variant
message
```

## Required Test: Retry After Failure

After all callers observe failure and the slot is removed:

- configure next factory attempt to succeed or fail differently;
- call public path again;
- assert invocation count increments exactly once;
- assert stale cleanup from first attempt does not affect second.

## Required Test: Successful Shutdown Race

The current shutdown race uses a failing factory. Replace/add a success-capable fake.

Sequence:

1. Public-path leader starts and factory blocks.
2. Additional same-key waiters join.
3. Shutdown transitions lifecycle/generation.
4. Factory returns a successful fake client.
5. Assert:
   - fake client disposal callback observed;
   - clients map empty;
   - initialization map empty;
   - document ownership empty;
   - leader and waiters receive lifecycle cancellation;
   - service phase is `Stopped`.

## Required Test: Publish Before Shutdown

Exercise the opposite ordering:

1. Hold shutdown before lifecycle write acquisition if a test hook is available.
2. Allow publication to complete under lifecycle read guard.
3. Start shutdown.
4. Assert shutdown drains/disposes the published client.

## Required Test: Task Panic

Factory or wrapper panics. Assert:

- all public-path callers resolve with a stable cancellation/internal error;
- slot is removed;
- retry works.

## Test Timeouts

Every concurrency test should use bounded `tokio::time::timeout` to turn deadlocks into clear failures. Avoid tests whose expected success condition is merely “the timeout fired.”

# Phase 6 — Mark Transport Failed on Cancellation Write Failure

## Current Gap

When a timed-out request attempts to send `$/cancelRequest`, a writer error is logged but does not call `fail_transport()`.

## Required Behavior

On cancellation write failure:

1. preserve the current caller’s `RequestTimeout` result;
2. call `fail_transport()` with the writer error;
3. drain any other pending requests;
4. ensure subsequent operations fail fast with `WriterClosed`.

Pseudo-flow:

```rust
if let Err(e) = writer.send_notification_message("$/cancelRequest", params).await {
    fail_transport(&transport_state, &pending, format!(...)).await;
}
return Err(RequestTimeout(...));
```

## Tests

- current timed-out request still returns `RequestTimeout`;
- another pending request is failed promptly;
- transport becomes `Failed`;
- later notification/request returns `WriterClosed` immediately.

# Phase 7 — Explicitly Dispose Unpublishable Clients

Any successfully initialized client must be disposed when:

- lifecycle generation changed;
- lifecycle phase is not running;
- slot disappeared;
- attempt ID no longer matches;
- publication loses to another already-published client unexpectedly.

Introduce a helper:

```rust
async fn dispose_unpublished_client(client: Arc<LspClient>, reason: &str)
```

It should attempt graceful shutdown, then rely on existing process-drop/kill-on-drop behavior if graceful shutdown fails. Do not block indefinitely; use a bounded timeout if necessary.

Logging should include key, attempt ID, and reason, but not sensitive payloads.

# Phase 8 — Review Ready-State Duplication

The current code stores initialized clients in `clients` and can also retain `InitSlotState::Ready`. This creates two sources of truth.

Preferred outcome:

- `clients` is authoritative for ready clients;
- `initializing` contains only active attempts;
- success inserts into `clients`, removes the attempt slot, then notifies callers.

If `Ready` is retained, document why and ensure:

- no caller re-inserts stale clients after shutdown;
- no ready slot survives success cleanup;
- lifecycle is revalidated before any insertion from a ready slot.

Removing `Ready` is likely simpler.

# Phase 9 — Lock-Order and Await Audit

Audit these paths:

```text
role election
attempt completion
success publication
failure cleanup
shutdown invalidation
unpublished client disposal
leader/waiter completion
```

Rules:

- no client/process I/O while lifecycle guard is held except client-map insertion/removal;
- no client/process I/O while initialization map/slot locks are held;
- no slot lock while awaiting lifecycle or clients if avoidable;
- lifecycle read may be held through clients insertion because this is the required atomic publication boundary;
- completion sends occur after coordinator locks are released;
- task panic cleanup cannot deadlock with shutdown.

Update lock-order documentation to describe the lifecycle-read-through-publication exception explicitly.

# Phase 10 — Documentation Corrections

Update:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Document only after implementation:

- explicit leader state rather than waiter-count inference;
- one completion channel model for leader and waiters;
- atomic lifecycle validation/publication;
- integrated public-path concurrency coverage;
- explicit disposal of unpublishable clients;
- cancellation-write transport failure behavior.

Remove or correct claims that the current coordinator is already exactly-once under concurrent public-path calls.

# Suggested Implementation Order

1. Redesign slot state so leader election is atomic and explicit.
2. Give leader and waiters the same completion receiver model.
3. Add a public-path two-caller race test immediately.
4. Add public-path 20-caller single-flight and shared-failure tests.
5. Make lifecycle validation atomic with client insertion.
6. Add successful-client shutdown race and publish-before-shutdown tests.
7. Centralize attempt cleanup and remove redundant `Ready` state if practical.
8. Explicitly dispose all unpublishable successful clients.
9. Mark transport failed on cancellation write failure.
10. Audit locks, update docs, and run full verification.

# File-Level Guidance

## `crates/egglsp/src/service.rs`

Expected changes:

- replace waiter-count leader inference;
- perform map insertion and leader election atomically;
- add completion receiver for leader;
- return shared result directly instead of inspecting maps;
- synchronize lifecycle read validation with clients insertion;
- centralize compare-and-remove attempt cleanup;
- explicitly dispose unpublishable clients;
- replace bypass-style concurrency tests with public-path tests;
- retain test factory but make it capable of deterministic success/disposal observation.

## `crates/egglsp/src/client.rs`

Expected changes:

- call `fail_transport()` when timeout cancellation write fails;
- add regression tests for timeout result preservation and transport failure.

## `crates/egglsp/src/error.rs`

Likely minimal changes. Ensure leader completion maps `SharedInitError` identically to waiter completion. Add a stable internal/panic error category only if needed.

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

Run coordinator tests under both single- and multi-thread scheduling:

```bash
cargo test -p egglsp service::tests -- --test-threads=1
cargo test -p egglsp service::tests -- --test-threads=8
```

Where supported, repeat the exact same-key race test in a bounded loop to increase scheduling coverage without introducing nondeterministic sleeps.

# Review Checklist

- [ ] Leadership is explicit state.
- [ ] Slot creation atomically elects one leader.
- [ ] Empty waiter list has no leadership meaning.
- [ ] Leader receives completion through the same result mechanism as waiters.
- [ ] Leader and waiters receive identical failure category/message.
- [ ] Same-key public-path concurrency invokes factory once.
- [ ] Second caller in the pre-spawn window becomes waiter.
- [ ] Task panic resolves all callers and cleans the attempt.
- [ ] Lifecycle validation is held through client insertion.
- [ ] Shutdown cannot drain before a validated publication inserts.
- [ ] Successful invalidated clients are explicitly disposed.
- [ ] Missing slot after success triggers disposal.
- [ ] Attempt cleanup cannot remove newer attempts.
- [ ] Public-path shutdown race uses a successful fake client.
- [ ] Cancellation write failure marks transport failed.
- [ ] Current timeout still returns `RequestTimeout`.
- [ ] No process I/O occurs under coordinator locks.
- [ ] Documentation reflects actual behavior.

# Completion Criteria

This pass is complete when:

1. Same-key concurrent cold start launches exactly one initialization attempt.
2. Leader election cannot be duplicated by an early second caller.
3. Leader and waiters consume the same completion result.
4. Shared failures preserve the same category and message for every caller.
5. Client publication is atomic with lifecycle-generation validation.
6. Shutdown cannot leave a post-shutdown client in the map.
7. Successful but unpublishable clients are explicitly disposed.
8. Integrated public-path tests cover concurrency, failure, panic, retry, and shutdown races.
9. Cancellation writer failure transitions transport state while preserving the current timeout error.
10. No coordinator lock is held across client/process I/O.
11. All `egglsp` tests and lint checks pass.
12. Phase 1 is ready to close and Phase 2 can proceed with the scripted stdio fake-server harness.
