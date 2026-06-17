# LSP Phase 3 Final Closure: Runtime Termination, Generation-Safe Supervision, Restart Budgets, Readiness, and Fresh Evidence

## Purpose

Finish the remaining Phase 3 work after:

```text
21d2def237438cb596f913650752da9cd9c14399
c018c3c45c61d4841979f1e069ab7834076f38c3
```

The repository now has most Phase 3 structures in place:

- valid real-server initialization and semantic checks;
- one service-level generation map;
- repeated generation tests through generation 3;
- persisted exit metadata and stderr fallback in health snapshots;
- user-facing restart configuration fields;
- client-side diagnostic generation stamping;
- progress observation state;
- compatibility reports containing real capability snapshots;
- bounded real-server version and whole-suite execution;
- document replay and ownership restoration;
- a consolidated restart coordinator.

The remaining issues are concentrated in lifecycle integration and evidence semantics. These are not broad feature gaps. They are correctness gaps in process ownership, runtime-map safety, restart budgeting, readiness, and diagnostic retention.

This plan is intentionally narrow and ordered for a smaller implementation model. Do not add new language servers, new LSP operations, or new UI features during this pass.

## Phase 3 Closure Definition

Phase 3 is complete only when all of the following are true:

1. `LspService::shutdown_all()` sets graceful runtime intent before protocol shutdown.
2. Every live runtime is awaited, force-killed on timeout, and reaped before shutdown completes or reports forced failure.
3. Runtime-map insertion and removal are generation-aware.
4. An old monitor cannot remove a newer runtime.
5. Only one layer assigns a replacement generation.
6. Manual restart terminates the old live runtime before starting a replacement.
7. Restart attempts are consumed across rapid crash cycles and reset only after the configured healthy interval.
8. Old diagnostics are transferred to the replacement client and remain visible as stale evidence.
9. New diagnostics replace stale evidence with the current generation.
10. `post_restart` means generation 2 or later everywhere.
11. Progress readiness requires an observed completed progress cycle.
12. The real-server harness uses production readiness primitives instead of fixed sleeps.
13. Production restart configuration is validated and demonstrably reaches the descriptor.
14. Cold start and restart use identical resolved initialization/configuration data.
15. Real-server reports capture stderr where available.
16. Advertised references fail when the fixture returns no locations.
17. No public production constructor can create a service with inactive exit supervision.
18. Documentation no longer overstates completion before these invariants pass.

## Primary Files

Likely files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/config.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/restart.rs
crates/egglsp/src/runtime.rs
crates/egglsp/src/service.rs
crates/egglsp/tests/real_server_smoke.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
crates/codegg-config/src/schema.rs
src/lsp/semantic_context.rs
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

## Non-Goals

Do not implement:

- Tier 2 servers;
- multi-root workspaces;
- pull diagnostics;
- incremental sync;
- restart jitter;
- TUI health panels;
- workspace-edit application;
- model-facing restart commands;
- automatic server installation;
- unrelated crate refactors.

# Pass 1 — Make Runtime Map Operations Generation-Aware

## Current Problem

`runtime_map` is keyed only by client key. `spawn_process_monitor()` inserts the runtime and later unconditionally removes `runtime_map[key]` after receiving the exit event. A delayed old monitor can remove a newer generation's runtime.

## Required Type

Replace:

```rust
HashMap<String, LspProcessRuntime>
```

with:

```rust
#[derive(Clone)]
struct RuntimeEntry {
    generation: u64,
    runtime: LspProcessRuntime,
}

type RuntimeMap = Arc<Mutex<HashMap<String, RuntimeEntry>>>;
```

The explicit generation field is preferred even though `LspProcessRuntime` also exposes `generation()`. It makes comparisons obvious and testable.

## Required Helpers

Add internal helpers:

```rust
async fn install_runtime(
    runtime_map: &RuntimeMap,
    key: String,
    generation: u64,
    runtime: LspProcessRuntime,
) -> Option<RuntimeEntry>;

async fn runtime_for_generation(
    runtime_map: &RuntimeMap,
    key: &str,
    generation: u64,
) -> Option<LspProcessRuntime>;

async fn remove_runtime_if_generation(
    runtime_map: &RuntimeMap,
    key: &str,
    generation: u64,
) -> Option<RuntimeEntry>;
```

