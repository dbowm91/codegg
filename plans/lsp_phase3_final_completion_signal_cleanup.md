# LSP Phase 3 Final Cleanup: Completion-Signal Ordering and Waiter Semantics

## Purpose

Close the last remaining Phase 3 synchronization issue after:

```text
e08c78b20736906f9f937f3256d2cf8eb803876c
```

The repository now implements the complete Tier 1 LSP supervision and restart lifecycle:

- per-key restart ownership;
- cancellation distinct from completion;
- bounded manual supersession;
- pre-wait generation capture and revalidation;
- generation-safe process/runtime ownership;
- exact restart budgets;
- deterministic cleanup before publication;
- coherent completion after publication;
- retained diagnostic provenance;
- readiness-aware replacement outcomes;
- runtime-backed real-server smoke tests;
- repeated serial and parallel race coverage.

One minor race remains in the ownership-release handshake:

```text
RestartLease sends Finished
waiter wakes and checks map
old owner entry may still be present
waiter returns false failure
async cleanup removes entry immediately afterward
```

The final cleanup must make `RestartCompletion::Finished` and ownership-slot release one consistent synchronization boundary.

This plan is deliberately narrow and tailored for a smaller implementation model. Do not modify unrelated LSP behavior.

## Final Closure Definition

Phase 3 is closed when:

1. `RestartCompletion::Finished` is never observable before the matching ownership entry has been removed.
2. A waiter that observes `Finished` can immediately and reliably acquire a new restart lease.
3. Lock contention cannot produce a false `InitializationCancelled` result after successful owner completion.
4. Old-owner cleanup remains owner-ID-safe and cannot remove a newer owner.
5. Sender closure without a valid completion/release remains an invariant failure.
6. Focused adversarial tests prove the release/signalling order.
7. Documentation states that `Finished` means both coordinator completion and slot release.
8. The full race suite passes repeatedly with no child-process leaks.

## Primary Files

```text
crates/egglsp/src/restart.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not change:

- restart budgets;
- manual supersession flow;
- generation assignment;
- runtime termination;
- diagnostic transfer;
- readiness semantics;
- real-server compatibility logic;
- public LSP APIs unrelated to restart ownership.

# Pass 1 — Make Slot Removal Precede `Finished`

## Current Problem

`RestartLease::release_internal()` currently:

```text
1. sends RestartCompletion::Finished
2. tries to remove the ownership entry
3. falls back to spawned async removal if the lock is contended
```

A waiter can observe `Finished` before the slot is actually free.

## Required Ordering

Use this invariant:

```text
remove matching ownership entry first
then send RestartCompletion::Finished
```

`Finished` must mean:

```text
owner coordinator has exited
matching ownership slot has been released
new acquisition may proceed
```

## Preferred Implementation

Replace the synchronous `try_lock` plus spawned fallback with an explicitly async release path.

Recommended API:

```rust
impl RestartLease {
    pub async fn release(mut self) -> bool {
        self.release_async().await
    }

    async fn release_async(&mut self) -> bool {
        if self.released {
            return false;
        }
        self.released = true;

        let removed = {
            let mut map = self.restart_tasks.lock().await;
            match map.get(&self.key) {
                Some(ctrl) if ctrl.owner_id == self.owner_id => {
                    map.remove(&self.key);
                    true
                }
                _ => false,
            }
        };

        if removed {
            if let Some(tx) = self.completion_tx.take() {
                let _ = tx.send(RestartCompletion::Finished);
            }
        }

        removed
    }
}
```

Update all explicit release call sites to await it.

## Drop Semantics

`Drop` cannot await. Choose one of these approaches.

### Preferred

Require explicit async release in production ownership paths and make `Drop` a safety fallback only:

```text
if unreleased:
  move key, owner_id, map, sender into spawned async task
  remove owner-ID-matched entry
  send Finished only after removal
