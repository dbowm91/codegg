# LSP Phase 3 Final Cleanup: Ownership Slot Integrity, Cancellation Semantics, and Documentation Reconciliation

## Purpose

Close the last outstanding Phase 3 correctness and documentation gaps after the implementation series ending at:

```text
fa68990c50a8078d8a6b484a8459cd48822935cc
```

The repository now contains nearly all planned Phase 3 functionality:

- restart-owner completion channels;
- unified manual and automatic restart entry paths;
- explicit runtime-install outcomes;
- generation-scoped cleanup of unpublished replacements;
- live degraded restart outcomes;
- validated user restart-policy resolution;
- empty-diagnostics readiness coverage;
- expanded restart race tests;
- runtime-backed real-server smoke testing;
- extensive architecture and skill documentation.

The remaining work is narrow but important. It concerns the exact semantics of restart ownership during cancellation, the timing of generation snapshots used for manual supersession, cancellation after a replacement has already been published, permissive race-test assertions, and stale comments that describe earlier algorithms.

This plan is tailored for a smaller implementation model. Follow the passes in order and do not broaden scope.

## Phase 3 Final Closure Definition

Phase 3 is complete only when all of the following hold:

1. Cancelling a restart owner does not remove its ownership entry.
2. The ownership slot remains unavailable until the current owner explicitly signals completion and releases it.
3. Manual supersession snapshots the current generation before cancellation begins.
4. Manual supersession detects any generation advance that occurs while waiting for owner completion.
5. Manual restart never tears down a client unless it owns the restart slot.
6. Cancellation after replacement publication has one explicit policy: finish the replacement to a coherent live state or terminate/remove the exact generation.
7. No cancellation path returns while leaving an incompletely initialized client in an ambiguous state.
8. Race tests assert ownership ordering and generation coherence, not merely bounded completion.
9. Restart algorithm comments describe the actual reservation loop, ownership model, readiness outcomes, and user policy resolution.
10. Architecture, skill, agent, and README documentation use consistent test names and completion claims.
11. The final race suite passes repeatedly with no leaked fake-server processes.

## Primary Files

