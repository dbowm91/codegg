# LSP Phase 1 Authoritative Task Completion Finalization

## Purpose

Complete the remaining quiescent-shutdown work after:

```text
f0bd6b4935a3f0ae6ef22f3cb4303ca4bd68fe75
```

The current implementation has corrected several important areas:

- client-map read/write lock discipline;
- explicit leader/waiter single-flight initialization;
- lifecycle-generation validation through publication;
- watch-based shutdown state propagation;
- absolute shutdown deadline and forced state finalization;
- concurrent ready-client shutdown;
- panic capture around initialization;
- explicit disposal of unpublished clients.

One central lifecycle defect remains: the shutdown join helper wraps each authoritative initialization `JoinHandle` inside a forwarding task, then aborts the forwarding task on timeout. Dropping the inner `JoinHandle` detaches the actual initialization task. Shutdown subsequently signals the detached task through an `AbortHandle`, but no longer owns a completion primitive that proves the real task has terminated.

This pass should make the following contract true:

> After `shutdown_all()` returns, every initialization task spawned by the service has either completed normally or been aborted, and the service has observed that task’s actual terminal completion.

This is intended to be the final Phase 1 lifecycle correction before moving to the scripted stdio fake-server harness.

## Scope

Primary implementation file:

```text
crates/egglsp/src/service.rs
```

Possible supporting changes:

```text
crates/egglsp/src/error.rs
crates/egglsp/Cargo.toml
```

Documentation:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

## Non-Goals

Do not implement:

- the Phase 2 scripted stdio fake LSP server;
- automatic language-server restart;
- pull diagnostics;
- incremental text synchronization;
- multi-root workspaces;
- new model-facing LSP operations;
- direct `workspace/applyEdit` execution;
- broader service/process supervision unrelated to initialization shutdown.

## Preserve Existing Correctness

Do not regress:

- `clients.read().await` for non-mutating operations;
- explicit one-leader-per-attempt election;
- shared leader/waiter result fan-out;
- attempt-ID compare-and-remove slot cleanup;
- lifecycle generation held through client publication;
- watch-based `ShuttingDown -> Stopped` observation;
- absolute shutdown deadline and unconditional transition to `Stopped`;
- concurrent ready-client drain;
- deterministic URI ownership;
- timeout cancellation and transport-failure propagation;
- protocol-peer request handling;
- atomic dynamic registration batches.

## Current Defects

### 1. Forwarder tasks detach authoritative initialization handles

`drain_joins_with_deadline()` currently moves each initialization `JoinHandle` into a new task:

```rust
set.spawn(async move {
    let _ = h.await;
});
```

When the grace period expires, `JoinSet::abort_all()` aborts those forwarding tasks. The underlying `JoinHandle` is dropped, which detaches the real initialization task.

### 2. Abort is not followed by authoritative completion observation

The shutdown path retains `AbortHandle`s and calls `abort()`, but the real `JoinHandle`s have already been moved into/dropped with the forwarders. Shutdown cannot await task termination after abort.

### 3. `drain_joins_with_deadline()` does not return unfinished handles

The helper signature claims to return unfinished `JoinHandle`s, but it always returns an empty vector. The caller cannot distinguish completed tasks from stragglers.

### 4. Active task removal remains best-effort

`ActiveTaskGuard::drop()` uses `try_lock()`. If the map is contended, the active-task entry is not removed and no retry occurs.

### 5. Spawn-before-registration race remains

The wrapper task starts before its `InitTaskControl` is inserted. A fast task can complete before registration, causing cleanup to run before the entry exists, after which the completed handle is inserted as a stale active record.

### 6. Registration path still nests locks in reverse order

The path currently acquires `active_init_tasks`, then awaits `initializing`, contrary to the documented lock ordering.

### 7. Test factories still bypass cancellation

The injected test factory path is returned directly and is not raced against `cancellation.cancelled()`. Tests labeled cooperative therefore exercise forced abort rather than cooperative cancellation.

### 8. Task-exit counters are not RAII-based

Counters incremented after an awaited blocking point do not run when a task is aborted. They do not prove that the future body was dropped before shutdown returned.

## Required Invariants

1. Shutdown retains an authoritative completion primitive for every real initialization task until that task terminates.
2. No forwarding task may own and then drop the only real initialization `JoinHandle`.
3. Every task that is aborted is subsequently awaited to completion.
4. Active-task registration is complete before the wrapper can finish.
5. Active-task removal is explicit on normal completion and guaranteed on panic/abort/drop fallback.
6. No path nests `active_init_tasks` and `initializing` in the wrong order.
7. Cooperative test factories are actually cancellation-aware.
8. Tests prove future drop/termination using RAII guards.
9. `shutdown_all()` transitions to `Stopped` only after completion has been observed or the global deadline forces an explicitly documented terminal fallback.
10. Empty bookkeeping maps are a consequence of task completion, not a substitute for it.

