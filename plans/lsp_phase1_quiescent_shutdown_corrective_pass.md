# LSP Phase 1 Quiescent Shutdown Corrective Pass

## Purpose

Correct the remaining lifecycle defects after:

```text
b6fa821b399f8329af0172b8e82b249adbc74a9f
ad0ffeb092e2b3dd22e7999ce73639b11a293b1e
```

The read-lock cleanup is complete and should be preserved. The remaining work is isolated to initialization-task completion tracking and shutdown coordination.

The current implementation has the right conceptual components—`CancellationToken`, `AbortHandle`, active task tracking, lifecycle generations, and shared shutdown notification—but the completion path is not tied to the actual spawned task. As a result, `shutdown_all()` can return while initialization work remains active.

This pass should make the shutdown contract true in implementation and tests:

> After `shutdown_all()` returns, no initialization task remains active, no ready client remains registered, lifecycle is `Stopped`, and concurrent shutdown callers have observed the same completed shutdown epoch.

## Scope

Primary file:

```text
crates/egglsp/src/service.rs
```

Documentation updates:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Possible small supporting changes:

```text
crates/egglsp/src/error.rs
```

Do not broaden this into the scripted stdio fake-server phase.

## Non-Goals

Do not implement:

- automatic LSP restart;
- pull diagnostics;
- incremental synchronization;
- multi-root workspaces;
- new semantic operations;
- new model-facing tools;
- direct `workspace/applyEdit` execution;
- process supervision outside initialization/shutdown cleanup.

## Preserve Existing Correctness

Do not regress:

- read-only service methods using `clients.read().await`;
- explicit single-flight leader/waiter election;
- shared leader/waiter completion fan-out;
- attempt-ID compare-and-remove cleanup;
- lifecycle-generation validation held through publication;
- deterministic document ownership;
- explicit disposal of unpublished clients;
- transport-failure propagation;
- timeout cancellation semantics;
- atomic dynamic registration handling;
- public-path coordinator tests.

## Current Defects

### 1. Completion receiver is disconnected from the spawned task

`finished_rx` is stored in `InitTaskControl`, but `finished_tx` is not owned by the task monitor on the normal registration path. The sender is dropped immediately.

Shutdown therefore observes:

```text
Ok(Err(RecvError))
```

instead of actual task completion. Because the code only aborts on timeout, a disconnected channel is incorrectly treated as completion.

### 2. Normal completion leaks `active_init_tasks` entries

The monitor returns immediately on `Ok(())` and does not remove the attempt from `active_init_tasks`.

Successful, failed, and lifecycle-invalidated attempts therefore leave stale task-control entries until shutdown drains the map.

### 3. Forced abort is not awaited

After `abort_handle.abort()`, shutdown only yields once. This does not prove that the task has been cancelled and fully unwound.

### 4. Global timeout silently abandons shutdown

`shutdown_all()` wraps `shutdown_inner()` in a timeout and discards the timeout result. If the timeout expires, lifecycle may remain `ShuttingDown`, maps may remain populated, and concurrent shutdown callers may never receive completion.

### 5. Per-task grace periods are sequential

Each task receives its own cancellation grace period. Shutdown duration therefore grows linearly with task count.

### 6. Concurrent shutdown completion can lose wakeups

A caller can observe `ShuttingDown`, release lifecycle state, and register with `Notify` after `notify_waiters()` has already occurred.

### 7. Lock ordering is inverted in task registration/cleanup

The documented order places `initializing` before `active_init_tasks`, but task registration and `finish_attempt()` acquire them in the reverse order.

### 8. Tests prove map draining, not task termination

Several tests assert that `active_init_tasks` is empty after shutdown, but shutdown itself drains the map before actual task completion is established.

## Required Invariants