Rules:

- install may replace an older generation only after the old runtime has been terminated;
- install must reject or loudly log replacement of the same/newer generation;
- removal succeeds only when the stored generation matches;
- no direct `map.remove(key)` remains in monitor or shutdown code.

## Monitor Ordering

Use:

```text
install matching runtime
await exit event
send event to service
service persists event metadata
remove matching runtime
```

If removal must remain in the monitor, perform it only after forwarding the event and only through `remove_runtime_if_generation`.

Do not remove the runtime before the service can capture stderr/exit metadata.

## Tests

Add:

### `old_monitor_cannot_remove_new_runtime`

Sequence:

```text
generation 1 monitor is delayed after process exit
generation 2 runtime is installed
generation 1 monitor resumes
```

Assert:

```text
runtime map still contains generation 2
generation 2 runtime can receive shutdown/kill intent
health remains generation 2
```

### `runtime_removal_requires_exact_generation`

Unit-test the helper directly.

## Acceptance Criteria

- No unconditional runtime-map removal remains.
- Runtime map stores generation explicitly.
- A stale monitor cannot remove the active runtime.

# Pass 2 — Integrate Runtime Intent, Wait, Kill, and Reap into Service Shutdown

## Current Problem

`shutdown_all()` currently drains clients and calls `client.shutdown()`, but does not set runtime intent, wait on `LspProcessRuntime`, force-kill hung processes, or prove runtime-map quiescence.

## Protocol/Process Separation

Rename or document:

```rust
LspClient::shutdown()
```

as protocol-only, preferably:

```rust
pub async fn request_protocol_shutdown(&self) -> Result<(), LspError>
```

It must only send:

```text
shutdown request
exit notification
```

Do not let `LspClient` wait on the child once the runtime owns it.

## Service Helper

Add:

```rust
#[derive(Debug, Clone, Copy)]
enum RuntimeTerminationReason {
    ServiceShutdown,
    ManualRestart,
    FailedPublication,
}

struct RuntimeTerminationOutcome {
    exited: bool,
    forced: bool,
    event: Option<LspProcessExitEvent>,
}

async fn terminate_runtime(
    &self,
    key: &str,
    generation: u64,
    client: Option<Arc<LspClient>>,
    graceful_deadline: Instant,
    absolute_deadline: Instant,
    reason: RuntimeTerminationReason,
) -> RuntimeTerminationOutcome;
```

Required sequence:

```text
1. lookup runtime only when generation matches;
2. runtime.request_graceful_shutdown();
3. send protocol shutdown under the graceful deadline;
4. await runtime.wait_for_exit();
5. on timeout, runtime.request_force_kill();
6. await runtime.wait_for_exit() under the absolute deadline;
7. persist event metadata if the exit receiver has not already done so;
8. remove runtime only if generation matches;
9. return whether force kill was required.
```

Intent must be set before the protocol shutdown request is sent.

## Update `shutdown_inner`

Required ordering:

```text
lifecycle -> ShuttingDown
cancel initialization/restart work
snapshot clients with authoritative generations
terminate all runtimes concurrently under one global deadline
clear clients/owners after termination attempts
force-kill any remaining matching runtimes
verify runtime_map is empty or record invariant failures
lifecycle -> Stopped
```

Do not drain the client map before the termination helper receives the client handles.

## Global Deadline

The total shutdown duration must remain bounded independently of client count. Use concurrent futures and one absolute deadline.

## Tests

### `graceful_shutdown_marks_exit_expected`

Assert:

```text
runtime intent is graceful before server exits
exit event expected == true
no restart is scheduled
runtime entry removed
```

### `hung_process_is_force_killed_and_reaped_via_shutdown_all`

Use the actual service shutdown path, not a runtime-only helper.

Assert:

```text
shutdown_all returns within deadline
force-kill intent observed
process exit event observed
runtime map empty
lifecycle Stopped
```

### `shutdown_all_leaves_no_live_runtime`

After return:

```text
client map empty
runtime map empty
no fake-server process remains
```

