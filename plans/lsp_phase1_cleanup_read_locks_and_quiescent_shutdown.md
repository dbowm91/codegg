# LSP Phase 1 Cleanup: Read Locks and Quiescent Shutdown

## Purpose

Perform the final narrow cleanup after:

```text
6020c3e85b16e7a64587b1a55ee2b8eb26b8927b
```

The protocol-peer and initialization coordinator work is now functionally correct. This pass should remove a concurrency regression in client-map access and make service shutdown quiescent with respect to in-flight initialization tasks.

This is a cleanup pass, not a feature phase.

## Scope

Primary files:

```text
crates/egglsp/src/service.rs
crates/egglsp/src/client.rs
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Possible additional dependency or internal helper:

```text
tokio_util::sync::CancellationToken
```

Only add a dependency if the workspace does not already expose an equivalent cancellation primitive.

## Non-Goals

Do not implement:

- the scripted stdio fake LSP server;
- automatic server restart;
- pull diagnostics;
- incremental document synchronization;
- multi-root workspace support;
- new model-facing LSP operations;
- changes to semantic context or security context behavior;
- direct mutation through `workspace/applyEdit`.

## Preserve Existing Correctness

Do not regress:

- atomic same-key leader election;
- shared completion fan-out for leader and waiters;
- attempt-ID compare-and-remove cleanup;
- lifecycle-generation validation held through client publication;
- explicit disposal of unpublished clients;
- public-path concurrency tests;
- timeout cancellation and transport-failure propagation;
- deterministic document ownership;
- idempotent close/save behavior;
- read-only LSP tool semantics.

## Required Invariants

1. Non-mutating client-map access uses a read guard.
2. Client-map write guards are limited to insertion, removal, draining, or actual mutation.
3. No client-map guard is held across client I/O.
4. `shutdown_all()` does not return while initialization tasks remain active.
5. Shutdown cancellation resolves every waiting leader/waiter exactly once.
6. A cancelled initialization task cannot publish a client after shutdown.
7. A client created during cancellation is explicitly disposed.
8. Concurrent shutdown callers either await the same shutdown completion or return only after the service is stopped.
9. Shutdown remains bounded and cannot hang indefinitely on a nonresponsive language server.

# Phase 1 — Restore Read Locks for Non-Mutating Client Access

## Current Problem

Several service methods acquire:

```rust
self.clients.write().await
```

but only perform read operations such as:

```text
get
contains_key
keys
clone handle
```

This unnecessarily serializes unrelated diagnostics, request routing, capability reads, file lifecycle lookups, and client enumeration.

## Required Audit

Review every `clients.write().await` in `crates/egglsp/src/service.rs`.

Use `clients.read().await` for operations that only:

- look up a client by key;
- clone an `Arc<LspClient>`;
- test whether a key exists;
- enumerate keys;
- read capability handles;
- resolve document-owner client handles;
- inspect diagnostics/client state indirectly after cloning the handle.

Retain write guards only for:

- successful client publication;
- `entry(...).or_insert...` or equivalent insertion;
- removing one client;
- draining all clients during shutdown;
- any genuine client-map mutation.

Likely methods to correct include:

```text
open_file
update_file
close_file
save_file
is_file_open
get_diagnostics_for_key
get_all_diagnostics_for_key
diagnostics_may_still_be_warming
get_diagnostic_snapshot_for_key
send_request
client_keys
get_capabilities_for_key
find_existing_client_for_root_hint indirectly through client_keys
```

Do not mechanically replace publication or shutdown write locks.

## Lock Lifetime Pattern

Use the existing short-scope pattern:

```rust
let client = {
    let clients = self.clients.read().await;
    clients.get(key).cloned()
};