```text
crates/egglsp/src/restart.rs
crates/egglsp/src/service.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
crates/egglsp/tests/empty_diagnostics_readiness.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not implement:

- Tier 2 language servers;
- new LSP methods;
- multi-root support;
- incremental synchronization;
- pull diagnostics;
- restart jitter;
- TUI work;
- model-facing restart commands;
- unrelated crate refactors.

# Pass 1 — Keep Restart Ownership Installed During Cancellation

## Current Problem

`cancel_restart_ownership()` removes the per-key ownership entry before cancelling the token:

```rust
let ctrl = map.remove(key)?;
ctrl.token.cancel();
```

This creates an interval where:

```text
old coordinator is still unwinding
ownership map is empty
new caller can acquire the key
completion waiter is still waiting
```

That contradicts the intended invariant that cancellation is intent and completion is the ownership boundary.

## Required Change

Update `cancel_restart_ownership()` so it does not remove the entry.

Required sequence:

```text
1. lock restart_tasks
2. lookup control by key
3. clone owner_id and completion receiver
4. cancel control.token
5. leave control entry installed
6. return RestartOwnerWaiter
```

Suggested implementation:

```rust
pub async fn cancel_restart_ownership(
    restart_tasks: &RestartTaskMap,
    key: &str,
) -> Option<RestartOwnerWaiter> {
    let map = restart_tasks.lock().await;
    let ctrl = map.get(key)?;
    ctrl.token.cancel();
    Some(RestartOwnerWaiter {
        owner_id: ctrl.owner_id,
        completion: ctrl.completion.clone(),
    })
}
```

The original owner remains responsible for:

```text
sending RestartCompletion::Finished
removing only its own owner_id-matched entry
```

## Waiter Semantics

Update comments in `RestartOwnerWaiter`:

- the ownership entry remains installed while waiting;
- sender closure without `Finished` is an invariant failure, not proof that the slot is safe;
- do not describe the slot as already removed.

Recommended behavior if the completion sender closes without sending `Finished`:

```text
return InitializationCancelled or an internal invariant error
```

Do not silently treat channel closure as successful completion unless an owner-ID-checked map lookup confirms the old owner entry is gone.

## Tests

### `cancel_does_not_remove_restart_owner`

- acquire owner;
- cancel through `cancel_restart_ownership`;
- attempt second acquisition before release;
- assert `AlreadyInProgress`.

### `completion_release_allows_new_owner`

- cancel owner;
- owner releases;
- waiter resolves;
- new acquisition succeeds.

### `closed_completion_without_release_is_not_success`

Use a controlled test double if practical.

## Acceptance Criteria

- Cancellation alone never empties the ownership slot.
- New acquisition is impossible until owner release.
- Waiter semantics match the map state.

# Pass 2 — Capture the True Pre-Wait Generation

## Current Problem

The manual supersession path attempts to detect a generation advance during the wait, but its “pre-wait” snapshot is currently captured after the wait and after lease acquisition. It therefore usually reads the same generation as the later comparison.

## Required Sequence

Before cancelling any owner, capture:

```rust
struct ManualSupersessionSnapshot {
    generation: u64,
    server_id: String,
    client_identity: Option<usize>,
}
```

A client identity field is optional, but useful in tests. `Arc::as_ptr` may be used internally if needed.

Required manual flow:

```text
1. verify lifecycle Running
2. capture pre_wait_generation and server_id
3. inspect/cancel current owner
4. wait for exact owner completion
5. acquire manual lease
6. re-read current generation and current client
7. compare current generation against pre_wait_generation
8. if generation advanced, abort with ServerRestarted before teardown
9. otherwise continue with diagnostics snapshot and runtime termination
```

Do not call a helper named `restart_owner_diagnostic_snapshot()` after the wait and treat it as pre-wait data.

## Helper Refactor

Replace or rename the current helper to make timing explicit:

```rust
async fn capture_manual_supersession_snapshot(&self, key: &str) -> ManualSupersessionSnapshot;
```

Call it before `cancel_restart_ownership()`.

## Comparison Rules

- `pre_wait_generation == 0` and current generation becomes positive: treat as generation advance;
- current generation greater than pre-wait generation: return `ServerRestarted`;
- current generation equal: safe to proceed;
- current generation lower: invariant error or explicit warning; do not silently proceed.

## Tests

### `manual_detects_generation_advance_during_wait`

- automatic owner publishes generation N+1 before releasing;
- manual waiter resolves;
- manual acquisition succeeds;
- manual returns `ServerRestarted` without terminating generation N+1.

### `manual_same_generation_proceeds_after_wait`

### `manual_timeout_preserves_original_generation`

## Acceptance Criteria

- Pre-wait generation is captured before cancellation.
- Generation advance during wait is observable and enforced.
- New generation is not torn down by stale manual intent.

# Pass 3 — Define One Policy for Cancellation After Publication

## Current Problem

The coordinator cleans up cancellation immediately after spawn, but if cancellation arrives after the replacement is already inserted into the client map and before replay/readiness, it returns `InitializationCancelled` while leaving the replacement live and incompletely initialized.

This produces ambiguous ownership and state semantics.

## Required Policy Decision

Choose one of the following and implement it consistently.

### Preferred Policy — Finish the Published Replacement

Once the replacement is published and becomes visible to other readers:

```text
ignore lease cancellation for teardown purposes
complete retained diagnostics installation
complete document replay
complete readiness evaluation
return Ready or Degraded
release ownership
```

Rationale:

- publication is the irreversible visibility boundary;
- removing a visible replacement can disrupt concurrent readers;
- the manual caller will revalidate generation after owner completion and can decide whether another restart is still needed.

Under this policy:

- cancellation before publication terminates/removes replacement;
- cancellation after publication records a debug note and continues to a coherent `Ready` or `Degraded` result;
- no `InitializationCancelled` is returned after publication merely because the lease token fired.

### Alternative Policy — Deterministically Tear Down Published Replacement

Only use this if the system can guarantee generation-scoped removal without disrupting consumers.

Required sequence:

```text
transition replacement to explicit stopping/restarting state
terminate exact generation runtime
remove exact generation client
restore no older client
return InitializationCancelled
```

This policy is more disruptive and is not recommended.

## Required Code Change for Preferred Policy

Replace the current cancellation branch before replay:

```rust
if token.is_cancelled() {
    return Err(...);
}
```

with:

```text
log cancellation observed after publication
continue replay and readiness
return final live outcome
```

Document the publication boundary explicitly in `restart_client_coordinator`.

## State Semantics

After publication:

- replay failure remains a real error and transitions `Degraded`;
- readiness timeout returns live `RestartOutcome::Degraded`;
- cancellation does not downgrade or abort the coherent completion path.

## Tests

### `cancellation_after_publication_finishes_replacement`

- trigger cancellation after client insertion but before replay;
- assert replacement reaches `Ready` or `Degraded`;
- assert runtime/client remain generation-coherent;
- assert owner releases only after coherent completion.

### `cancellation_before_publication_still_reaps_replacement`

Retain existing cleanup coverage.

### `manual_waits_for_published_replacement_completion_then_revalidates`

- automatic owner publishes replacement;
- manual cancellation occurs;
- automatic finishes coherent outcome;
- manual sees generation advance and returns `ServerRestarted` without teardown.

## Acceptance Criteria

- No post-publication cancellation returns with an ambiguous client state.
- Publication boundary semantics are explicit and tested.

# Pass 4 — Tighten Restart Supersession Tests

## Current Problem

Current race tests accept a wide set of outcomes:

```text
Ok
InitializationCancelled
ServerRestarted
LaunchFailed
```

This proves bounded execution but does not prove ownership order or deterministic supersession semantics.

## Required Test Assertions

Add or tighten tests to assert concrete invariants.

### `cancelled_owner_retains_slot_until_finished`

Assert:

```text
restart_tasks contains original owner while waiter is pending
second acquisition is rejected
owner_id remains unchanged
```

### `manual_acquires_only_after_finished`

Use barriers/notifications:

```text
old owner reaches controlled unwind point
manual cancels and waits
assert manual has not acquired lease
old owner releases
assert manual acquisition occurs afterward
```

### `manual_generation_advance_returns_server_restarted`

Require exactly `ServerRestarted`, not an arbitrary accepted result.

### `post_publication_cancellation_returns_live_outcome`

Require `Ready` or `Degraded`, depending on fixture readiness signal.

### `one_runtime_and_one_client_after_supersession`

Assert exact counts and generation agreement.

### `timeout_does_not_touch_current_client`

Assert same `Arc` identity, generation, runtime entry, and operational state after timeout.

## Avoid Over-Permissive Assertions

Do not use assertions equivalent to:

```rust
assert!(matches!(result, Ok(_) | Err(A) | Err(B) | Err(C)));
```

when the scenario is intended to prove a specific ownership ordering.

## Repeatability

Run the focused race subset ten serial times and five times with default parallelism.

## Acceptance Criteria

- Tests prove order, not merely absence of deadlock.
- Exactly one replacement wins each race.
- No leaked process remains after any scenario.

# Pass 5 — Reconcile Restart Module Documentation

## Current Problem

The top-level `restart.rs` documentation still describes older behavior:

- `for attempt in 1..=policy.max_attempts`;
- caller-owned healthy reset language from earlier designs;
- direct transition to `Ready` on success;
- profile-only readiness/restart policy claims in `from_profile` comments.

Later comments describe the newer reservation and resolved-policy behavior, producing contradictory guidance.

## Required Documentation Rewrite

Update the module-level algorithm to describe the current implementation:

```text
1. acquire per-key ownership
2. snapshot authoritative generation
3. atomically reserve one restart attempt before spawn
4. perform cancellable backoff
5. calculate one replacement generation
6. spawn/initialize replacement
7. clean up if cancelled before publication
8. publish replacement
9. install retained diagnostics
10. replay documents
11. execute readiness policy
12. return Ready or live Degraded
13. set last_healthy_at only for Ready
14. release ownership and signal Finished
```

Document the publication boundary cancellation policy selected in Pass 3.

## Descriptor Documentation

Update `LspClientDescriptor` docs:

- `from_resolved` is the production path for resolved user/profile policy;
- `from_profile` is a convenience/default constructor when no explicit override is supplied;
- remove “no per-user override yet” language.

## Remove Stale Trait Methods or Comments

Audit:

```text
mark_diagnostics_stale_for_key
increment_restart_attempts
set_generation
```

If methods are now unused or retained only for tests, remove them or clearly mark their remaining role. Do not leave comments implying the coordinator calls obsolete methods.

## Acceptance Criteria

- One consistent algorithm is documented.
- No comments describe obsolete retry or policy behavior.

# Pass 6 — Reconcile Architecture and User-Facing Documentation

## Files

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Required Updates

Document:

- ownership entry remains installed during cancellation;
- `RestartCompletion::Finished` is the ownership release boundary;
- manual pre-wait generation snapshot timing;
- publication boundary cancellation semantics;
- exact deterministic supersession test names;
- Phase 3 final closure criteria;
- automatic restart production-readiness status.

## Remove Drift

Search for outdated phrases:

```text
slot removed on cancellation
no per-user restart override
for attempt in 1..=max_attempts
cancelled published client eventually handled by supervisor
manual restart may return any bounded outcome
Phase 3 complete
Phase 3 experimental
```

After implementation and verification, use one consistent status statement.

Recommended final status:

```text
Phase 3 supervision and restart lifecycle complete for Tier 1 servers; broader language/server compatibility remains future work.
```

Do not imply that all LSP roadmap phases are complete.

## Acceptance Criteria

- All docs agree on ownership and cancellation semantics.
- Test names match actual functions.
- Phase status is precise and scoped.

# Pass 7 — Final Verification and Closure Gate

## Focused Tests

```bash
cargo test -p egglsp --features lsp-test-support --lib restart::

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