## Acceptance Criteria

- Runtime intent is set before protocol shutdown.
- Hung processes are killed and reaped.
- `shutdown_all()` does not report success while runtimes remain live.

# Pass 3 — Give Restart One Generation Owner

## Current Problem

The reinitialization closure increments generation and starts the monitor. The restart coordinator then sets `expected_generation + 1` again. Both currently choose the same value, but there are still two generation writers.

## Required Ownership

The restart coordinator must own replacement generation selection.

Preferred design:

```rust
let new_generation = expected_generation.saturating_add(1).max(1);
let reinitialized = reinit_fn(&descriptor, new_generation).await?;
```

Change the closure signature to accept generation:

```rust
FnMut(&LspClientDescriptor, u64) -> BoxFuture<'static, Result<Arc<LspClient>, LspError>>
```

The closure may:

- construct client;
- initialize it;
- bind the supplied generation;
- spawn/install runtime using the supplied generation.

It must not calculate or increment generation independently.

The coordinator then publishes the same generation exactly once.

## Publication Order

Use:

```text
construct and initialize replacement
bind supplied generation
install generation-aware runtime
publish client
set authoritative generation
replay documents
run readiness
publish Ready/Degraded
```

If runtime installation occurs before authoritative generation publication, ensure the monitor cannot emit an event before the map is set. A start barrier or publication helper may be needed.

## Tests

- Existing generation 1 → 2 → 3 test must remain green.
- Add an assertion that each spawned runtime generation equals the service generation at publication.
- Add a test that no generation is skipped or assigned twice.

## Acceptance Criteria

- Only one function computes replacement generation.
- Reinit closure receives, rather than derives, generation.
- Runtime, client, event, and health snapshot agree.

# Pass 4 — Terminate Old Runtime Before Manual Restart

## Current Problem

The coordinator can replace a healthy client without terminating the old runtime. This leaks the old process and allows runtime-map overwrites.

## Trigger-Aware Restart API

Use:

```rust
async fn restart_client_with_trigger(
    &self,
    key: &str,
    trigger: RestartTrigger,
) -> Result<(), LspError>;
```

Keep a public/manual wrapper only if needed.

## Manual Restart Sequence

```text
lookup current generation and client
cancel any scheduled automatic restart for key
terminate old runtime with RuntimeTerminationReason::ManualRestart
remove old client only after termination begins/completes
start replacement with generation + 1
replay documents
run readiness
publish final state
```

Automatic restart should verify the old runtime is already gone. If not, terminate it before replacement.

## Single Restart Task Per Key

Add a restart-task control map or per-key cancellation token:

```rust
restart_tasks: Arc<Mutex<HashMap<String, RestartTaskControl>>>
```

Requirements:

- one coordinator per key;
- manual restart cancels/supersedes automatic restart;
- shutdown cancels all restart tasks immediately;
- task cleanup removes only its own token/control entry.

## Tests

### `manual_restart_terminates_old_process_before_new_start`

Use process start/exit timestamps or transcript ordering.

### `manual_restart_supersedes_scheduled_automatic_restart`

Assert exactly one replacement starts.

### `manual_restart_leaves_one_runtime`

Assert runtime map contains exactly the new generation.

## Acceptance Criteria

- Manual restart cannot leave two live processes.
- Only one restart coordinator exists per key.

# Pass 5 — Make Restart Attempts Span Rapid Crash Cycles

## Current Problem

Every restart invocation gets a fresh internal `1..=max_attempts` loop. A server that restarts successfully and quickly crashes can receive an unlimited series of full retry budgets.

## Define the Counter

Use:

```text
restart_attempts = replacement process launches since the last healthy reset
```

Rules:

1. Every actual replacement spawn consumes one attempt.
2. A successful short-lived replacement does not reset the counter.
3. The counter resets only after the server remains healthy for `reset_after_healthy`.
4. The next crash evaluates whether the healthy interval was long enough and resets lazily if so.
5. When `restart_attempts >= max_attempts`, no new process is launched.

## Coordinator Changes

Remove the fresh full inner budget per invocation.

Recommended algorithm:

```rust
loop {
    let used = shared.restart_attempts(key).await;
    if used >= policy.max_attempts {
        fail_exhausted();
    }
    let attempt = shared.increment_restart_attempts(key).await;
    sleep(backoff_delay(attempt, policy));
    try one replacement launch;
    if launch/init fails, continue;
    if replacement succeeds, return Ok(());
}
```

On the next crash, the same counter continues unless healthy reset applies.

## Healthy Reset

Before scheduling restart after an unexpected exit:

```rust
if last_healthy_at.elapsed() >= reset_after_healthy {
    restart_attempts = 0;
}
```

Update `last_healthy_at` only after readiness reaches `Ready` or after a deliberate policy decision for `Degraded`.

## Tests

### `rapid_crash_loop_exhausts_shared_budget`

Each replacement initializes successfully, becomes ready briefly, and crashes before reset interval. With max 3, assert exactly three replacement launches total.

### `healthy_interval_resets_budget`

Use paused Tokio time where practical.

### `failed_initialization_and_post_ready_crash_share_budget`

One failed init plus one short-lived successful replacement should consume two attempts.

## Acceptance Criteria

- Restart exhaustion applies across crash cycles.
- Healthy interval reset is implemented and tested.

# Pass 6 — Transfer and Classify Diagnostics Across Restart

## Current Problem

The new client receives no retained diagnostics. `mark_diagnostics_stale_for_key()` runs against the newly published empty cache, so old evidence disappears instead of remaining stale.

## Required Sequence

Before old-client removal:

```rust
let retained = old_client.diagnostic_cache_snapshot().await;
```

After new client construction and generation binding:

```rust
new_client.install_retained_diagnostics("restart", retained).await;
```

Do not rewrite the retained entries to the new generation. Preserve:

```text
old server_generation
old post_restart
received_at
content_version
source
diagnostics vector, including empty vectors
```

Freshness should become stale because entry generation differs from current generation.

## Freshness Rule

Ensure classifier uses:

```text
entry.server_generation != current_generation -> Stale
```

before content timing checks.

## New Push Behavior

A new `publishDiagnostics` notification from generation N must overwrite retained generation N-1 evidence, including when the new vector is empty.

## `post_restart` Consistency

Fix every helper to use:

```rust
post_restart = generation > 1
```

In particular, `DiagnosticCacheEntry::with_generation()` currently uses `generation > 0`; correct or remove it.

## Tests

### `retained_diagnostics_visible_as_stale_after_restart`

### `new_generation_diagnostics_replace_retained_entries`

### `empty_new_diagnostics_clear_old_errors`

### `generation_one_is_not_post_restart`

### `generation_two_and_three_are_post_restart`

## Acceptance Criteria

- Diagnostics survive restart as stale evidence.
- Generation 1 is never marked post-restart.
- New pushes replace stale entries.

# Pass 7 — Make Progress Readiness Require an Observed Cycle

## Current Problem

`ProgressState` tracks `completed_cycle`, but `wait_for_progress_end()` still returns true whenever `active_tokens` is empty, including before any progress notification.

## Required Semantics

For `WaitForProgressEndOrTimeout`:

```text
success only when completed_cycle == true
```

A zero timeout succeeds only if a completed cycle was already observed.

Do not treat:

```text
active_tokens empty + observed_any false
```

as ready.

## State Reset

Progress state is per client generation. A replacement client starts with a fresh tracker. No reset API is needed if a new client is created.

## Production Readiness

During cold start and restart:

```text
Initializing -> Indexing
wait for progress cycle
Indexing -> Ready on success
Indexing -> Degraded on timeout
```

Do not transition directly to Ready before readiness completes.

## Real-Server Harness

Replace fixed sleep for progress readiness with:

```rust
client.wait_for_progress_end(timeout)
```

Record pass/failure based on the actual result.

## Tests

### `progress_wait_does_not_succeed_before_begin`

### `progress_wait_succeeds_after_begin_end`

### `progress_report_without_begin_does_not_complete_cycle`

### `restart_remains_indexing_until_generation_two_progress_ends`

## Acceptance Criteria

- Empty active-token set is not sufficient.
- Real-server rust-analyzer readiness no longer uses sleep-only logic.