# Phase 1 — Replace Forwarder-Based Join Draining

## Preferred Completion Model

Use a completion channel whose sender is owned by the real wrapper task until its final line.

Recommended structures:

```rust
type InitTaskCompletionRx = tokio::sync::oneshot::Receiver<InitTaskExit>;

enum InitTaskExit {
    Completed,
    Panicked(String),
    Cancelled,
}

struct InitTaskControl {
    attempt_id: u64,
    cancellation: CancellationToken,
    abort_handle: tokio::task::AbortHandle,
    completion: InitTaskCompletionRx,
}
```

The real wrapper task owns:

```rust
completion_tx: oneshot::Sender<InitTaskExit>
```

and sends exactly once after all attempt cleanup is complete.

This is not the previous broken completion design. The critical difference is that the sender must be moved into the real wrapper task and retained until the wrapper is actually exiting.

## Alternative

Retain actual `JoinHandle`s in a data structure that allows repeated polling without transferring ownership to forwarding tasks.

For example, use:

```rust
FuturesUnordered<JoinHandle<()>>
```

constructed directly from the authoritative handles, while preserving unresolved handles across the grace timeout.

Do not introduce an intermediary task whose cancellation drops the only authoritative handle.

## Remove `drain_joins_with_deadline()`

Delete or replace the current helper. No helper should claim to return unfinished handles while always returning an empty vector.

## Acceptance Criteria

- The shutdown owner retains authoritative completion for every task.
- No detached initialization task can survive because its `JoinHandle` was dropped by a forwarder.

# Phase 2 — Make Task Registration Atomic with Task Startup

## Current Race

The task is spawned before its control record is installed. A fast task can complete before registration.

## Recommended Start-Barrier Design

Create a registration barrier:

```rust
let (start_tx, start_rx) = oneshot::channel::<()>();
let (completion_tx, completion_rx) = oneshot::channel::<InitTaskExit>();
```

Spawn the wrapper with `start_rx`:

```rust
let handle = tokio::spawn(run_init_task_wrapper(
    ...,
    start_rx,
    completion_tx,
));
```

The wrapper begins with:

```rust
if start_rx.await.is_err() {
    return;
}
```

Then:

1. Create task and obtain `AbortHandle`.
2. Insert `InitTaskControl` into `active_init_tasks`.
3. Revalidate attempt ID/lifecycle without nested locks.
4. Send `start_tx` only after registration is complete.
5. If registration becomes invalid, abort the not-yet-started task and await completion.

This guarantees the wrapper cannot complete before its active-task record exists.

## Alternative Single-Map Design

A localized refactor may combine slot and task state:

```rust
struct InitAttempt {
    attempt_id: u64,
    leader: InitCompletionSender,
    waiters: Vec<InitCompletionSender>,
    cancellation: CancellationToken,
    abort_handle: Option<AbortHandle>,
    completion: Option<InitTaskCompletionRx>,
}
```

This removes cross-map registration races. Use this only if the refactor remains contained and tests remain straightforward.

## Acceptance Criteria

- A task cannot begin its initialization body before its task-control record is registered.
- Fast success/failure cannot create stale active-task entries.

# Phase 3 — Explicit Normal Completion Cleanup

## Required Behavior

The wrapper should explicitly remove its active-task entry before sending terminal completion:

```rust
active_init_tasks.lock().await.remove(&attempt_id);
let _ = completion_tx.send(exit);
```

Normal success and ordinary failure must not rely on `Drop` cleanup.

## Drop Guard Role

Retain a fallback guard for:

- panic before explicit removal;
- forced abort;
- unexpected future drop.

However, `try_lock()` alone is not sufficient.

Recommended fallback:

```rust
struct ActiveTaskGuard {
    attempt_id: u64,
    active_init_tasks: ActiveTaskMap,
    armed: bool,
}
```

Normal completion calls:

```rust
guard.disarm();
```

after explicit removal.

On drop while armed, spawn a minimal cleanup task:

```rust
tokio::spawn(async move {
    active_init_tasks.lock().await.remove(&attempt_id);
});
```

If spawning during runtime teardown is a concern, use a synchronous coordinator-owned removal after observing task completion. The guard should not silently abandon cleanup because `try_lock()` failed.

## Acceptance Criteria

- Active-task entries disappear after success/failure without shutdown.
- Panic/abort fallback removal is guaranteed or coordinator-owned.