1. Every spawned initialization task has one authoritative completion handle tied to its actual `JoinHandle`.
2. Completion is signaled only after the task has terminated or been aborted and awaited.
3. Every terminal path removes its active-task entry exactly once.
4. Shutdown cancellation grace applies to all tasks concurrently, not serially.
5. Forced abort is followed by awaiting actual task completion.
6. `shutdown_all()` never returns with lifecycle `ShuttingDown`.
7. Global timeout still performs forced finalization.
8. Concurrent shutdown callers cannot miss completion.
9. No nested lock acquisition violates documented ordering.
10. Tests observe task-body exit, not merely map emptiness.

# Phase 1 — Replace Detached Completion Receiver with Real Task Ownership

## Preferred Design

Store the actual monitor `JoinHandle` in `InitTaskControl`.

Recommended shape:

```rust
struct InitTaskControl {
    attempt_id: u64,
    cancellation: CancellationToken,
    join_handle: tokio::task::JoinHandle<InitTaskExit>,
}
```

Where:

```rust
enum InitTaskExit {
    Completed,
    Panicked(String),
    Cancelled(String),
}
```

A simpler `JoinHandle<()>` is acceptable if panic/cancellation cleanup remains handled by a wrapper task.

The important requirement is:

- shutdown owns something it can await directly;
- no standalone completion oneshot exists unless its sender is unquestionably owned by the monitor until after `JoinHandle` resolution.

## Wrapper Task Pattern

Preferred spawn pattern:

```rust
let join_handle = tokio::spawn(async move {
    let result = AssertUnwindSafe(run_initialization_attempt(...))
        .catch_unwind()
        .await;

    match result {
        Ok(()) => InitTaskExit::Completed,
        Err(payload) => {
            // finish matching attempt and notify callers
            InitTaskExit::Panicked(format_panic(payload))
        }
    }
});
```

If `catch_unwind` would add unnecessary complexity, retain a monitor task but store/await the monitor `JoinHandle`, not a disconnected receiver.

## Registration Race

Avoid spawning an untracked task.

Acceptable sequence:

1. Elect leader and insert init slot.
2. Spawn task.
3. Obtain abort handle.
4. Insert task control under the matching attempt ID.
5. If slot/lifecycle was invalidated between steps 2–4, immediately cancel/abort and await the task.

A stronger design is to use a single coordinator map containing slot and task state so registration is one atomic state transition.

## Acceptance Criteria

- Shutdown awaits a handle tied to the actual task.
- Dropping a completion sender cannot simulate task completion.
- No spawned task can remain permanently untracked.

# Phase 2 — Remove Active-Task Entries on Every Terminal Path

## Required Behavior

Every attempt must remove its `active_init_tasks` entry when it reaches:

```text
Published
Existing-client result
Lifecycle invalidation
Initialization failure
Cooperative cancellation
Forced abort
Panic
```

Do not rely on shutdown to drain stale completed entries.

## Recommended Ownership Model

The task wrapper should remove its own active-task record in a finalization guard or call one terminal helper.

Recommended helper:

```rust
async fn finalize_init_task(
    active_tasks: &ActiveTaskMap,
    attempt_id: u64,
) {
    active_tasks.lock().await.remove(&attempt_id);
}
```

If shutdown temporarily extracts controls from the map, ensure task-side cleanup tolerates a missing record.

## Completion Ordering

Suggested normal completion order:

1. Complete initialization attempt outcome.
2. Remove matching init slot and notify leader/waiters.
3. Remove task-control entry.
4. Return from task wrapper.

For panic/abort:

1. Remove matching init slot if still present.
2. Notify leader/waiters with terminal error.
3. Remove task-control entry.
4. Complete task join.

## Tests

Add assertions that `active_init_tasks` becomes empty shortly after:

- successful initialization;
- ordinary initialization failure;
- lifecycle invalidation;
- task panic;
- retry after failure.

These checks must occur before calling shutdown.

# Phase 3 — Await Cooperative Cancellation Concurrently

## Current Problem

Shutdown processes each task sequentially, granting every task a separate grace timeout.

## Required Algorithm

1. Snapshot or drain all active task controls.
2. Signal every cancellation token.
3. Await all task handles concurrently under one aggregate grace deadline.
4. Identify unfinished tasks.
5. Abort all unfinished tasks together.
6. Await all aborted tasks concurrently under one aggregate abort deadline.