```

The spawned task must preserve the same remove-before-signal ordering.

### Alternative

Eliminate release work from `Drop` and require every owner path to explicitly await release. Only use this if compile-time/control-flow review proves every path releases, including early errors and cancellation.

The preferred fallback is safer.

## Owner-ID Safety

Removal must remain conditional:

```rust
ctrl.owner_id == self.owner_id
```

If the entry is absent or belongs to a newer owner:

- do not remove anything;
- do not signal `Finished` as proof that this owner released the current slot;
- log at debug/warn level as appropriate.

## Acceptance Criteria

- No code path sends `Finished` before owner-ID-matched map removal.
- No detached cleanup task sends `Finished` early.
- New lease acquisition succeeds immediately after waiter completion.

# Pass 2 — Simplify and Correct Waiter Verification

## Current Problem

`RestartOwnerWaiter::wait()` observes `Finished`, then performs a one-shot `verify_slot_free()` check. Under the current ordering this can produce false failure.

After Pass 1, `Finished` already proves slot release.

## Required Semantics

Use:

```text
Finished observed -> success
completion channel closed without Finished -> invariant failure
bounded timeout -> InitializationCancelled
```

The waiter should retain the observed `owner_id` for diagnostics.

## Recommended Implementation

```rust
pub async fn wait(
    mut self,
    timeout: Duration,
) -> Result<(), LspError> {
    if *self.completion.borrow() == RestartCompletion::Finished {
        return Ok(());
    }

    tokio::time::timeout(timeout, async {
        loop {
            self.completion.changed().await.map_err(|_| {
                LspError::InitializationCancelled(
                    format!(
                        "restart owner {} completion channel closed without Finished",
                        self.owner_id,
                    )
                )
            })?;

            if *self.completion.borrow() == RestartCompletion::Finished {
                return Ok(());
            }
        }
    })
    .await
    .map_err(|_| {
        LspError::InitializationCancelled(format!(
            "restart owner {} did not complete within timeout",
            self.owner_id,
        ))
    })?
}
```

## Defensive Verification

A map check may remain only as a debug assertion or diagnostic check:

```rust
#[cfg(debug_assertions)]
assert!(slot absent or owner_id differs)
```

It must not create a production false failure after `Finished` has been observed.

## Cleanup

Remove:

- the discarded `let _ = owner_id`;
- `verify_slot_free()` if no production caller needs it;
- stale comments describing `Finished` followed by map verification.

## Acceptance Criteria

- Waiter success depends on the completion signal.
- Channel closure without `Finished` remains an error.
- Lock scheduling cannot cause a false failure.

# Pass 3 — Add an Adversarial Release-Ordering Test

## Required Unit Test

Add a deterministic test in `crates/egglsp/src/restart.rs`:

```text
finished_is_not_observable_until_slot_is_removed
```

Test design:

1. Acquire owner A.
2. Obtain a completion waiter for owner A.
3. Hold or deliberately contend the restart-task map lock.
4. Trigger owner A release from another task.
5. While the lock is held, assert waiter has not completed.
6. Release the map lock.
7. Await waiter successfully.
8. Immediately acquire owner B.
9. Assert owner B acquisition succeeds and owner ID differs.

This test must fail under the old signal-before-removal implementation.

## Additional Tests

### `drop_fallback_removes_before_finished`

Exercise the `Drop` fallback path with map-lock contention.

Assert:

```text
waiter cannot resolve before slot removal
waiter resolves after cleanup task removes slot
new owner acquires immediately
```

### `old_owner_release_does_not_signal_for_new_owner`

Install/reproduce a newer owner and run delayed old-owner cleanup.

Assert:

```text
new owner remains installed
old waiter does not misrepresent new-owner slot state
```

### `completion_channel_close_without_finished_is_error`

Retain or strengthen the existing invariant-failure test.

## Acceptance Criteria

- Tests deliberately exercise lock contention.
- Old implementation would fail at least one new test.
- Tests assert exact acquisition ordering, not broad accepted outcomes.

# Pass 4 — Audit Every Lease Release Call Site

## Search Targets

```text
.release()
RestartLease
completion_tx
RestartCompletion::Finished
restart_tasks.remove
```

## Required Audit

Every production coordinator path must release ownership exactly once:

- successful `Ready` restart;
- live `Degraded` restart;
- pre-spawn cancellation;
- post-spawn cancellation cleanup;
- launch failure and retry exhaustion;
- manual generation revalidation failure;
- descriptor lookup failure;
- shutdown cancellation;
- unexpected early return.

Prefer structured ownership so the lease remains in scope until function completion and `Drop` is only a fallback.

Do not manually remove restart-task entries outside the lease implementation.

## Acceptance Criteria

- One owner-ID-safe removal implementation exists.
- No caller sends `Finished` directly.
- No caller removes ownership entries directly.
- Early-return paths cannot leak the slot.

# Pass 5 — Documentation Reconciliation

## `restart.rs`

Update comments to state:

```text
RestartCompletion::Finished is emitted only after owner-ID-matched slot removal.
Observing Finished is sufficient proof that the old ownership slot is free.
Cancellation remains intent; Finished remains the release boundary.
```

Correct any comment that says `Finished` is sent before removal.

## Architecture and Skill Documentation

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document the final handshake:

```text
cancel token
owner unwinds
owner removes slot
owner broadcasts Finished
waiter returns
new owner acquires
```

Use a precise final Phase 3 status:

```text
Phase 3 supervision and restart lifecycle complete for Tier 1 servers; broader server compatibility remains future work.
```

Do not claim universal LSP-server compatibility.

## Acceptance Criteria

- No documentation says `Finished` precedes slot removal.
- All documentation uses the same ownership-release sequence.

# Pass 6 — Final Verification Gate

## Focused Unit Tests

```bash
cargo test -p egglsp --features lsp-test-support --lib restart::
```

## Supervisor Suite

```bash
cargo test -p egglsp --features lsp-test-support \
  --test supervisor_restart_stdio -- --test-threads=1
```

## Repeated Race Runs

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

## Required Final Invariants

- [ ] Slot removal occurs before `Finished`.
- [ ] Waiter cannot complete while old owner entry remains installed.
- [ ] Waiter success permits immediate new acquisition.
- [ ] Drop fallback preserves remove-before-signal ordering.
- [ ] Delayed old-owner cleanup cannot remove a newer owner.
- [ ] Channel closure without `Finished` remains an error.
- [ ] All explicit release call sites await async release where applicable.
- [ ] Ten serial and five parallel race runs pass.
- [ ] No fake-server child process leaks.
- [ ] Documentation describes the final ordering correctly.

# Exact Execution Order for a Smaller Model

1. Refactor `RestartLease` to remove before signalling.
2. Update explicit release call sites for async release.
3. Correct the `Drop` fallback ordering.
4. Simplify waiter semantics and remove false-failure verification.
5. Add the lock-contention adversarial tests.
6. Audit all ownership-entry mutation call sites.
7. Reconcile documentation.
8. Run repeated race and full workspace validation.

# Recommended Commit Sequence

```text
1. fix(egglsp): release restart slot before signalling completion
2. refactor(egglsp): simplify owner waiter completion semantics
3. test(egglsp): add adversarial completion-order race coverage
4. docs(lsp): document final restart ownership handshake
```

# Handoff Output

The implementation handoff should report:

```text
commits created
release API changes
all updated call sites
focused adversarial test results
10 serial + 5 parallel race-run results
workspace check, Clippy, and test results
confirmation of no leaked fake-server processes
remaining limitations, if any
```

After this plan passes, Phase 3 should be closed without remaining lifecycle caveats for the Tier 1 server scope.