// guard released before await on client
client.send_request(...).await
```

## Tests

Add a concurrency regression test demonstrating that two read-only operations can proceed concurrently while a test hook blocks one client-local operation.

A simpler structural test may assert read access through helper methods, but a behavioral test is preferred.

At minimum verify:

- slow client A request does not block lookup of client B;
- `client_keys()` can run concurrently with a diagnostics lookup;
- publication still excludes concurrent slot creation correctly;
- shutdown write lock still blocks new publication as designed.

## Acceptance Criteria

- No non-mutating service method takes the client-map write lock.
- Independent read operations are not serialized by the map.

# Phase 2 — Track In-Flight Initialization Tasks

## Current Problem

Initialization attempts are spawned and detached. `shutdown_all()` drains completion senders and invalidates lifecycle generation, but it does not wait for or abort the underlying initialization tasks.

A task may continue after shutdown returns while:

- downloading a language server;
- spawning a process;
- waiting for `initialize`;
- sending `initialized`;
- disposing an invalidated client.

State correctness is preserved, but shutdown is not quiescent.

## Required Design

Extend each initialization slot with task cancellation/termination state.

Recommended shape:

```rust
struct InitSlot {
    attempt_id: u64,
    leader: InitCompletionSender,
    waiters: Vec<InitCompletionSender>,
    cancellation: CancellationToken,
    abort_handle: Option<tokio::task::AbortHandle>,
}
```

If `CancellationToken` is unavailable, use a `watch`, `Notify`, or oneshot cancellation channel plus `AbortHandle`.

The slot should be inserted before spawning. After spawning:

- store the task abort handle only if the attempt ID still matches;
- if shutdown removed the slot before the handle is stored, immediately abort/cancel the task;
- avoid a race where an untracked task survives slot removal.

## Cooperative Cancellation

Prefer cooperative cancellation around major initialization stages:

```text
before download
before process spawn
before initialize request
before initialized notification
before publication
```

Use `tokio::select!` between cancellation and long-running operations where practical.

At minimum, an abort handle must exist so shutdown can terminate the task.

Do not rely only on lifecycle invalidation after all initialization work has completed.

## Task Completion Tracking

Shutdown needs a completion primitive for each task. Options:

- retain `JoinHandle` in a task registry separate from the slot;
- store an abort handle plus a completion receiver;
- maintain an `active_init_tasks` map keyed by attempt ID.

Recommended internal representation:

```rust
struct InitTaskControl {
    attempt_id: u64,
    cancellation: CancellationToken,
    abort_handle: AbortHandle,
    finished: oneshot::Receiver<()>,
}
```

Keep completion senders for API callers separate from task completion tracking.

## Acceptance Criteria

- Every spawned initialization task is tracked before it can outlive the service.
- Shutdown can signal cancellation and await termination.
- Attempt cleanup remains guarded by attempt ID.

# Phase 3 — Make `shutdown_all()` Quiescent

## Required Shutdown Sequence

Use a deterministic sequence:

1. Acquire lifecycle write lock.
2. If already `Stopped`, return.
3. If already `ShuttingDown`, await the existing shutdown completion rather than returning immediately.
4. Transition `Running -> ShuttingDown` and increment generation.
5. Release lifecycle lock.
6. Clear document ownership.
7. Drain initialization slots and task controls.
8. Notify leader/waiter completion senders with shutdown cancellation.
9. Signal cooperative cancellation to all initialization tasks.
10. Abort tasks that do not terminate within a short grace interval.
11. Await all task completion handles with a bounded timeout.
12. Drain ready clients.
13. Gracefully shut down ready clients with bounded per-client or aggregate timeout.
14. Transition lifecycle to `Stopped`.
15. Notify concurrent shutdown waiters.

Ordering between ready-client drain and init-task cancellation may be adjusted, but document it and ensure no task can publish after the ready-client drain.

## Concurrent Shutdown Callers

Current behavior returns immediately for callers that observe `ShuttingDown`. Replace that with a shared completion signal.

Suggested lifecycle support:

```rust
struct LifecycleState {
    phase: ServiceLifecycle,
    generation: u64,
    shutdown_epoch: u64,
}
```

and a `watch` or `Notify` channel indicating transition to `Stopped`.

A second caller should:

- observe `ShuttingDown`;
- wait until phase becomes `Stopped`;
- then return.

This gives `shutdown_all()` a consistent contract: after it returns, shutdown is complete.

## Bounded Termination

Do not wait indefinitely.

Suggested policy:

```text
cooperative cancellation grace: 250–500 ms
forced abort wait: 1–2 s
client graceful shutdown: existing 2 s bound or equivalent
```

Use constants in one place and document them.

## Acceptance Criteria

- No initialization task remains active after `shutdown_all()` returns.
- Concurrent shutdown callers all return after the service is `Stopped`.
- No client process remains reachable through the service.

# Phase 4 — Harden Initialization Task Cleanup

## Cancellation Path

When an initialization task observes cancellation or is aborted:

- remove only the matching attempt;
- notify callers only if shutdown has not already drained senders;
- avoid double-send assumptions;
- explicitly dispose any already-created client;
- signal task-finished tracking.

## Panic Path

The panic monitor should also signal task completion and should not race with shutdown cleanup.

Unify panic, explicit cancellation, lifecycle invalidation, and normal completion through one terminal helper where practical:

```rust
async fn finish_attempt(
    key: &str,
    attempt_id: u64,
    terminal: InitTerminal,
    ...
)
```

Possible terminal states:

```rust
enum InitTerminal {
    Published(Arc<LspClient>),
    Failed(SharedInitError),
    Cancelled(SharedInitError),
    Panicked(SharedInitError),
}
```

Do not broaden this refactor beyond the initialization coordinator.

# Phase 5 — Add Shutdown-Quiescence Tests

Use deterministic barriers and notifications. Do not depend on sleeps except inside bounded timeout assertions.

## Required Test: Download/Factory Blocked During Shutdown

1. Start public-path initialization with a blocked test factory.
2. Confirm factory entered.
3. Call `shutdown_all()`.
4. Assert shutdown does not return before cancellation/abort completes.
5. Assert factory task is no longer active.
6. Assert leader/waiters receive cancellation.
7. Assert clients, initializing, ownership, and active-task maps are empty.

## Required Test: Cancellation-Uncooperative Task

Use a test task that ignores cooperative cancellation.

Assert:

- grace timeout expires;
- abort handle is invoked;
- shutdown still completes within the global bound;
- no task remains active.

## Required Test: Concurrent Shutdown Callers

1. Begin shutdown and pause it with a test hook.
2. Call `shutdown_all()` from a second task.
3. Assert the second task remains pending while phase is `ShuttingDown`.
4. Release the first shutdown.
5. Assert both callers return after phase is `Stopped`.

## Required Test: Publication Race Remains Safe

Repeat the successful initialization versus shutdown test after task tracking is introduced.

Assert:

- either publication occurs and shutdown drains it;
- or cancellation prevents publication and disposes the client;
- shutdown returns with no active tasks or clients.

## Required Test: Read-Lock Concurrency

Demonstrate two read-only client-map operations proceed without exclusive serialization.

## Time Bounds

Wrap all task joins with `tokio::time::timeout` so deadlocks fail clearly.

# Phase 6 — Documentation

Update:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Document:

- client-map read versus write lock discipline;
- ready client map mutations that legitimately require write guards;
- tracked initialization tasks;
- cooperative cancellation and forced abort fallback;
- quiescent `shutdown_all()` contract;
- behavior of concurrent shutdown callers;
- shutdown timeout policy.

Do not claim full process supervision or automatic restart; those remain later roadmap work.

# Suggested Implementation Order

1. Audit and replace read-only client-map write guards.
2. Add active initialization task-control state.
3. Store cancellation and abort handles without introducing a spawn-registration race.
4. Make shutdown callers share a completion signal.
5. Implement bounded task cancellation/abort and await.
6. Unify terminal attempt cleanup where necessary.
7. Add deterministic shutdown-quiescence tests.
8. Update documentation and run verification.

# File-Level Guidance

## `crates/egglsp/src/service.rs`

Expected changes:

- replace read-only `clients.write()` with `clients.read()`;
- extend initialization slot/task tracking;
- implement quiescent shutdown;
- add shared shutdown completion signaling;
- add cancellation/abort handling;
- add concurrency tests.

## `crates/egglsp/src/client.rs`

Likely minimal changes. Reuse existing bounded `shutdown()` behavior and test stub disposal counters. Add cancellation-aware initialization seams only if needed.

# Verification

Run:

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo test -p egglsp
cargo test --workspace
cargo clippy -p egglsp --all-targets -- -D warnings
cargo clippy --workspace --all-targets -- -D warnings
```