Use `FuturesUnordered`, `join_all`, or equivalent.

Pseudo-flow:

```rust
for task in &tasks {
    task.cancellation.cancel();
}

let unfinished = await_until_deadline(tasks, grace_deadline).await;

for task in &unfinished {
    task.abort_handle.abort();
}

let still_unfinished = await_until_deadline(unfinished, abort_deadline).await;
```

The helper should retain handles across the first timeout so unfinished tasks can be aborted and awaited.

## Join Result Handling

Handle:

- normal completion;
- cancellation (`JoinError::is_cancelled()`);
- panic (`JoinError::is_panic()`);
- timeout after abort.

A timeout after abort should be logged as a severe lifecycle failure, but final shutdown cleanup must still continue.

## Acceptance Criteria

- Shutdown grace duration is independent of task count.
- Every aborted task is actually awaited.
- No `yield_now()` is used as a substitute for task completion.

# Phase 4 — Make the Global Shutdown Deadline Finalizing, Not Abandoning

## Current Problem

The outer timeout drops `shutdown_inner()` and ignores the result.

## Required Design

Prefer an absolute shutdown deadline propagated through each stage:

```rust
let deadline = Instant::now() + SHUTDOWN_GLOBAL_TIMEOUT;
```

Each stage receives remaining time:

```text
cancel init tasks
abort remaining init tasks
shutdown ready clients
finalize lifecycle
notify waiters
```

Do not wrap the entire state machine in a timeout that can cancel finalization.

## Forced Finalization

If the deadline expires:

1. Abort every remaining initialization task.
2. Drop/clear remaining task controls.
3. Drain ready clients from the service map.
4. Attempt nonblocking/bounded client termination.
5. Clear document ownership and init slots.
6. Set lifecycle to `Stopped`.
7. Publish shutdown completion.
8. Log that shutdown required forced finalization.

The service state must always be finalized even if some external process refuses graceful termination.

## API Result

Consider changing:

```rust
pub async fn shutdown_all(&self)
```

to:

```rust
pub async fn shutdown_all(&self) -> Result<(), LspError>
```

only if callers can absorb the change without broad churn.

Otherwise, retain `()` but log structured timeout/finalization outcomes and guarantee the postconditions.

## Acceptance Criteria

- `shutdown_all()` never returns while lifecycle is `ShuttingDown`.
- Concurrent callers are always released.
- Deadline expiry cannot strand service state.

# Phase 5 — Replace Lost-Wakeup-Prone `Notify` Coordination

## Preferred Design

Use `tokio::sync::watch` for retained lifecycle/shutdown state.

Example:

```rust
struct LspService {
    lifecycle: Arc<RwLock<LifecycleState>>,
    lifecycle_tx: watch::Sender<LifecycleState>,
}
```

Or replace the separate RwLock with a watch channel plus a short mutation mutex if practical.

A concurrent shutdown caller should:

1. subscribe before checking state;
2. inspect current state;
3. if `Stopped`, return;
4. if `ShuttingDown`, await changes until `Stopped`;
5. if `Running`, become shutdown owner.

## Acceptable Notify Pattern

If retaining `Notify`, use a loop that creates the notification future before state inspection:

```rust
loop {
    let notified = shutdown_complete.notified();
    match lifecycle.read().await.phase {
        Stopped => return,
        ShuttingDown => notified.await,
        Running => ...,
    }
}
```

The loop must re-check state after wakeup.

`watch` is preferred because it naturally retains the latest state.

## Tests

Create a deterministic lost-wakeup test:

1. Secondary caller subscribes/checks at the exact completion boundary.
2. Primary caller transitions to `Stopped` and publishes completion.
3. Secondary caller must return without waiting for timeout.

Do not use sleeps to establish ordering.

# Phase 6 — Fix Lock Ordering and Remove Nested Lock Acquisition

## Current Inversion

Task registration holds `active_init_tasks` and then awaits `initializing`.

`finish_attempt()` also acquires active task state before taking the init slot.

