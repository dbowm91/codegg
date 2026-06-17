# LSP Phase 3 Final Race and Harness Closure

## Purpose

Close the remaining Phase 3 defects after the implementation series ending at:

```text
a42f54bc1d612473dc5cf3b0e65c388b53527898
```

The previous closure pass successfully established:

- generation-aware runtime bookkeeping;
- service-owned graceful shutdown, force kill, and reap;
- one replacement-generation decision point;
- manual restart support;
- shared restart-attempt accounting;
- retained diagnostics across restart;
- observed-cycle progress readiness;
- validated restart-policy conversion;
- real capability reporting;
- repeated-generation tests;
- local compile, Clippy, and formatting cleanup.

The remaining work is narrower. It consists of race elimination, correcting manual-restart sequencing, enforcing exact restart budgets, applying readiness after replacement, finishing the real-server runtime harness, tightening reference assertions, removing the unsupervised constructor path, and aligning retained diagnostic metadata and documentation.

This plan is tailored for a smaller implementation model. Execute each pass in order. Do not broaden scope.

## Phase 3 Final Completion Definition

Phase 3 is closed only when all of the following hold:

1. At most one restart coordinator may own a client key at a time.
2. Concurrent manual and automatic restarts cannot spawn two replacement processes.
3. A losing restart attempt cannot leave an untracked process alive.
4. Manual restart sends protocol shutdown through the old client before force kill.
5. Manual restart preserves the old client's diagnostics for stale-evidence transfer.
6. A restart policy with `max_attempts = N` permits exactly N replacement launches, never N+1.
7. Replacement clients execute the configured readiness policy before entering `Ready`.
8. The real-server harness uses the same readiness primitives as production.
9. The real-server harness owns the child through `LspProcessRuntime` and captures stderr.
10. Advertised references fail when the fixture returns zero locations.
11. No public production constructor can create an unsupervised service.
12. Shutdown requests force kill for every remaining runtime even after the nominal deadline expires.
13. Retained diagnostic entries preserve their original generation and `post_restart` metadata.
14. Production comments and architecture documentation describe the implemented behavior accurately.
15. Deterministic race tests pass repeatedly.

## Primary Files

```text
crates/egglsp/src/client.rs
crates/egglsp/src/restart.rs
crates/egglsp/src/runtime.rs
crates/egglsp/src/service.rs
crates/egglsp/tests/real_server_smoke.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not implement:

- Tier 2 language servers;
- new LSP methods;
- multi-root workspaces;
- incremental synchronization;
- pull diagnostics;
- restart jitter;
- TUI health dashboards;
- model-facing restart commands;
- unrelated architectural refactors.

# Pass 1 — Serialize Restart Ownership Per Client Key

## Current Problem

`restart_client_with_trigger()` can be invoked concurrently for the same key. Two callers may:

```text
read the same current generation
calculate the same replacement generation
spawn two replacement processes
race to publish the same generation
```

`install_runtime()` rejects a same/newer generation, but its return value is ignored. A losing process can remain alive without an entry in `runtime_map`.

## Required Design

Add explicit per-key restart ownership to `LspService`.

Preferred structure:

```rust
struct RestartTaskControl {
    token: CancellationToken,
    owner_id: u64,
    trigger: RestartTrigger,
}

restart_tasks: Arc<Mutex<HashMap<String, RestartTaskControl>>>,
restart_owner_counter: Arc<AtomicU64>,
```

A simpler per-key `tokio::sync::Mutex<()>` map is acceptable if it also supports:

- cancellation during shutdown;
- manual restart superseding a scheduled automatic restart;
- owner-safe cleanup.

Preferred behavior:

```text
automatic restart starts -> installs owner token
second automatic restart -> joins/rejects as already in progress
manual restart starts -> cancels existing automatic owner, waits for ownership, then proceeds
shutdown -> cancels all owners immediately
owner completion -> removes only its own owner_id
```

Do not use a global restart mutex. Different client keys must still restart independently.

## Required API Shape

Add helpers similar to:

```rust
async fn acquire_restart_ownership(
    &self,
    key: &str,
    trigger: RestartTrigger,
) -> Result<RestartLease, LspError>;