# Phase 4 — Await Cooperative Completion, Then Abort and Await Stragglers

## Required Shutdown Algorithm

After draining task controls:

1. Signal all cancellation tokens.
2. Await all real completion receivers concurrently under one grace deadline.
3. Partition controls into:
   - completed;
   - still pending.
4. Abort only pending tasks.
5. Await the same pending completion receivers under the remaining global deadline.
6. Log panic/cancellation outcomes.
7. Only then continue to client drain/finalization.

## Completion Receiver Polling

Use `FuturesUnordered` or `select_all` over completion receivers while preserving unresolved receivers.

A helper may return:

```rust
struct PendingInitTask {
    attempt_id: u64,
    abort_handle: AbortHandle,
    completion: InitTaskCompletionRx,
}
```

and:

```rust
async fn await_init_tasks_until(
    tasks: Vec<PendingInitTask>,
    deadline: Instant,
) -> (Vec<InitTaskExit>, Vec<PendingInitTask>);
```

The helper must return unresolved controls intact.

## Abort Deadline

After abort:

- await actual completion until the global deadline;
- if a completion channel closes without an exit value, treat it as task termination and log the missing exit metadata;
- do not interpret channel closure before wrapper drop as success unless sender ownership guarantees closure means task termination.

## Acceptance Criteria

- Grace period is aggregate, not per-task.
- Only stragglers are aborted.
- Every aborted task’s real completion is observed.

# Phase 5 — Correct Lock Ordering

## Required Rule

Do not hold `active_init_tasks` while acquiring `initializing`.

Registration should use short, non-nested phases:

1. Check attempt validity under `initializing`.
2. Release `initializing`.
3. Insert control under `active_init_tasks`.
4. Re-check validity under `initializing`.
5. If invalid, remove the control and abort/await the task.

Attempt IDs and lifecycle generation make stale registration harmless.

Do not hold either map lock across task/client I/O.

## Documentation

Update lock-order comments to match actual code. Remove any claim that is not mechanically true.

# Phase 6 — Make Test Factory Cancellation Real

## Production-Like Test Path

Change the injected factory branch to:

```rust
tokio::select! {
    result = init_fn(server, &root) => result,
    _ = cancellation.cancelled() => {
        Err(LspError::InitializationCancelled("shutting down".into()))
    }
}
```

This makes the default blocking test factory cooperative.

## Explicit Uncooperative Factory

Keep a separate test-only path that intentionally ignores cancellation. It should block until aborted and must not be externally released after shutdown.

## Acceptance Criteria

- Cooperative cancellation tests exercise token cancellation.
- Forced-abort tests exercise genuinely cancellation-insensitive futures.

# Phase 7 — Use RAII Drop Guards in Tests

## Test Exit Probe

Add a test-only guard:

```rust
struct FutureExitProbe {
    exited: Arc<AtomicUsize>,
}

impl Drop for FutureExitProbe {
    fn drop(&mut self) {
        self.exited.fetch_add(1, Ordering::SeqCst);
    }
}
```

Construct it at the beginning of each test factory future.

This proves:

- normal return drops the future body;
- cooperative cancellation drops the future body;
- forced abort drops the future body.

Do not rely on code after the blocking await to increment an exit counter.

# Phase 8 — Preserve Forced Lifecycle Finalization

The absolute shutdown deadline and unconditional transition to `Stopped` should remain.

If an aborted task still fails to produce completion by the global deadline:

1. log the attempt ID as an unresolved terminal failure;
2. ensure its abort handle has been signaled;
3. drain all service maps;
4. transition lifecycle to `Stopped`;
5. publish lifecycle state to `watch` subscribers.

Be precise in documentation:

- normal contract: all task termination observed;
- pathological deadline fallback: service state is finalized after abort was requested, with unresolved task completion logged as a severe invariant failure.

Do not claim absolute proof of termination after the runtime deadline if Tokio itself does not deliver the terminal event.

# Phase 9 — Strengthen Tests

## Required Test: Fast Completion Cannot Beat Registration

Use a factory that returns immediately.

Assert after `get_or_create_client()` resolves:

```text
active_init_tasks empty
initializing empty
client published or expected failure returned
```

Run repeatedly in a bounded loop to expose scheduler races.

## Required Test: Normal Completion Removes Entry

Verify success and ordinary failure remove active-task state before any shutdown call.

## Required Test: Cooperative Cancellation Is Observed

1. Start a cancellation-aware blocked factory.
2. Call shutdown.
3. Assert exit probe incremented before shutdown returned.
4. Assert no abort-only instrumentation fired if tracked.