## Required Rule

Prefer no nested coordinator locks.

Task registration:

```rust
let slot_still_exists = {
    let init = initializing.lock().await;
    init.get(&key).is_some_and(|slot| slot.attempt_id == attempt_id)
};

if slot_still_exists {
    active_init_tasks.lock().await.insert(...);
} else {
    abort_and_await(task).await;
}
```

Because state may change between checks, use attempt IDs and lifecycle validation to make stale registration harmless.

A better design is a single coordinator map:

```rust
HashMap<String, InitAttemptState>
```

containing:

```text
attempt ID
leader/waiters
task cancellation
task handle
```

This eliminates cross-map races and lock ordering entirely. Use this only if the refactor remains localized.

## Acceptance Criteria

- No path holds `active_init_tasks` while awaiting `initializing`.
- No path holds `initializing` while awaiting task/client I/O.
- Documentation matches actual lock order.

# Phase 7 — Correct Ready-Client Shutdown Handling

## Error Handling

Handle all timeout result layers:

```rust
match timeout(remaining, client.shutdown()).await {
    Ok(Ok(())) => {}
    Ok(Err(err)) => warn!(... graceful shutdown error ...),
    Err(_) => warn!(... shutdown timeout ...),
}
```

## Concurrency

Shutdown ready clients concurrently under one aggregate deadline where practical.

Do not grant a full independent two-second timeout sequentially to every client.

Use one of:

- `FuturesUnordered` with per-client deadline capped by the global deadline;
- `join_all` over bounded per-client timeout futures.

## Process Termination

After graceful shutdown failure/timeout, ensure existing child-process drop/kill behavior is sufficient. If not, add a narrow explicit terminate helper.

# Phase 8 — Make Test Factories Exercise Cancellation Correctly

## Cooperative Test Factory

Wrap the injected test factory with cancellation:

```rust
tokio::select! {
    result = init_fn(server, &root) => result,
    _ = cancellation.cancelled() => Err(InitializationCancelled(...)),
}
```

This ensures the blocked-factory test actually proves cooperative cancellation.

## Uncooperative Test Factory

Retain a separate factory that intentionally ignores cancellation and only terminates when aborted.

Instrument it with:

```text
entered counter/notification
exited counter/notification
drop guard
```

After shutdown returns, assert `exited == entered`.

Do not release the uncooperative factory after shutdown merely to let it clean up; shutdown itself must establish termination.

# Phase 9 — Strengthen Quiescence Tests

## Required Test: Normal Completion Removes Active Entry

1. Start successful public-path initialization.
2. Await completion.
3. Assert `active_init_tasks` is empty before shutdown.

Repeat for ordinary initialization failure.

## Required Test: Cooperative Cancellation Exits Task Body

1. Start blocked cancellation-aware factory.
2. Record task-body entry.
3. Call shutdown.
4. Assert task-body exit/drop guard fired before shutdown returned.
5. Assert leader/waiters received cancellation.

## Required Test: Forced Abort Is Awaited

1. Start cancellation-uncooperative factory with a drop guard.
2. Call shutdown.
3. Assert the task future was dropped/aborted before shutdown returned.
4. Do not externally release the factory afterward.

## Required Test: Many Tasks Share One Grace Period

1. Start multiple independent blocked init tasks.
2. Call shutdown.
3. Assert elapsed time is approximately one grace period plus bounded abort/finalization, not `N × grace`.

Use a generous deterministic upper bound rather than brittle exact timing.

## Required Test: Global Deadline Finalizes State

Inject a task/client that cannot complete gracefully.

Assert after shutdown returns:

```text
lifecycle == Stopped
clients empty
initializing empty
active_init_tasks empty
document_owners empty
concurrent shutdown waiter released
```

## Required Test: Lost Wakeup Boundary

Use barriers/watch channels to place a secondary caller at the transition boundary and prove it cannot miss completion.

## Required Test: Client Shutdown Error Logging Path

Use a test stub whose `shutdown()` returns an error and assert shutdown still reaches `Stopped` and removes the client.