struct RestartLease {
    key: String,
    owner_id: u64,
    token: CancellationToken,
    restart_tasks: Arc<Mutex<HashMap<String, RestartTaskControl>>>,
}
```

`RestartLease::drop` or an explicit `release()` must remove the entry only when `owner_id` still matches.

Manual trigger rules:

```text
manual cancels automatic
manual does not silently cancel another manual call; reject or serialize deterministically
```

Automatic trigger rules:

```text
if owner exists, return InitializationCancelled("restart already in progress")
or join the existing task; choose one behavior and test it
```

## Reinit Cancellation

Pass the lease cancellation token into backoff and reinitialization boundaries.

Check cancellation:

```text
before backoff
inside backoff
before process spawn
immediately after process spawn
before client publication
before document replay
before readiness
```

If cancellation occurs after process spawn but before publication, terminate the just-created runtime/process through the failed-publication cleanup path.

## Runtime Installation Contract

Change `install_runtime()` to return an explicit result:

```rust
enum RuntimeInstallResult {
    Installed { replaced: Option<RuntimeEntry> },
    Rejected { existing_generation: u64 },
}
```

Callers must handle `Rejected` by terminating the newly spawned runtime immediately. Ignoring the result is forbidden.

## Tests

### `concurrent_automatic_restarts_spawn_one_replacement`

Use a barrier so two calls enter at the same time. Assert:

```text
one replacement process start
one published generation
one live runtime
one caller succeeds; the other deterministically joins or receives already-in-progress
```

### `manual_restart_supersedes_automatic_backoff`

Start an automatic restart in long backoff, invoke manual restart, and assert:

```text
automatic token cancelled
manual obtains ownership
only one replacement starts
```

### `rejected_runtime_install_terminates_losing_process`

Force a same-generation install race. Assert the rejected runtime receives force-kill and no child survives.

### `restart_lease_cleanup_is_owner_safe`

Old owner cleanup must not remove a newer owner entry.

## Acceptance Criteria

- One coordinator per key.
- No same-generation double-spawn can leave an untracked process.
- Manual restart can supersede automatic restart deterministically.
- Shutdown can cancel restart work immediately.

# Pass 2 — Correct Manual Restart Sequencing

## Current Problem

The current manual path removes the old client before termination and passes `None` to `terminate_runtime()`. Therefore:

- no protocol `shutdown`/`exit` is sent;
- the runtime often requires force kill;
- diagnostic snapshot lookup sees no old client and returns empty;
- stale evidence is lost.

## Required Sequence

Use this exact order:

```text
1. acquire restart ownership
2. read current generation
3. clone old client from clients map
4. snapshot diagnostics directly from old client
5. terminate old runtime with Some(old_client.clone())
6. verify old runtime removed or force-killed
7. remove old client from map only after termination begins/completes
8. invoke coordinator with retained diagnostics already captured
9. publish replacement
10. release restart ownership
```

Do not rely on `snapshot_diagnostics_for_restart()` after the old client has been removed.

## Coordinator Input

Prefer extending the coordinator call with explicit retained diagnostics:

```rust
restart_client_coordinator(
    ...,
    retained_diagnostics: HashMap<String, DiagnosticCacheEntry>,
    ...,
)
```

For automatic restart, snapshot before old-client replacement. For manual restart, pass the snapshot captured before termination.

This is clearer than having the coordinator reach back into the live client map after manual teardown.

## Graceful Manual Shutdown

Pass `Some(old_client)` into `terminate_runtime()`.

Assert runtime intent transitions:

```text
Running -> GracefulShutdownRequested
```

Only force kill if the server fails to exit before the graceful deadline.

## Tests

### `manual_restart_sends_protocol_shutdown_before_replacement`

Fake server records `shutdown` and `exit`. Assert both arrive before the second process starts.

### `manual_restart_preserves_diagnostics_as_stale`

Generation 1 publishes diagnostics. Manual restart. Before generation 2 publishes diagnostics, assert retained generation-1 evidence remains visible and stale.

### `manual_restart_does_not_force_kill_cooperative_server`

Assert `forced == false` for a cooperative fake server.

## Acceptance Criteria

- Manual restart uses protocol shutdown.
- Old diagnostics survive manual restart.
- Cooperative server exits gracefully.

# Pass 3 — Enforce Exact Restart Attempt Budgets

## Current Problem

The caller increments `restart_attempts` before invoking the coordinator. If the value becomes `max_attempts + 1`, the coordinator still launches once before checking exhaustion.

## Required Budget Semantics

Define:

```text
restart_attempts = number of replacement process launches consumed since last healthy reset
```

The check must occur before every spawn.

Recommended helper:

```rust
async fn reserve_restart_attempt(
    &self,
    key: &str,
    max_attempts: u32,
) -> Result<u32, LspError>;
```

Atomic behavior under one operational-state write lock:

```text
if restart_attempts >= max_attempts -> exhausted, do not increment
else increment and return new attempt number
```

Move increment ownership into the coordinator or the reservation helper. Do not increment once in the caller and again after failures.

Preferred coordinator loop:

```text
reserve attempt
if exhausted -> Failed, return
backoff based on reserved attempt
spawn exactly one replacement
on init failure -> loop and reserve another attempt
on success -> return; counter remains consumed until healthy reset
```

## Healthy Reset

Retain lazy reset on the next unexpected exit, but ensure `last_healthy_at` is set only after the replacement reaches final readiness state according to policy.

Decide whether `Degraded` counts as healthy enough to reset. Recommended:

```text
Ready counts
Degraded does not reset unless profile explicitly permits it
```

Document the choice.

## Tests

### `max_three_allows_exactly_three_replacement_spawns`

Use a fake server that starts successfully and crashes before healthy reset. Assert exactly three replacement starts, then `Failed`.

### `attempt_four_is_rejected_before_spawn`

Seed counter at three with max three. Invoke restart and assert process-start counter does not change.

### `failed_initialization_consumes_attempt`

Initialization failure counts as one replacement launch.

### `healthy_reset_restores_full_budget`

Use paused time or a controlled timestamp.

## Acceptance Criteria

- No N+1 launch.
- Attempt reservation is atomic.
- Tests count real spawn attempts, not only counter values.

# Pass 4 — Apply Readiness Policy After Restart

## Current Problem

After replay, the restart coordinator transitions directly to `Ready`. `descriptor.readiness_policy` is not executed.

## Shared Readiness Helper

Extract or reuse one helper for cold start and restart:

```rust
async fn wait_for_client_readiness(
    client: &LspClient,
    policy: &LspReadinessPolicy,
) -> ReadinessResult;
```

Required semantics:

```text
InitializedIsReady -> Ready immediately
WarmupDelay -> sleep bounded duration, then Ready
WaitForDiagnosticsOrTimeout -> Ready on first push, including empty diagnostics; Degraded on timeout
WaitForProgressEndOrTimeout -> Ready only after completed progress cycle; Degraded on timeout
```

Use the same helper in both initialization paths.

## Restart State Sequence

Required sequence:

```text
Restarting
Initializing
replay documents
Indexing
wait readiness
Ready or Degraded
```

Do not set `last_healthy_at` before readiness completes.

If readiness returns `Degraded`, preserve the live client and runtime but do not treat it as fully healthy for restart-budget reset.

## Tests

### `restart_stays_indexing_until_progress_cycle_completes`

### `restart_degrades_when_progress_never_arrives`

### `empty_diagnostics_push_satisfies_restart_readiness`

### `cold_start_and_restart_use_same_readiness_helper`

Use a shared fake client transcript and assert identical outcomes.

## Acceptance Criteria

- Replacement never enters `Ready` before policy completion.
- Cold start and restart share one readiness implementation.

# Pass 5 — Finish the Real-Server Runtime Harness

## Current Problem

The smoke harness still:

- leaves `stderr_tail` as an empty vector;
- does not wrap the child in `LspProcessRuntime`;
- calls protocol-only `client.shutdown()`;
- uses a fixed sleep for progress readiness;
- records readiness as passing even when the signal was not observed.

## Runtime-Backed Harness Structure

After `LspClient::new_with_launch_spec()`:

```text
take child via take_child_for_runtime()
take stderr via take_stderr_for_runtime()
spawn LspProcessRuntime generation 1
retain runtime handle for entire suite
```

Create a harness struct:

```rust
struct RealServerHarness {
    client: Arc<LspClient>,
    runtime: LspProcessRuntime,
}
```

Add bounded cleanup:

```rust
async fn shutdown_and_collect(
    &self,
    graceful_timeout: Duration,
    absolute_timeout: Duration,
) -> HarnessShutdownResult;
```

Sequence:

```text
runtime.request_graceful_shutdown()
client.request_protocol_shutdown()
await runtime exit
force kill on timeout
capture stderr tail
```

All early-return paths after process launch must call cleanup before building the report.

## Readiness Check

Use production primitives:

```text
WaitForDiagnosticsOrTimeout -> client.wait_for_first_diagnostics()
WaitForProgressEndOrTimeout -> client.wait_for_progress_end()
WarmupDelay -> bounded sleep
InitializedIsReady -> immediate pass
```

Record:

```text
Passing when signal observed
Failing or PassingWithKnownLimits only according to profile requirement when timed out
```

Do not unconditionally record readiness as passing.

## Stderr Reporting

At final report construction:

```rust
stderr_tail = runtime.stderr_tail_capped(20)
```

On timeout/failure, include captured stderr in `stage_timeout_error()`.

## Tests

Use a lightweight fake real-server process where practical:

### `smoke_harness_captures_stderr`

### `smoke_harness_force_kills_hung_server`

### `progress_readiness_failure_is_reported`

### `empty_diagnostics_readiness_passes`

## Acceptance Criteria

- Report stderr is real, not always empty.
- Harness has one child waiter.
- Readiness reflects actual observed behavior.

# Pass 6 — Fail Advertised References on Empty Results

## Current Problem

The standard references check marks `Ok(Vec::new())` as passing.

## Required Rule

For advertised references:

```text
zero locations -> RequiredIfAdvertised failure
one or more locations -> pass
```

Rust fixture must require at least one reference.

Python cross-file fixture retains the stricter requirement:

```text
at least two distinct URIs
```

## Tests

### `empty_references_fail_required_if_advertised`

### `single_rust_reference_passes`

### `python_cross_file_references_still_require_two_uris`

## Acceptance Criteria

- No `references (0 found)` passing report exists.

# Pass 7 — Remove the Public Unsupervised Constructor

## Current Problem

`pub fn LspService::new(...) -> Self` remains available and produces a service with no cyclic self-reference. Documentation alone does not prevent production misuse.

## Required API

Preferred change:

```rust
pub fn new(config: LspConfig) -> Arc<Self>
```

Use the current `new_arc` implementation internally and optionally retain:

```rust
#[cfg(test)]
pub(crate) fn new_bare_for_test(config: LspConfig) -> Self
```

Then either:

```text
remove new_arc and migrate callers to new
```

or temporarily retain `new_arc` as a deprecated alias returning `Arc<Self>`.

Do not leave any public constructor returning unsupervised `Self`.

## Caller Audit

Search the full repository for:

```text
LspService::new(
LspService::new_arc(
```

Migrate all production callers.

## Tests

### `all_public_constructors_auto_start_supervision`

### `unexpected_exit_is_observed_without_manual_receiver_start`

## Acceptance Criteria

- Public construction always wires `self_ref`.
- Bare constructor is test-only or removed.

# Pass 8 — Guarantee Force-Kill Intent on Deadline Exhaustion

## Current Problem

If the absolute shutdown deadline has already expired, straggler runtime handling logs and returns without requesting force kill. The service can then publish `Stopped` with a live runtime.

## Required Finalization Behavior

At forced finalization:

```text
snapshot every remaining RuntimeEntry
request_force_kill() on all entries immediately, without awaiting first
then perform best-effort bounded waits if any budget remains
log unresolved runtimes
```

Even with zero remaining wait budget, force-kill intent must be sent.

Before transitioning to `Stopped`, inspect `runtime_map`:

```text
empty -> normal postcondition
non-empty -> severe invariant log including key/generation/intent
```

Prefer not to clear runtime entries blindly; retain them until process-owner tasks publish exit, unless service destruction requires final map drain.

## Tests

### `expired_shutdown_deadline_still_requests_force_kill`

### `shutdown_logs_or_exposes_unresolved_runtime_count`

### `normal_shutdown_ends_with_empty_runtime_map`

## Acceptance Criteria

- Deadline exhaustion never skips force-kill intent.
- `Stopped` publication includes an explicit unresolved-runtime check.

# Pass 9 — Preserve Retained Diagnostic Origin Metadata

## Current Problem

The coordinator installs retained diagnostics preserving original generation and `post_restart`, then calls `mark_diagnostics_stale_for_key()`, which rewrites those fields.

Generation mismatch already makes the entries stale. The rewrite destroys provenance.

## Required Change

After installing retained diagnostics, do not call `mark_diagnostics_stale_for_key()`.

Freshness must be derived by:

```text
entry.server_generation != current_client_generation -> Stale
```

Preserve:

```text
entry.server_generation
entry.post_restart
entry.received_at
entry.content_version
entry.source
```

Deprecate or remove `mark_diagnostics_stale_for_key()` if no other valid caller remains.

## Tests

### `generation_two_diagnostic_retained_into_generation_three_keeps_post_restart_true`

### `retained_generation_is_not_rewritten`

### `freshness_is_stale_due_to_generation_mismatch`

## Acceptance Criteria

- Staleness classification is derived, not encoded by destructive metadata rewrite.

# Pass 10 — Update Production Documentation and Comments

## Required Corrections

Update `restart.rs` module docs and comments that still describe:

```text
for attempt in 1..=max_attempts
no per-user restart override
caller-owned increment semantics that no longer apply after reservation refactor
```

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

- per-key restart ownership;
- cancellation and manual supersession;
- exact attempt reservation semantics;
- manual restart graceful sequence;
- restart readiness state transitions;
- real-server runtime-backed harness;
- supervised constructor invariant;
- retained diagnostic provenance;
- forced shutdown behavior after deadline exhaustion.

Do not mark Phase 3 complete until all final verification gates pass.

# Required Verification Matrix

## Formatting and static checks

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Focused library tests

```bash
cargo test -p egglsp --features lsp-test-support --lib
```

## Restart and supervision tests

```bash
cargo test -p egglsp --features lsp-test-support \
  --test supervisor_restart_stdio -- --test-threads=1

for i in 1 2 3 4 5; do
  cargo test -p egglsp --features lsp-test-support \
    --test supervisor_restart_stdio -- --test-threads=1 || exit 1
done
```

## Composite workflow tests

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --test lsp
cargo test --test security_review_runner
```

## Real-server tests

```bash
cargo test -p egglsp \
  --features lsp-real-server-tests \
  --test real_server_smoke \
  -- rust_analyzer --nocapture

cargo test -p egglsp \
  --features lsp-real-server-tests \
  --test real_server_smoke \
  -- basedpyright --nocapture
```

Inspect generated reports for:

```text
real stderr tail when emitted
actual readiness result
non-empty advertised references
real capability snapshot
bounded shutdown result
```

## Workspace tests

```bash
cargo test --workspace --all-features
```

# Mandatory Final Invariant Tests

- [ ] Two concurrent automatic restarts spawn one replacement.
- [ ] Manual restart cancels/supersedes automatic restart.
- [ ] Rejected runtime installation kills the losing process.
- [ ] Manual restart sends shutdown and exit before replacement spawn.
- [ ] Manual restart preserves stale diagnostics.
- [ ] Max-attempt policy allows exactly N replacement launches.
- [ ] Attempt N+1 is rejected before spawn.
- [ ] Replacement remains `Indexing` until readiness completes.
- [ ] Replacement becomes `Degraded` on readiness timeout.
- [ ] Real-server progress readiness uses `wait_for_progress_end`.
- [ ] Real-server reports include stderr when emitted.
- [ ] Zero references fails advertised-reference check.
- [ ] Every public constructor is supervised.
- [ ] Expired shutdown deadline still sends force-kill intent.
- [ ] Retained diagnostics preserve original generation and post-restart metadata.
- [ ] All race tests pass five consecutive runs.

# Recommended Commit Sequence

```text
1. fix(egglsp): serialize restart ownership per client key
2. fix(egglsp): preserve old client through manual restart shutdown
3. fix(egglsp): reserve restart attempts before spawn
4. fix(egglsp): apply readiness policy after replacement
5. test(egglsp): use runtime-backed real-server harness
6. test(egglsp): reject empty advertised references
7. refactor(egglsp): remove public unsupervised constructor
8. fix(egglsp): force-kill stragglers after deadline exhaustion
9. fix(egglsp): preserve retained diagnostic provenance
10. docs(lsp): close final Phase 3 race and harness gaps
```

# Implementation Discipline for a Smaller Model

1. Implement one pass at a time.
2. Run focused tests after each pass.
3. Do not hold service locks across process I/O, backoff, readiness waits, or protocol requests.
4. Never ignore `install_runtime()` failure or rejection.
5. Never spawn a replacement without restart ownership.
6. Never increment restart attempts outside the atomic reservation helper after Pass 3.
7. Never remove the old client before capturing diagnostics and sending protocol shutdown.
8. Never transition replacement to `Ready` without executing readiness policy.
9. Never build a real-server report before bounded runtime cleanup.
10. Never rewrite retained diagnostic provenance to manufacture staleness.
11. Keep automatic restart disabled by default.
12. Treat transition errors and unresolved live runtimes as invariant failures in tests.

# Final Handoff Output

The implementation handoff must report:

```text
commits created
files changed by pass
exact test commands and results
race tests repeated-run results
real server versions tested
compatibility report paths
known unrelated failures
remaining limitations, if any
```

After this plan passes, Phase 3 may be marked complete and the roadmap can move to broader LSP capability work rather than supervision and lifecycle correction.
