# LSP Phase 3 Final Cleanup: Async Release Cancellation Safety and Ownership Diagnostics

## Purpose

Close the final Phase 3 lifecycle issue after:

```text
f26f1fe4c2f964c904fa9444dfdabb82e41aaf4c
```

The remove-before-signal ownership handshake is now correct in the normal path:

```text
remove owner-ID-matched restart slot
broadcast RestartCompletion::Finished
waiter returns
new owner acquires
```

The remaining defect is cancellation safety inside `RestartLease::release_async()`. The method currently marks the lease as released before awaiting the ownership-map lock. If the release future is cancelled or its task is aborted while blocked on that lock, `Drop` sees `released == true` and skips fallback cleanup. The per-key ownership slot can then remain wedged indefinitely and the completion channel closes without `Finished`.

This plan is intentionally narrow and tailored for a smaller implementation model. Do not modify unrelated LSP behavior.

## Final Closure Definition

Phase 3 is complete when all of the following hold:

1. Cancelling or aborting `RestartLease::release()` while it waits for the ownership-map lock cannot leak the slot.
2. `released` is not committed before the final cancellation point.
3. `Drop` fallback cleanup still runs when the async release future is cancelled before map removal.
4. Once map removal succeeds, no later cancellation point can prevent completion signalling.
5. Waiter timeout and channel-closure errors identify the relevant owner ID.
6. A deterministic adversarial test aborts a blocked release task and proves fallback cleanup removes the slot and emits `Finished`.
7. The explicit release path and `Drop` fallback preserve the same remove-before-signal ordering.
8. All production release call sites continue to await `release()`.
9. The focused race suite and full workspace checks pass.
10. Documentation states the async cancellation-safety invariant accurately.

## Primary Files

```text
crates/egglsp/src/restart.rs
crates/egglsp/src/service.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not change:

- restart ownership acquisition semantics;
- manual supersession sequencing;
- restart budgets;
- runtime generation logic;
- process shutdown or reap behavior;
- diagnostic retention;
- readiness policy behavior;
- real-server compatibility logic;
- public APIs outside restart ownership cleanup.

# Pass 1 — Make `release_async()` Cancellation-Safe

## Current Problem

The current implementation performs:

```rust
self.released = true;
let mut map = self.restart_tasks.lock().await;
```

This creates the failure sequence:

```text
release future starts
released becomes true
future blocks on restart_tasks lock
release task is aborted or future is dropped
RestartLease::drop runs
Drop sees released == true and returns
slot remains installed
completion sender closes without Finished
```

This violates the lease fallback invariant.

## Required Ordering

Do not set `released = true` until after the final `await` in the explicit release path.

Recommended implementation:

```rust
async fn release_async(&mut self) -> bool {
    if self.released {
        return false;
    }

    let key = self.key.clone();
    let owner_id = self.owner_id;

    let removed = {
        let mut map = self.restart_tasks.lock().await;

        // There are no further await points after this mutation.
        self.released = true;

        match map.get(&key) {
            Some(ctrl) if ctrl.owner_id == owner_id => {
                map.remove(&key);
                true
            }
            _ => false,
        }
    };

    if removed {
        if let Some(tx) = self.completion_tx.take() {
            let _ = tx.send(RestartCompletion::Finished);
        }
    } else {
        let _ = self.completion_tx.take();
    }

    removed
}
```

Key invariant:

```text
before lock acquisition completes:
  released == false, so Drop fallback remains armed

after released becomes true:
  no await points remain
```

## Alternative State Model

A small explicit state enum is acceptable if it improves clarity:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartLeaseReleaseState {
    Active,
    ReleasingCommitted,
    Released,
}
```

However, do not introduce this unless necessary. Moving the mutation after lock acquisition is sufficient and lower-risk.

## Removal Failure

If the slot is absent or belongs to a newer owner:

- mark the lease released only after the lock is acquired;
- suppress `Finished`;
- drop the sender so waiters observe channel closure;
- retain existing owner-ID safety.

## Acceptance Criteria

- No state mutation that disables `Drop` occurs before an `await`.
- Explicit release still removes before signalling.
- Removal failure still suppresses a misleading `Finished` signal.

# Pass 2 — Add the Abort-While-Blocked Adversarial Test

## Required Test

Add a deterministic unit test in `crates/egglsp/src/restart.rs`:

```text
cancelled_async_release_falls_back_to_drop_cleanup
```

## Test Design

Use this exact sequence:

1. Create a fresh `RestartTaskMap` and owner counter.
2. Acquire owner A and obtain a completion waiter by calling `cancel_restart_ownership` or cloning the control receiver.
3. Acquire and hold the `restart_tasks` mutex from the test task.
4. Spawn a task that owns the lease and calls `lease.release().await`.
5. Ensure the release task is blocked on the map lock.
6. Abort the release task.
7. Release the held map lock.
8. Allow the lease `Drop` fallback task to run.
9. Await the completion waiter and require `Ok(())`.
10. Assert the old slot is absent.
11. Immediately acquire owner B and require success.
12. Assert owner B has a different owner ID.

## Synchronization

Do not depend only on arbitrary sleeps.

Use one of:

- `tokio::sync::Barrier`;
- `Notify`;
- a test-only hook invoked immediately before `restart_tasks.lock().await`;
- polling with a strict bounded timeout only if no hook is practical.

The test must deterministically prove the release task reached the blocked-lock state before aborting it.

## Expected Failure Under Old Code

Under the old implementation:

```text
released == true before lock await
aborted task drops lease
Drop skips cleanup
waiter observes closure/error or times out
new acquisition remains blocked by stale slot
```

The new test must fail against that implementation.

## Additional Assertions

Assert:

```text
restart_tasks contains owner A while test holds the lock
completion is not Finished before lock release
restart_tasks does not contain owner A after fallback cleanup
completion becomes Finished only after removal
```

## Acceptance Criteria

- Test is deterministic.
- Test exercises task abortion, not only token cancellation.
- New owner acquisition succeeds immediately after waiter completion.

# Pass 3 — Add a Direct Future-Drop Cancellation Test

## Goal

Cover cancellation by dropping the `release()` future without spawning/aborting a task.

## Suggested Test

```text
cancelled_release_future_keeps_drop_fallback_armed
```

Possible structure:

1. Hold the map lock.
2. Pin `lease.release()`.
3. Poll once so it reaches the pending lock acquisition.
4. Drop the future.
5. Release the map lock.
6. Verify `Drop` fallback removes the slot and signals `Finished`.

If Rust ownership makes this awkward because `release(self)` consumes the lease into the future, the task-abort test from Pass 2 is sufficient. Only add this test if it can be written clearly without unsafe code or fragile executor internals.

## Acceptance Criteria

- No unsafe code is introduced.
- Skip this pass if it duplicates Pass 2 without adding meaningful coverage.

# Pass 4 — Improve Waiter Diagnostic Errors

## Current Problem

`RestartOwnerWaiter::wait()` retains `owner_id` but discards it:

```rust
let _ = owner_id;
```

Timeout and channel-closure errors therefore omit the owner involved.

## Required Change

Include `owner_id` in both errors.

Recommended messages:

```rust
LspError::InitializationCancelled(format!(
    "restart owner {owner_id} completion channel closed without Finished signal"
))
```

and:

```rust
LspError::InitializationCancelled(format!(
    "restart owner {owner_id} did not signal completion within {timeout:?}"
))
```

Remove:

```rust
#[allow(dead_code)]
```

from `RestartOwnerWaiter::owner_id` if no longer needed.

## Tests

Add or update tests to assert error messages contain the owner ID:

```text
completion_channel_close_error_names_owner
completion_timeout_error_names_owner
```

Do not overfit the entire error string if repository conventions prefer substring assertions.

## Acceptance Criteria

- Owner ID appears in closure and timeout diagnostics.
- No discarded owner-ID binding remains.

# Pass 5 — Audit Release-State Mutations and Call Sites

## Search Targets

```text
released = true
RestartLease::release
.release().await
completion_tx.take
RestartCompletion::Finished
restart_tasks.remove
```

## Required Audit

Confirm:

- only `RestartLease` implementation removes restart ownership entries;
- only release/drop cleanup sends `Finished`;
- no caller mutates `released` directly;
- all production ownership paths call `release().await` explicitly;
- early-return and error paths retain the lease until explicit release or fallback drop;
- no production path uses `mem::forget`, `ManuallyDrop`, or similar mechanisms that bypass cleanup;
- no lock is held across unrelated process I/O or readiness waits.

## Service Call Sites

Review all `release().await` sites in `service.rs` and ensure:

```text
manual generation-advance return
manual timeout/cancellation return
Ready outcome
Degraded outcome
coalesced automatic restart
coordinator error
shutdown cancellation
```

all either explicitly release or intentionally rely on `Drop` as a documented fallback.

Prefer explicit release in normal control flow.

