# LSP Phase 3 Final Gap Closure: Restart Ownership, Cancellation Cleanup, and Degraded Outcomes

## Purpose

Close the remaining Phase 3 concurrency and lifecycle gaps after the implementation series ending at:

```text
067add8d6855eafb7da0f03d53aef712f1fdc7fd
```

Most Phase 3 work is now complete. The remaining defects are concentrated in one narrow subsystem:

- manual restart cancellation does not wait for the previous automatic owner to finish before touching the current client;
- a restart cancelled after process spawn can leave an unpublished process/runtime alive;
- runtime installation rejection is ambiguous and can leave a losing runtime untracked;
- restart ownership cancellation removes the slot before the old coordinator has actually exited;
- degraded readiness is represented as `LaunchFailed` even though a live degraded client remains published;
- one readiness test is misnamed and does not prove that an empty `publishDiagnostics` notification satisfies readiness;
- restart descriptor documentation still understates user restart-policy overrides.

This plan is intentionally narrow. Do not add new LSP features or broaden Phase 3 scope.

## Completion Definition

Phase 3 is complete only when:

1. Manual restart cannot terminate or remove the current client until it owns the restart slot.
2. Cancelling an automatic restart waits for that exact owner to finish before manual ownership is granted.
3. A cancelled restart that has already spawned a replacement always terminates and reaps it.
4. A rejected runtime installation always terminates and reaps the rejected runtime.
5. Runtime-install outcomes distinguish installed, replaced, and rejected states explicitly.
6. Restart-control entries remain installed until coordinator completion, not merely token cancellation.
7. Degraded readiness returns a distinct live outcome rather than `LaunchFailed`.
8. Restart budgets and health metadata treat degraded-but-live clients consistently.
9. Empty diagnostics readiness is proven with a real empty `publishDiagnostics` notification.
10. Documentation matches the actual restart-policy and ownership model.
11. All race tests pass repeatedly.

## Primary Files