Run service tests under both scheduler configurations:

```bash
cargo test -p egglsp service::tests -- --test-threads=1
cargo test -p egglsp service::tests -- --test-threads=8
```

# Review Checklist

- [ ] Read-only client-map access uses `read()`.
- [ ] Write guards are limited to actual mutation.
- [ ] No client-map guard survives client I/O.
- [ ] Every initialization task has cancellation and completion tracking.
- [ ] No spawn-before-registration race leaves an untracked task.
- [ ] Shutdown drains caller completion senders.
- [ ] Shutdown cancels or aborts every initialization task.
- [ ] Shutdown waits for task termination with a bound.
- [ ] Concurrent shutdown callers await the same completion.
- [ ] Unpublished clients are explicitly disposed.
- [ ] Attempt-ID cleanup remains safe against stale tasks.
- [ ] Service maps are empty after shutdown returns.
- [ ] Existing coordinator race tests still pass.
- [ ] Documentation matches behavior.

# Completion Criteria

This cleanup is complete when:

1. Non-mutating LSP service operations no longer serialize on the client-map write lock.
2. Every initialization task is tracked and cancellable.
3. `shutdown_all()` returns only after initialization tasks have terminated or been forcibly aborted.
4. Concurrent shutdown callers all return after the service reaches `Stopped`.
5. Shutdown remains bounded under unresponsive tasks or servers.
6. No client can publish after shutdown invalidation.
7. All ready clients, active attempts, task controls, and document ownership are empty after shutdown.
8. Existing Phase 1 protocol and coordinator tests remain green.
9. The repository is cleanly ready for the Phase 2 scripted stdio fake-server harness.