## Test Timeouts

Wrap all coordinator tests in bounded `tokio::time::timeout`.

Avoid sleeps except for explicit duration-bound assertions. Prefer `Barrier`, `Notify`, `watch`, and drop guards.

# Phase 10 — Documentation Corrections

Update documentation to match the corrected implementation.

Remove claims that:

- a disconnected oneshot proves task completion;
- one `yield_now()` waits for abort;
- the current outer global timeout guarantees finalization;
- `Notify` alone provides retained shutdown state.

Document:

- authoritative task `JoinHandle` ownership;
- concurrent aggregate cancellation grace;
- forced abort and awaited termination;
- absolute deadline and forced finalization;
- race-free concurrent shutdown waiting;
- active-task entry removal on every terminal path;
- ready-client shutdown error handling.

# Suggested Implementation Order

1. Replace `finished` oneshot with actual task/monitor `JoinHandle` ownership.
2. Remove active-task entries on normal completion.
3. Add task-body exit instrumentation tests.
4. Implement concurrent cancellation grace and awaited abort.
5. Replace shutdown `Notify` coordination with watch or race-free loop.
6. Replace outer timeout cancellation with internal absolute deadline/finalization.
7. Fix lock-order inversions.
8. Make ready-client shutdown concurrent and error-aware.
9. Correct test-factory cancellation behavior.
10. Update docs and run full verification.

# File-Level Guidance

## `crates/egglsp/src/service.rs`

Expected changes:

- redesign `InitTaskControl` around a real `JoinHandle` or monitor handle;
- remove disconnected `finished` channel;
- remove stale task entries on every terminal path;
- add aggregate cancellation/abort helpers;
- guarantee lifecycle finalization under deadline expiry;
- replace lost-wakeup-prone shutdown signaling;
- eliminate lock-order inversions;
- strengthen tests.

## `crates/egglsp/src/error.rs`

Optional:

- add a narrow shutdown-timeout/finalization error if `shutdown_all()` returns `Result`.

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

Run service tests with both scheduler modes:

```bash
cargo test -p egglsp service::tests -- --test-threads=1
cargo test -p egglsp service::tests -- --test-threads=8
```

Run the quiescence tests repeatedly in a bounded loop if practical.

# Review Checklist

- [ ] Task completion is tied to the actual `JoinHandle`.
- [ ] No disconnected completion receiver remains.
- [ ] Active-task entries are removed on normal success and failure.
- [ ] Cooperative cancellation is awaited concurrently.
- [ ] Forced abort is followed by actual task join completion.
- [ ] Grace duration does not scale linearly with task count.
- [ ] Global deadline cannot leave lifecycle `ShuttingDown`.
- [ ] Concurrent shutdown callers cannot miss completion.
- [ ] No nested `active_init_tasks -> initializing` lock acquisition remains.
- [ ] Test factory path participates in cooperative cancellation.
- [ ] Uncooperative task test proves task-body exit before return.
- [ ] Ready-client shutdown handles both timeout and inner error.
- [ ] Service maps are empty after shutdown.
- [ ] Read-lock cleanup remains intact.
- [ ] Documentation matches actual behavior.

# Completion Criteria

This corrective pass is complete when:

1. Every initialization task is tracked by an authoritative completion handle.
2. `active_init_tasks` is empty after each attempt terminates, without requiring shutdown.
3. Shutdown cancels all tasks concurrently.
4. Uncooperative tasks are aborted and actually awaited.
5. Shutdown always transitions lifecycle to `Stopped`, even under deadline expiry.
6. Concurrent shutdown callers always observe final completion without lost wakeups.
7. No initialization task body remains active after `shutdown_all()` returns.
8. Ready clients are drained with correct timeout/error handling.
9. Lock ordering is consistent and free of nested inversion.
10. Tests prove task termination rather than only map cleanup.
11. Existing Phase 1 protocol, coordinator, and read-lock tests remain green.
12. Phase 1 can be closed and Phase 2 can proceed to the scripted stdio fake-server harness.