## Readiness Tests

```bash
cargo test -p egglsp --features lsp-test-support \
  --test empty_diagnostics_readiness
```

## Composite and Workspace Checks

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

## Real-Server Checks When Available

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright --nocapture
```

## Process-Leak Verification

Every race test involving a fake server must assert or otherwise verify:

```text
no remaining runtime entry for cancelled generation
no remaining client entry for cancelled pre-publication generation
exactly one live runtime after successful supersession
all child processes exited after test cleanup
```

## Final Invariant Checklist

- [ ] Cancelling ownership does not remove the map entry.
- [ ] Ownership remains exclusive until `Finished` and release.
- [ ] Manual pre-wait generation is captured before cancellation.
- [ ] Generation advance during wait returns `ServerRestarted`.
- [ ] Manual timeout preserves current client/runtime.
- [ ] Cancellation before publication reaps replacement.
- [ ] Cancellation after publication produces a coherent live outcome or exact teardown.
- [ ] Race tests assert specific ownership order.
- [ ] Exactly one runtime/client remains after supersession.
- [ ] Restart module docs describe attempt reservation and live degraded outcomes.
- [ ] Descriptor docs describe resolved user policy correctly.
- [ ] Architecture and skill docs use current test names.
- [ ] Race suite passes ten serial and five parallel runs.
- [ ] No fake-server process leaks.

# Exact Execution Order for a Smaller Model

1. Fix `cancel_restart_ownership` to retain the slot.
2. Correct waiter failure semantics.
3. Capture manual pre-wait generation before cancellation.
4. Implement the selected post-publication cancellation policy.
5. Tighten race tests around ownership ordering and generation coherence.
6. Rewrite stale `restart.rs` algorithm and descriptor comments.
7. Reconcile architecture, skill, AGENTS, and README documentation.
8. Run repeated race, workspace, and real-server validation.

Do not update Phase 3 status to complete before the repeated race gate passes.

# Recommended Commit Sequence

```text
1. fix(egglsp): retain restart ownership until owner completion
2. fix(egglsp): snapshot generation before manual supersession wait
3. fix(egglsp): make post-publication cancellation coherent
4. test(egglsp): enforce deterministic restart supersession ordering
5. docs(egglsp): rewrite restart algorithm and descriptor policy comments
6. docs(lsp): reconcile Phase 3 closure status and test references
```

# Handoff Output

The implementation handoff must include:

```text
commits created
files changed per pass
exact race-test results for ten serial and five parallel runs
workspace check and Clippy results
real-server versions tested, if available
confirmation that no fake-server processes leaked
remaining limitations, if any
```

After this plan passes, Phase 3 supervision, restart ownership, readiness, and evidence-lifecycle work may be considered closed for the Tier 1 server scope.