```text
crates/egglsp/src/restart.rs
crates/egglsp/src/service.rs
crates/egglsp/src/runtime.rs
crates/egglsp/src/client.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
crates/egglsp/tests/real_server_smoke.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not implement:

- new language servers;
- new LSP methods;
- multi-root support;
- incremental sync;
- pull diagnostics;
- restart jitter;
- TUI work;
- model-facing restart commands;
- unrelated refactors.

# Pass 1 — Add Restart Owner Completion Signaling

## Current Problem

`RestartTaskControl` contains an owner ID, trigger, and cancellation token, but no completion signal. `cancel_restart_ownership()` removes the slot immediately and cancels the token. A new owner can therefore acquire the key while the old coordinator is still unwinding.

Cancellation is intent, not completion.

## Required Structure

Extend restart ownership with an explicit completion channel:

```rust
pub struct RestartTaskControl {
    pub owner_id: u64,
    pub trigger: RestartTrigger,
    pub token: CancellationToken,
    pub completion: watch::Receiver<RestartCompletion>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartCompletion {
    Running,
    Finished,
}
```

A oneshot is also acceptable if the control map can expose a clonable waiter. A `watch` channel is preferred because manual supersession and shutdown may both need to wait.

`RestartLease` should own the matching completion sender:

```rust
completion_tx: Option<watch::Sender<RestartCompletion>>
```

On explicit release or drop:

```text
send Finished
remove map entry only if owner_id still matches
```

Do not remove the map entry when merely cancelling the token.

## New APIs

Add:

```rust
pub async fn cancel_restart_owner(
    restart_tasks: &RestartTaskMap,
    key: &str,
) -> Option<RestartOwnerWaiter>;

pub struct RestartOwnerWaiter {
    owner_id: u64,
    completion: watch::Receiver<RestartCompletion>,
}

impl RestartOwnerWaiter {
    pub async fn wait(self, timeout: Duration) -> Result<(), LspError>;
}
```

Cancellation sequence:

```text
lookup current control
clone completion receiver
cancel token
leave control entry installed
return waiter
```

## Tests

### `cancel_does_not_release_restart_slot`

Assert a second acquisition still sees `AlreadyInProgress` until the first owner releases.

### `owner_completion_waiter_resolves_on_release`

### `old_owner_release_cannot_remove_new_owner`

Retain owner-ID safety.

## Acceptance Criteria

- Token cancellation does not remove ownership.
- Completion is observable separately from cancellation.
- Ownership remains exclusive until coordinator exit.

# Pass 2 — Make Manual Supersession Acquire Ownership Before Touching the Client

## Current Problem

`manual_restart_client()` currently cancels the automatic token, then snapshots diagnostics, terminates the current runtime, removes the client, and only afterward attempts to acquire the manual lease.

This allows manual restart to dismantle the current generation without owning the restart slot.

## Required Sequence

Use this exact order:

```text
1. verify service is Running
2. inspect current restart owner
3. if automatic owner exists:
   a. cancel token
   b. wait for that exact owner completion under bounded timeout
4. acquire manual restart lease
5. re-read authoritative generation and current client
6. snapshot diagnostics from that client
7. terminate runtime using Some(old_client.clone())
8. remove old client only after termination
9. start replacement coordinator while holding manual lease
10. release lease after coordinator completion
```

Do not snapshot or terminate before manual ownership is acquired.

## Race Revalidation

After acquiring the manual lease, re-read:

```text
current generation
current client
current runtime generation
descriptor
```

If a newer generation appeared while waiting for ownership, restart that newer generation or return an explicit stale-operation error. Do not operate on cached pre-wait values.

## Timeout Behavior

If the cancelled owner does not complete within a bounded timeout:

- do not terminate the current client;
- return `InitializationCancelled` or a dedicated busy error;
- preserve current runtime and ownership state.

## Tests

### `manual_restart_waits_for_cancelled_automatic_owner`

Use a barrier to hold the automatic coordinator after spawn/pre-publication. Assert manual restart does not terminate the current client until automatic completion is signaled.

### `manual_restart_timeout_preserves_current_client`

### `manual_restart_revalidates_generation_after_wait`

## Acceptance Criteria

- Manual restart never touches the live client before ownership.
- Cancelled automatic owner is confirmed finished first.
- Timeout leaves the current generation intact.

# Pass 3 — Introduce Explicit Runtime Installation Outcomes

## Current Problem

`install_runtime()` returns `Option<RuntimeEntry>` for both successful replacement and rejection. Callers cannot distinguish the outcomes reliably.

## Required Type

Add:

```rust
pub enum RuntimeInstallResult {
    Installed,
    Replaced { prior: RuntimeEntry },
    Rejected {
        existing_generation: u64,
        requested_generation: u64,
    },
}
```

Update:

```rust
async fn install_runtime(...) -> RuntimeInstallResult
```

Rules:

```text
no existing entry -> Installed
older existing entry -> Replaced { prior }
same/newer existing entry -> Rejected
```

A replaced prior runtime should normally already be terminated. If it is still live, treat that as an invariant violation and terminate it explicitly.

## Caller Contract

Every caller must match exhaustively.

On `Rejected`:

```text
terminate requested runtime immediately
wait/reap under bounded deadline
return ServerRestarted or InitializationCancelled
never publish client
```

Ignoring the result must be impossible.

## Tests

### `same_generation_install_is_rejected`

### `rejected_install_kills_requested_runtime`

### `older_generation_replacement_reports_prior_entry`

## Acceptance Criteria

- Installation result is unambiguous.
- Rejected runtimes cannot survive untracked.

# Pass 4 — Make Post-Spawn Cancellation Own Cleanup

## Current Problem

After `reinit_fn()` returns, the coordinator checks cancellation. If cancelled, it returns an error while the replacement runtime may already be spawned and installed.

## Required Reinit Result

Replace the closure return type:

```rust
Result<Arc<LspClient>, LspError>
```

with a structured unpublished replacement:

```rust
pub struct UnpublishedReplacement {
    pub client: Arc<LspClient>,
    pub generation: u64,
    pub runtime_installed: bool,
}
```

Prefer including a cleanup handle or service callback rather than relying on a boolean:

```rust
pub struct ReplacementCleanup {
    key: String,
    generation: u64,
}
```

The coordinator must be able to terminate the exact replacement generation before publication.

## Cancellation Boundaries

After spawn and before publication:

```text
if cancelled:
  terminate replacement runtime with FailedPublication reason
  remove runtime only if generation matches
  ensure replacement client is not in clients map
  return InitializationCancelled
```

After publication but before replay/readiness:

```text
if cancelled:
  terminate published replacement
  remove client if generation matches
  preserve previous stable state if possible
  return InitializationCancelled
```

Do not leave a live replacement in an ambiguous state.

## Generation-Scoped Removal

Client removal also needs a generation check. Add or use a helper:

```rust
async fn remove_client_if_generation(
    &self,
    key: &str,
    generation: u64,
) -> Option<Arc<LspClient>>;
```

The client must expose its bound generation or publication metadata must store it alongside the client.

## Tests

### `cancellation_after_spawn_reaps_unpublished_replacement`

### `cancellation_after_publication_removes_matching_generation_only`

### `cleanup_does_not_remove_newer_client`

## Acceptance Criteria

- Every post-spawn cancellation path terminates the exact runtime.
- No cancelled replacement remains live or published.

# Pass 5 — Make Manual and Automatic Restart Use One Supersession Path

## Goal

Remove duplicated ownership logic between `manual_restart_client()` and `restart_client_with_trigger()`.

## Required Refactor

Create one internal method:

```rust
async fn restart_client_owned(
    &self,
    key: &str,
    trigger: RestartTrigger,
    retained_diagnostics: Option<HashMap<String, DiagnosticCacheEntry>>,
) -> Result<RestartOutcome, LspError>;
```

Ownership behavior:

```text
Automatic:
  acquire or coalesce
Manual:
  cancel automatic owner
  wait for completion
  acquire manual owner
```

Manual teardown happens only inside the owned path.

This prevents future divergence between the two entry points.

## Tests

- automatic/automatic coalescing;
- manual/automatic supersession;
- manual/manual collision;
- shutdown cancellation.

## Acceptance Criteria

- One ownership implementation exists.
- Manual teardown cannot bypass ownership.

# Pass 6 — Represent Degraded Restart as a Live Outcome

## Current Problem

A replacement that initializes, replays documents, and remains operational but times out on readiness is transitioned to `Degraded` and then returned as `LaunchFailed`.

This conflates a live degraded client with a failed restart.

## Required Result Type

Add:

```rust
pub enum RestartOutcome {
    Ready,
    Degraded { reason: String },
}
```

Change coordinator return type:

```rust
Result<RestartOutcome, LspError>
```

Semantics:

```text
process launch/init/replay failure -> Err
readiness success -> Ok(Ready)
readiness timeout with live client -> Ok(Degraded { reason })
```

## Budget Semantics

Recommended:

- `Ready` sets `last_healthy_at`;
- `Degraded` does not set `last_healthy_at`;
- the consumed restart attempt remains consumed;
- no immediate new restart is scheduled solely because readiness degraded;
- a later process exit continues from the existing budget.

## Caller Behavior

Exit handler/manual caller should log degraded outcome distinctly and not report “restart failed.”

## Tests

### `degraded_restart_returns_live_outcome`

### `degraded_restart_does_not_reset_budget`

### `degraded_client_remains_published`

## Acceptance Criteria

- Degraded is not encoded as `LaunchFailed`.
- Live degraded client remains usable and observable.

# Pass 7 — Correct the Empty Diagnostics Readiness Test

## Current Problem

The named test `empty_diagnostics_readiness_passes` currently uses a non-LSP process and observes no diagnostics, so it returns false. The name and claimed coverage are incorrect.

## Required Test

Use the scripted LSP server to emit:

```json
{
  "jsonrpc": "2.0",
  "method": "textDocument/publishDiagnostics",
  "params": {
    "uri": "file:///tmp/test.rs",
    "diagnostics": []
  }
}
```

Assert:

```text
wait_for_first_diagnostics(timeout) == true
diagnostic cache contains an entry with an empty vector
readiness policy returns Ready
```

Rename the existing no-notification test to:

```text
missing_diagnostics_readiness_times_out
```

## Acceptance Criteria

- Empty diagnostics notification is proven to satisfy readiness.
- Missing notification is separately proven to time out.

# Pass 8 — Audit Restart Policy Override Documentation and Construction

## Current Problem

`LspClientDescriptor::from_profile()` still says readiness and restart policies have no user override. The current signature also does not visibly accept a resolved restart policy.

## Required Audit

Trace the full descriptor creation path and determine where validated user restart policy is applied.

Required final design:

```rust
LspClientDescriptor::from_resolved(
    ...,
    readiness_policy: LspReadinessPolicy,
    restart_policy: LspRestartPolicy,
)
```

or an equivalent builder that receives already merged policies.

Do not construct a descriptor with profile policy and mutate it later unless that mutation is explicit, tested, and documented.

## Test

### `user_restart_policy_reaches_descriptor`

Parse config with non-default values and assert the stored descriptor exactly matches them.

## Acceptance Criteria

- Descriptor construction receives final resolved policy.
- Documentation no longer claims no user override.

# Pass 9 — Add Final Race Tests

Add focused tests to `supervisor_restart_stdio.rs` or a new dedicated file.

Required tests:

```text
manual_waits_for_cancelled_automatic_completion
manual_timeout_does_not_touch_current_client
cancel_after_spawn_reaps_replacement
rejected_runtime_install_reaps_loser
old_owner_completion_cannot_release_new_owner
manual_revalidates_generation_after_wait
degraded_restart_is_live_outcome
empty_publish_diagnostics_satisfies_readiness
```

Run race tests repeatedly:

```bash
for i in 1 2 3 4 5 6 7 8 9 10; do
  cargo test -p egglsp --features lsp-test-support \
    --test supervisor_restart_stdio -- --test-threads=1 || exit 1
done
```

Also run with default parallelism at least three times.

## Acceptance Criteria

- No flaky process-count, generation, or ownership failures.
- No leaked fake-server processes after any test.

# Pass 10 — Documentation and Phase Closure

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- cancellation vs completion distinction;
- restart owner completion signaling;
- manual supersession sequence;
- explicit runtime-install outcomes;
- post-spawn cleanup ownership;
- degraded restart outcome semantics;
- exact descriptor policy resolution;
- empty diagnostics readiness behavior.

Do not mark Phase 3 closed until all required tests pass.

# Exact Execution Order

1. Add owner completion signaling.
2. Fix manual supersession ordering.
3. Add explicit runtime-install result.
4. Add post-spawn cancellation cleanup.
5. Consolidate restart entry paths.
6. Add `RestartOutcome` and degraded semantics.
7. Correct readiness tests.
8. Audit descriptor policy resolution.
9. Add repeated race tests.
10. Update documentation and close Phase 3.

# Required Verification

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p egglsp --features lsp-test-support --lib
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --workspace --all-features
```

Real-server tests when binaries are available:

```bash
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer --nocapture

cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- basedpyright --nocapture
```

# Final Invariant Checklist

- [ ] Cancellation does not release ownership.
- [ ] Manual restart waits for cancelled owner completion.
- [ ] Manual timeout preserves current client/runtime.
- [ ] Runtime-install rejection is explicit.
- [ ] Rejected runtime is terminated and reaped.
- [ ] Post-spawn cancellation cleans up replacement.
- [ ] Client/runtime removal is generation-scoped.
- [ ] One restart ownership path exists.
- [ ] Degraded restart returns a live outcome.
- [ ] Empty diagnostics notification satisfies readiness.
- [ ] User restart policy reaches descriptor unchanged.
- [ ] Race tests pass ten serial runs and three parallel runs.
- [ ] No fake-server child remains after tests.
- [ ] Documentation accurately marks Phase 3 complete.

# Recommended Commit Sequence

```text
1. fix(egglsp): separate restart cancellation from owner completion
2. fix(egglsp): acquire manual ownership before client teardown
3. refactor(egglsp): make runtime installation outcomes explicit
4. fix(egglsp): clean up cancelled unpublished replacements
5. refactor(egglsp): unify restart ownership entry paths
6. refactor(egglsp): return live degraded restart outcomes
7. test(egglsp): prove empty diagnostics readiness
8. fix(egglsp): resolve user restart policy in descriptors
9. test(egglsp): add final restart race coverage
10. docs(lsp): close Phase 3 ownership and cleanup invariants
```

# Handoff Result

After this plan is complete, automatic and manual restart should be safe to treat as production-ready rather than experimental. Every replacement process will have a single owner, cancellation will be distinguished from completion, losing or cancelled runtimes will be deterministically reaped, degraded readiness will be modeled accurately, and Phase 3 can be closed without remaining lifecycle caveats.