## Required Test: Forced Abort Is Joined

1. Start a factory that ignores cancellation.
2. Call shutdown.
3. Assert its RAII exit probe incremented before return.
4. Do not release the factory externally.
5. Assert completion receiver resolved/closed after abort.

## Required Test: Many Tasks Share One Grace Period

Start several independent initialization attempts. Verify shutdown duration remains bounded near one grace interval rather than `N × grace`.

## Required Test: No Stale Active Entries Under Contention

Run concurrent fast success/failure attempts across multiple keys and assert the active map becomes empty without shutdown.

## Required Test: Lock-Order Regression

Use test hooks to force registration and shutdown overlap. Assert completion under timeout with no deadlock.

## Required Test: Global Deadline Fallback

Retain the existing lifecycle-finalization test, but assert:

```text
all abort handles signaled
maps empty
lifecycle Stopped
watch waiter released
unresolved completion logged/test-observable
```

## Scheduler Coverage

Run service tests with:

```bash
--test-threads=1
--test-threads=8
```

Use Tokio current-thread and multi-thread tests where relevant.

# Phase 10 — Documentation Cleanup

Update documentation to describe the final design accurately:

- task control stores cancellation, abort, and a completion receiver owned by the real wrapper;
- wrapper cannot start before registration completes;
- normal completion explicitly removes task state;
- cancellation grace is aggregate;
- stragglers are aborted and then awaited through the same authoritative completion path;
- no forwarding task detaches real handles;
- `watch` retains shutdown state;
- global deadline fallback semantics are explicit.

Remove references to:

- `JoinSet` forwarders around existing `JoinHandle`s;
- a helper returning unfinished handles if it does not;
- `try_lock()` as a sufficient guaranteed cleanup mechanism.

# Suggested Implementation Order

1. Introduce wrapper-owned completion sender and task-control completion receiver.
2. Add registration start barrier.
3. Remove `drain_joins_with_deadline()` and forwarder tasks.
4. Implement aggregate grace wait returning unresolved controls.
5. Abort unresolved controls and await completion again.
6. Add explicit normal active-map removal and fallback guard.
7. Eliminate nested lock inversion.
8. Wrap injected factories in cancellation.
9. Add RAII exit probes and race tests.
10. Update documentation and run verification.

# File-Level Guidance

## `crates/egglsp/src/service.rs`

Expected changes:

- redesign `InitTaskControl`;
- add task start-registration barrier;
- add authoritative completion sender/receiver;
- delete forwarder-based join helper;
- add aggregate completion/abort helper;
- make normal cleanup explicit;
- correct lock ordering;
- improve test factories and exit probes;
- strengthen quiescence tests.

## `crates/egglsp/src/error.rs`

Likely no change. Add a narrow shutdown invariant error only if needed for structured logging or a future `Result` return.

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

Run focused service tests:

```bash
cargo test -p egglsp service::tests -- --test-threads=1
cargo test -p egglsp service::tests -- --test-threads=8
```

Repeat fast-completion and forced-abort tests in a bounded loop if practical.

# Review Checklist

- [ ] No forwarding task owns a real initialization `JoinHandle`.
- [ ] Every wrapper owns and sends its terminal completion signal.
- [ ] Task startup is blocked until task-control registration completes.
- [ ] Fast completion cannot create stale active-task entries.
- [ ] Normal success/failure explicitly removes active-task state.
- [ ] Drop fallback does not silently fail on lock contention.
- [ ] Grace wait returns unresolved task controls intact.
- [ ] Only unresolved tasks are aborted.
- [ ] Aborted tasks are awaited through authoritative completion.
- [ ] Test factories are cancellation-aware unless explicitly uncooperative.
- [ ] RAII probes prove task-future drop before shutdown returns.
- [ ] No `active_init_tasks -> initializing` nested lock remains.
- [ ] Global deadline still finalizes lifecycle and releases watch waiters.
- [ ] Read-lock cleanup remains intact.
- [ ] Documentation matches implementation.

# Completion Criteria

This pass is complete when:

1. Every initialization wrapper has an authoritative terminal completion signal.
2. Shutdown does not detach real initialization tasks through forwarding handles.
3. Cooperative tasks complete within one aggregate grace period.
4. Uncooperative tasks are aborted and their termination is observed.
5. Fast task completion cannot outrun registration.
6. Active task entries are removed after every terminal path without requiring shutdown.
7. Lock ordering is mechanically correct.
8. Tests prove future termination with RAII drop guards.
9. Lifecycle always reaches `Stopped` and concurrent waiters are released.
10. Phase 1 can be closed and Phase 2 can begin with the scripted stdio fake-server harness.