## Acceptance Criteria

- One release-state implementation exists.
- No direct map removal or completion send exists outside lease cleanup.
- Normal production paths do not depend on `Drop`.

# Pass 6 — Document Async Cancellation Safety

## `restart.rs`

Add a concise cancellation-safety note to `RestartLease::release`:

```text
`released` is committed only after the async map-lock acquisition completes. Therefore cancellation while waiting for the lock leaves Drop fallback armed. After the commit there are no further await points, so cleanup cannot be interrupted between disabling fallback and removing/signalling.
```

Update `Drop` comments to clarify that it handles:

- forgotten explicit release;
- panic/early return;
- cancellation or task abortion while explicit release is blocked before commit.

## Architecture and Skill Docs

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Only add detail where useful. The minimum required statement is:

```text
The async release path is cancellation-safe: fallback cleanup remains armed until the ownership-map lock is acquired, and no await occurs after release state is committed.
```

Do not add another large historical pass section unless existing documentation structure requires it.

## Phase Status

Keep the scoped statement:

```text
Phase 3 supervision and restart lifecycle complete for Tier 1 servers; broader language/server compatibility remains future work.
```

Update completion wording only after all tests pass.

## Acceptance Criteria

- Documentation describes the cancellation-safety invariant.
- No documentation implies that task abortion during release can leak ownership.

# Pass 7 — Final Verification Gate

## Focused Unit Tests

```bash
cargo test -p egglsp --features lsp-test-support --lib restart::
```

Run the new test directly:

```bash
cargo test -p egglsp --features lsp-test-support --lib \
  cancelled_async_release_falls_back_to_drop_cleanup -- --nocapture
```

## Repeated Adversarial Run

```bash
for i in 1 2 3 4 5 6 7 8 9 10; do
  cargo test -p egglsp --features lsp-test-support --lib \
    cancelled_async_release_falls_back_to_drop_cleanup || exit 1
done
```

## Supervisor Race Suite

```bash
for i in 1 2 3 4 5 6 7 8 9 10; do
  cargo test -p egglsp --features lsp-test-support \
    --test supervisor_restart_stdio -- --test-threads=1 || exit 1
done

for i in 1 2 3 4 5; do
  cargo test -p egglsp --features lsp-test-support \
    --test supervisor_restart_stdio || exit 1
done
```

## Full Validation

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

## Leak Verification

Tests involving release abortion must confirm:

```text
old restart ownership entry removed
completion waiter resolves successfully
new ownership acquisition succeeds
no runtime/client/process state is affected unexpectedly
```

This bug concerns restart ownership metadata, not child process ownership, but the full supervisor suite must still report no leaked fake-server children.

## Final Invariant Checklist

- [ ] `released` remains false while waiting for the map lock.
- [ ] Aborting blocked `release().await` triggers `Drop` fallback cleanup.
- [ ] Fallback removes the slot before `Finished`.
- [ ] Waiter resolves after fallback cleanup.
- [ ] New owner acquires immediately after waiter completion.
- [ ] Removal failure suppresses `Finished`.
- [ ] Timeout and closure errors include owner ID.
- [ ] Production release sites await explicit release.
- [ ] Ten focused abort tests pass.
- [ ] Ten serial and five parallel supervisor runs pass.
- [ ] Workspace check, Clippy, and tests pass.
- [ ] Documentation states async cancellation safety correctly.

# Exact Execution Order for a Smaller Model

1. Move `released = true` after map-lock acquisition.
2. Verify there are no later await points in the committed path.
3. Add the abort-while-blocked adversarial test.
4. Add owner ID to waiter diagnostics.
5. Audit all release-state and ownership-map mutation sites.
6. Update concise cancellation-safety documentation.
7. Run focused repeated tests and full validation.

# Recommended Commit Sequence

```text
1. fix(egglsp): make async restart lease release cancellation-safe
2. test(egglsp): abort blocked lease release and verify fallback cleanup
3. chore(egglsp): include restart owner id in waiter errors
4. docs(lsp): document async release cancellation safety
```

# Handoff Output

The implementation handoff should report:

```text
commits created
exact release-state code change
new adversarial test mechanics and results
owner-ID diagnostic changes
all audited release call sites
10 focused abort-test results
10 serial + 5 parallel supervisor results
workspace check, Clippy, and test results
confirmation that no ownership slot or fake-server process leaked
remaining limitations, if any
```

After this plan passes, Phase 3 can be closed without remaining ownership or lifecycle caveats for the Tier 1 server scope.