# Pass 8 — Validate Restart Configuration and Prove Descriptor Parity

## Current Problem

Restart config fields exist, but validation and descriptor propagation are insufficiently demonstrated. Documentation in `LspClientDescriptor::from_profile()` still says no user restart override exists.

## Config Validation

Add validation returning a configuration error for:

```text
enabled mode with max_attempts == 0
initial_backoff_ms > max_backoff_ms
invalid/overflowing duration values
unknown restart mode via serde
```

Use a dedicated conversion:

```rust
pub fn try_to_domain(&self, base: &LspRestartPolicy) -> Result<LspRestartPolicy, LspError>
```

or an appropriate config error type.

## Merge Precedence

Required order:

```text
explicit user field
profile field
system default
```

Partial user config must inherit unspecified profile values rather than resetting to generic defaults.

Rename `merge_with_profile()` if its direction is ambiguous.

## Descriptor-First Cold Start

Construct one resolved descriptor before client construction and use its fields for cold start:

```text
launch_spec
initialization_options
workspace_configuration
readiness_policy
restart_policy
```

Restart must use the same persisted descriptor.

## Tests

### Config parsing/validation

- omitted restart -> disabled;
- explicit enabled policy;
- partial user override inherits profile;
- zero attempts rejected when enabled;
- initial backoff above maximum rejected.

### `cold_start_and_restart_receive_identical_configuration`

Fake server captures:

```text
initialize.initializationOptions
workspace/configuration response
launch args
environment
```

Assert generation 1 and generation 2 match exactly.

## Acceptance Criteria

- User restart config reaches descriptor.
- Invalid policies fail early.
- Cold start and restart are configuration-identical.

# Pass 9 — Capture Real-Server Stderr and Strengthen Reference Assertions

## Current Problem

The smoke harness reports an empty stderr vector because direct clients are not attached to `LspProcessRuntime`. Standard references also pass with zero results.

## Runtime-Backed Smoke Client

After direct client construction:

```text
take child
take stderr
spawn LspProcessRuntime with generation 1
retain runtime handle
```

Before protocol shutdown:

```text
runtime.request_graceful_shutdown()
client.request_protocol_shutdown()
await runtime exit
force kill on timeout
```

At report construction:

```rust
let stderr_tail = runtime.stderr_tail_capped(20);
```

On early failure, perform the same bounded cleanup and capture stderr before returning the report.

Do not introduce a second child waiter.

## References

For advertised references:

- Rust fixture must return at least one location;
- Python cross-file fixture must continue requiring at least two distinct URIs.

A zero-length result is a `RequiredIfAdvertised` failure.

## Tests

- report serialization preserves stderr;
- a fake real-server process that writes stderr exposes it in report;
- zero references fails required-if-advertised assertion.

## Acceptance Criteria

- Smoke reports include stderr when emitted.
- References cannot pass with zero locations.

# Pass 10 — Remove the Bare Inactive Constructor

## Current Problem

`LspService::new()` remains public and returns a bare value without a back-reference, so automatic exit supervision may never start.

## Required API

Preferred:

```rust
pub fn new(config: LspConfig) -> Arc<Self>
```

Make the current bare constructor private or test-only:

```rust
fn new_bare(config: LspConfig) -> Self
```

Alternative if API compatibility is required:

```rust
#[deprecated(note = "use LspService::new_arc")]
pub fn new(...) -> Self
```

but migrate every production caller immediately and add a compile-time/runtime test ensuring only the supervised constructor is used.

Best outcome is one public constructor that always returns `Arc<Self>` and wires `self_ref`.

## Tests

Create service through every public constructor, launch a fake server, crash it, and assert the exit handler runs without explicit activation.

## Acceptance Criteria

- No public production path creates an unsupervised service.
- No caller must remember `ensure_exit_receiver_started()`.

# Pass 11 — Correct Documentation and Test Timing

## Documentation

Until all completion gates pass, change wording from “Phase 3 complete” to “Phase 3 closure in progress.”

After completion, document:

- runtime termination sequence;
- generation-aware runtime map;
- single generation owner;
- manual restart termination;
- shared crash-cycle restart budget;
- healthy reset semantics;
- retained stale diagnostics;
- `post_restart` definition;
- observed-cycle readiness;
- validated restart configuration;
- real-server stderr capture;
- supervised constructor invariant.

## Fix Scenario Timing in Generation Test

The `generation_is_identical_across_health_and_exit_event` test should not overwrite the generation-2 scenario before generation 2 starts.

Use:

```text
write phase 2
trigger generation 1 crash
wait for process start/generation 2 readiness
write phase 3 only when generation 3 is about to start
```

Or simplify the test so it needs only one restart.

## Acceptance Criteria

- Documentation matches implementation.
- Scenario files are changed only at deterministic launch boundaries.

# Exact Execution Order

A smaller model should follow this order exactly:

1. RuntimeEntry and generation-aware map helpers.
2. Shutdown runtime termination helper and production integration.
3. Single generation owner in restart publication.
4. Manual restart old-runtime termination and restart-task ownership.
5. Shared restart budget and healthy reset.
6. Diagnostic transfer and `post_restart` correction.
7. Progress readiness and real-server readiness integration.
8. Restart config validation and descriptor parity test.
9. Real-server stderr and non-empty references.
10. Constructor cleanup.
11. Documentation and deterministic test cleanup.

Do not start diagnostic or readiness work before runtime shutdown/map invariants are green.

# Required Verification

## Focused runtime/restart tests

```bash
cargo test -p egglsp --features lsp-test-support \
  --test supervisor_restart_stdio -- --test-threads=1

for i in 1 2 3; do
  cargo test -p egglsp --features lsp-test-support \
    --test supervisor_restart_stdio || exit 1
done
```

## Library/config tests

```bash
cargo test -p egglsp --features lsp-test-support --lib
cargo test -p codegg-config
```

## Composite workflows

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --test lsp
cargo test --test security_review_runner
```

## Real servers

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

## Workspace

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

# Final Invariant Checklist

- [ ] Old monitor cannot remove new runtime.
- [ ] Runtime-map removal checks generation.
- [ ] Shutdown sets graceful intent before protocol request.
- [ ] Hung server is force-killed and reaped.
- [ ] Runtime map is empty after shutdown.
- [ ] Only coordinator chooses replacement generation.
- [ ] Manual restart terminates old runtime first.
- [ ] One restart coordinator exists per key.
- [ ] Rapid crash cycles exhaust one shared budget.
- [ ] Healthy interval resets budget.
- [ ] Old diagnostics survive as stale evidence.
- [ ] New diagnostics replace stale evidence.
- [ ] Generation 1 has `post_restart = false`.
- [ ] Generation 2+ has `post_restart = true`.
- [ ] Progress wait requires completed cycle.
- [ ] Real-server readiness uses production primitive.
- [ ] Restart config is validated.
- [ ] Partial user config inherits profile values.
- [ ] Cold start and restart receive identical resolved settings.
- [ ] Real-server reports include stderr when emitted.
- [ ] Zero references fails advertised-reference check.
- [ ] No public unsupervised service constructor remains.
- [ ] Documentation accurately states Phase 3 status.

# Recommended Commit Sequence

```text
1. fix(egglsp): make runtime map generation-aware
2. fix(egglsp): integrate runtime termination into shutdown
3. refactor(egglsp): centralize replacement generation ownership
4. fix(egglsp): terminate old runtime before manual restart
5. fix(egglsp): enforce shared restart budget and healthy reset
6. fix(egglsp): retain stale diagnostics across restart
7. fix(egglsp): require observed progress readiness
8. fix(egglsp): validate restart config and descriptor parity
9. test(egglsp): capture real-server stderr and enforce references
10. refactor(egglsp): remove unsupervised service constructor
11. docs(lsp): close Phase 3 with verified lifecycle invariants
```

# Handoff Result

After this plan is complete, Phase 3 should be genuinely closed rather than structurally complete. Codegg will have one authoritative process owner, generation-safe runtime bookkeeping, bounded shutdown and manual restart, restart budgets that cannot loop indefinitely, diagnostics that remain explicitly stale rather than disappearing, readiness based on observed server behavior, validated production restart configuration, and real-server reports that preserve both capabilities and stderr evidence.
