# LSP Phase 3 Final Corrective Pass: Runtime Ownership, Generation Safety, Restart Configuration, and Evidence Freshness

## Purpose

Complete the remaining Phase 3 work after:

```text
90366961b461fc774b2d651b307d8f6dc4ccb9c7
cb6d4f54156672b6b30c1a75f8e94adf22942633
```

Those commits established most of the required Phase 3 structure:

- valid real-server `initialize` / `initialized` handshakes;
- typed Rust and Python fixtures with semantic positions;
- enforceable compatibility requirements;
- `LspProcessRuntime` and explicit process intent;
- automatic exit-receiver activation through `LspService::new_arc`;
- per-key generation tracking;
- persisted restart descriptors;
- a consolidated restart coordinator;
- document replay using preserved versions;
- operational health snapshots;
- diagnostic generation fields;
- readiness tracking primitives;
- deterministic supervisor/restart tests;
- pinned Tier 1 CI jobs.

The architecture is now credible, but several lifecycle and metadata paths are only partially integrated. The remaining defects are tightly coupled, so this pass must be executed in the order given below.

This plan is written for a smaller implementation model. Do not improvise a broad redesign. Make one pass at a time, run the listed tests, and do not begin the next pass until the current pass satisfies its acceptance criteria.

## Completion Definition

Phase 3 is complete only when all of the following are true:

1. `LspService` is the sole coordinator of protocol shutdown, runtime intent, process wait, force kill, and reap.
2. A normal shutdown is classified as expected before the server can exit.
3. A hung server is force-killed and reaped under a bounded deadline.
4. Exactly one authoritative generation value exists per client key.
5. A runtime, exit event, health snapshot, and diagnostic entry all use the same generation.
6. Old monitors cannot remove or overwrite a newer runtime.
7. Second and later restart cycles work, not just the first restart.
8. Manual restart terminates the old live runtime before publishing a replacement.
9. Automatic restart is disabled by default but can be enabled through normal production configuration.
10. Cold start and restart resolve initialization options, workspace configuration, readiness policy, and restart policy identically.
11. Restart budgets and `reset_after_healthy` semantics are deterministic and bounded.
12. Diagnostics emitted by a live client carry the real server generation.
13. Diagnostics retained across restart are marked stale until the replacement server produces new evidence.
14. Progress-based readiness cannot report immediate success before any progress observation.
15. Operational-state lookup does not create or restart a client as a side effect.
16. Crash stderr remains visible after the runtime is removed.
17. Real-server compatibility reports contain the real capability snapshot and real stderr tail.
18. All scripted, composite, real-server, workspace, and documentation checks pass.

## Primary Files

Expect to modify these files:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/src/config.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/health.rs
crates/egglsp/src/restart.rs
crates/egglsp/src/runtime.rs
crates/egglsp/src/service.rs
crates/egglsp/src/supervisor.rs
crates/egglsp/tests/real_server_smoke.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
crates/egglsp/tests/production_service_stdio.rs
tests/lsp_composite_stdio.rs
src/lsp/semantic_context.rs
src/lsp/hunk_nav_collector.rs
src/lsp/hunk_nav_prompt.rs
src/security/workflow/report.rs
src/tool/lsp.rs
.github/workflows/lsp-real-server.yml
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Possible new test files are acceptable, but do not create additional production modules unless an existing module becomes unmanageably large.

## Non-Goals

Do not implement any of the following during this pass:

- Tier 2 language servers;
- multi-root or multi-workspace support;
- incremental text synchronization;
- pull diagnostics;
- arbitrary request replay;
- restart jitter;
- a TUI health dashboard;
- automatic server installation in tests;
- workspace-edit execution;
- a model-facing `/restart-lsp` command;
- performance tuning unrelated to lifecycle correctness;
- general refactors outside the LSP subsystem.

# Pass 0 — Establish a Reproducible Baseline

## Goal

Record the current state before changing lifecycle code. This prevents unrelated failures from being attributed to this pass.

## Required Commands

Run from the repository root:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo test -p egglsp --features lsp-test-support --lib
cargo test -p egglsp --features lsp-test-support --test supervisor_restart_stdio
cargo test --features lsp-test-support --test lsp_composite_stdio
```

Also run the existing provider transcript tests and record any known pre-existing failures without fixing them unless this pass changes the same code:

```bash
cargo test --test provider_transcripts
```

## Required Record

Add a short implementation note to the eventual commit message or handoff summary containing:

```text
baseline command
baseline result
pre-existing unrelated failures
```

Do not modify production code in this pass.

## Acceptance Criteria

- The current scripted restart suite result is known.
- Any unrelated failing tests are explicitly recorded.
- The working tree contains no accidental changes before Pass 1.

# Pass 1 — Make Generation State Authoritative and Generation-Aware

## Current Problem

Generation is stored in both:

```text
LspService::generation_map
OperationalServerState::generation
```

Initial publication updates both independently. Restart publication updates `generation_map`, while the reinitialization closure derives the runtime generation from `OperationalServerState::generation`. This can make the second restart runtime publish an exit event with an older generation than the authoritative map.

The runtime map is also keyed only by client key. An older monitor removes `runtime_map[key]` unconditionally when it exits, so it can remove the handle belonging to a newer generation.

## Required Design

Use exactly one authoritative per-key generation source.

Preferred implementation:

```rust
generation_map: Arc<Mutex<HashMap<String, u64>>>
```

Remove `OperationalServerState::generation`, or convert it into a derived snapshot field that is never independently mutated. Do not retain two writable generation stores.

Add helpers in `LspService`:

```rust
async fn generation_for_key(&self, key: &str) -> u64;
async fn set_generation(&self, key: &str, generation: u64);
async fn next_generation(&self, key: &str) -> u64;
```

`next_generation` must perform read-increment-write while holding one `generation_map` lock.

Generation rules:

```text
never initialized: 0
first published runtime: 1
first replacement runtime: 2
second replacement runtime: 3
...
```

Do not derive a runtime generation from operational state.

## Runtime Map Entry

Replace the ambiguous value:

```rust
HashMap<String, LspProcessRuntime>
```

with either:

```rust
struct RuntimeEntry {
    generation: u64,
    runtime: LspProcessRuntime,
}
```

or rely on `runtime.generation()` but always compare it before mutation.

Add generation-aware helpers:

```rust
async fn install_runtime_if_current(
    runtime_map: &RuntimeMap,
    key: &str,
    generation: u64,
    runtime: LspProcessRuntime,
);

async fn remove_runtime_if_generation(
    runtime_map: &RuntimeMap,
    key: &str,
    generation: u64,
) -> Option<LspProcessRuntime>;
```

An old monitor must not remove a newer runtime.

## Publication Ordering

For cold start and restart, use this order:

```text
1. complete process construction and LSP handshake;
2. compute the next authoritative generation once;
3. bind the generation to the client;
4. install the client and generation atomically enough that no exit handler sees a mismatched pair;
5. spawn/install the runtime using that exact generation;
6. publish operational state;
7. notify waiters.
```

If strict atomicity across separate locks is not feasible, enforce a documented lock order and ensure the monitor cannot publish an exit before `generation_map` contains its generation.

A simple safe pattern is:

```text
set generation
start monitor/runtime
publish client
```

provided failed publication explicitly terminates the just-created runtime and does not leave it installed.

## Remove Duplicate Generation Increments

The restart reinitialization closure and restart coordinator currently both participate in generation changes. Choose one owner.

Required owner:

```text
restart coordinator publication path
```

The closure should create and initialize a client but must not independently increment generation or publish it as authoritative.

Return enough data from the closure for the coordinator to complete publication, for example:

```rust
struct ReinitializedClient {
    client: Arc<LspClient>,
    process_parts_or_monitor_input: ...
}
```

If changing the closure return type is too invasive, the closure may install the runtime only after receiving a generation argument from the coordinator. It must not calculate the generation itself.

## Tests

Add deterministic tests to `supervisor_restart_stdio.rs`:

### `two_consecutive_restarts_use_monotonic_generations`

Sequence:

```text
generation 1 starts
process 1 crashes
generation 2 starts
process 2 crashes
generation 3 starts
```

Assert:

```text
generation_for_key == 3
runtime generation == 3 while live
state reaches Ready after each successful replacement
third runtime exit is not rejected as stale
```

### `old_monitor_cannot_remove_new_runtime`

Arrange an old generation exit notification after generation 2 is installed. Assert:

```text
runtime_map still contains generation 2
operational state remains associated with generation 2
no replacement is triggered by the stale event
```

### `generation_is_identical_across_health_and_exit_event`

Assert that the live runtime, health snapshot, and synthetic/current exit event all report the same generation.

## Acceptance Criteria

- `OperationalServerState` is not an independent generation authority.
- No restart path increments generation twice.
- Second and third restart cycles work.
- Runtime-map removal checks generation.
- Existing stale-exit test still passes.
- New repeated-restart tests pass deterministically three consecutive runs.

# Pass 2 — Integrate Runtime Intent, Wait, Force Kill, and Reap into Service Shutdown

## Current Problem

`LspClient::shutdown()` currently performs only protocol traffic:

```text
shutdown request
exit notification
```

`LspService::shutdown_inner()` calls this method but does not set runtime intent before the process can exit, does not await the runtime exit event, and does not force-kill a hung runtime.

As a result:

- normal shutdown can be classified as unexpected;
- hung servers may survive until incidental drop behavior;
- `shutdown_all()` can report `Stopped` without proving process termination;
- runtime-map cleanup is not coordinated with shutdown.

## Required Separation of Responsibilities

Keep protocol shutdown separate from process shutdown.

Rename or document the client method as protocol-only:

```rust
pub async fn request_protocol_shutdown(&self) -> Result<(), LspError>
```

It should:

```text
send shutdown request
send exit notification
```

It must not wait on a child handle because the runtime owns the child.

All runtime intent and process termination belong to `LspService`.

## Add a Service Runtime-Termination Helper

Add a helper similar to:

```rust
enum RuntimeTerminationReason {
    ServiceShutdown,
    ManualRestart,
    DisposalAfterFailedPublication,
}

struct RuntimeTerminationOutcome {
    exited: bool,
    forced: bool,
    event: Option<LspProcessExitEvent>,
}

async fn terminate_runtime(
    &self,
    key: &str,
    expected_generation: u64,
    client: Option<Arc<LspClient>>,
    deadline: Instant,
    reason: RuntimeTerminationReason,
) -> RuntimeTerminationOutcome;
```

Required sequence:

```text
1. look up runtime only if generation matches;
2. call runtime.request_graceful_shutdown() BEFORE sending protocol shutdown;
3. send protocol shutdown through the client under a bounded timeout;
4. await runtime.wait_for_exit() until the graceful deadline;
5. if not exited, call runtime.request_force_kill();
6. await runtime.wait_for_exit() until the absolute deadline;
7. remove runtime only if the key still maps to expected_generation;
8. return the observed exit event/outcome.
```

The intent must be set before the server can react to `shutdown` and exit.

## Shutdown-All Ordering

Update `shutdown_inner()`:

```text
1. lifecycle -> ShuttingDown
2. cancel initialization and restart work
3. snapshot clients with their authoritative generations
4. concurrently terminate each matching runtime using one global deadline
5. clear client/document-owner maps after termination attempts
6. preserve final exit metadata
7. lifecycle -> Stopped
```

Do not simply drain the client map before runtime termination if the helper needs the client to send protocol shutdown.

Use a single aggregate deadline. Do not multiply total shutdown duration by client count.

## Forced Finalization

If the global deadline expires:

```text
request_force_kill on every still-matching runtime
perform one final bounded wait/reap attempt
log any runtime that still did not publish an exit event
clear maps only after recording the invariant failure
```

Do not claim successful graceful shutdown when forced finalization occurred.

## Expected Exit Handling

During service shutdown, the exit receiver may process expected events concurrently.

Required behavior:

- expected exit must never trigger restart;
- transition to `Stopped` is allowed;
- a late expected exit after lifecycle is already `Stopped` is harmless;
- stale expected events remain ignored by generation comparison.

## Tests

Update/add scripted tests:

### `graceful_shutdown_sets_intent_before_exit`

Assert that the observed event has:

```text
expected == true
status == 0 when the fake server exits normally
no replacement process
runtime removed for matching generation
```

### `hung_process_is_force_killed_and_reaped`

The fake server ignores protocol shutdown. Assert:

```text
shutdown_all returns within configured bound
runtime intent reaches ForceKillRequested
process no longer exists / exit event observed
runtime_map no longer contains the generation
no restart occurs
```

### `shutdown_does_not_report_stopped_with_live_runtime`

After `shutdown_all()` returns, assert:

```text
client map empty
runtime map empty
lifecycle Stopped
all start-counter PIDs have terminated when the platform permits checking
```

### `concurrent_shutdown_callers_share_completion`

Retain or extend the existing concurrent shutdown test so both callers return only after runtime termination is complete.

## Acceptance Criteria

- Service shutdown sets graceful intent before protocol shutdown.
- Hung process path calls force kill.
- Runtime owner remains the only child waiter.
- `shutdown_all()` is bounded independently of client count.
- No expected exit triggers restart.
- Runtime map is empty after normal shutdown.

# Pass 3 — Make Restart Own Old-Runtime Termination and Correct Retry Budgets

## Current Problem

Automatic restart usually begins after the old process has exited, but manual restart can replace a live client without terminating its runtime. Retry accounting also combines an external `restart_attempts` counter with a fresh internal `1..=max_attempts` loop, allowing repeated crash cycles to receive a new full budget indefinitely.

## Restart Trigger API

Keep:

```rust
RestartTrigger::Automatic
RestartTrigger::Manual
```

Expose one service entry point that accepts the trigger internally:

```rust
async fn restart_client_with_trigger(
    &self,
    key: &str,
    trigger: RestartTrigger,
) -> Result<(), LspError>;
```

`restart_client()` may remain a public/manual wrapper if required, but automatic exit handling must call the same coordinator with `Automatic`.

## Old Runtime Termination

Before spawning a replacement:

```text
Automatic:
  verify the old runtime already exited or terminate any still-live matching runtime.

Manual:
  call terminate_runtime(... ManualRestart ...) and require the old runtime to be gone before replacement publication.
```

Do not publish a new runtime while the old generation remains installed under the same key.

## Restart Task Ownership

Add explicit per-key restart task ownership if it does not already exist:

```rust
restart_tasks: Arc<Mutex<HashMap<String, RestartTaskControl>>>
```

or an equivalent cancellation token map.

Requirements:

- at most one restart coordinator per key;
- service shutdown cancels all scheduled/backoff restart work;
- a manual restart supersedes/cancels an older automatic restart;
- a newer generation causes the older coordinator to exit with `ServerRestarted`;
- task completion removes only its own control entry.

Do not rely solely on polling lifecycle every 50 ms if a cancellation token can make cancellation immediate.

## Retry Budget Semantics

Define the counter unambiguously:

```text
restart_attempts = consecutive failed replacement launches since the last healthy reset
```

Recommended behavior:

1. On unexpected exit, determine whether the prior generation was healthy for at least `reset_after_healthy`.
2. If yes, reset `restart_attempts = 0`.
3. Before each replacement spawn, increment the counter.
4. Stop when the next increment would exceed `max_attempts`.
5. A successful replacement does not immediately reset the counter; it records `last_healthy_at`.
6. The counter resets only after the replacement remains healthy for `reset_after_healthy`, evaluated lazily on the next crash or by a bounded timer.

Do not run a new full `1..=max_attempts` loop for every crash if the prior generation did not remain healthy long enough to reset the budget.

The coordinator should use the remaining budget:

```rust
let used = shared.restart_attempts(key).await;
let remaining = policy.max_attempts.saturating_sub(used);
```

Each actual spawn consumes one attempt.

## State Transitions

Use these transitions:

```text
Ready/Degraded -> RestartScheduled { attempt, delay_ms }
RestartScheduled -> Restarting { attempt }
Restarting -> Initializing
Initializing -> Indexing/Ready/Degraded
any retry failure with budget remaining -> RestartScheduled
budget exhausted -> Failed { reason }
shutdown/manual cancellation -> Stopped or prior newer-generation state
```

All transitions must go through the centralized validator.

Do not swallow transition errors. A transition error is an invariant failure and should fail deterministic tests.

## Tests

### `manual_restart_terminates_old_runtime_before_new_spawn`

Assert:

```text
old runtime receives expected shutdown intent
old process exits or is killed
new process starts only afterward
new generation increments once
only one live runtime remains
```

### `rapid_crash_loop_exhausts_shared_budget`

Configure `max_attempts = 3`. Make every replacement initialize successfully and crash before `reset_after_healthy`.

Assert:

```text
only three replacement spawns occur in total
final state Failed
no fourth replacement is scheduled
```

### `healthy_interval_resets_restart_budget`

Use paused Tokio time if practical. Let a replacement remain healthy beyond `reset_after_healthy`, then crash.

Assert:

```text
restart_attempts resets
another full configured budget is available
```

### `manual_restart_cancels_scheduled_automatic_restart`

Assert that only one replacement process is spawned and the manual request wins deterministically.

### `shutdown_cancels_restart_without_polling_delay`

Use a long backoff and verify shutdown completes without waiting for the backoff duration.

## Acceptance Criteria

- Manual restart cannot leak a live old process.
- Only one restart coordinator exists per key.
- Restart budget spans rapid crash cycles.
- `reset_after_healthy` is implemented and tested.
- Shutdown cancels scheduled restart work immediately.

# Pass 4 — Resolve One Descriptor for Cold Start, Restart, and Production Configuration

## Current Problem

Cold start derives initialization options and workspace configuration directly from `LspConfig`, while the persisted descriptor applies compatibility-profile defaults. Restart then uses the descriptor. The same server can therefore receive different settings on cold start and restart.

Restart policy is also disabled by default and currently can be changed only by test-only descriptor mutation.

## Descriptor-First Initialization

Resolve the launch specification and build `LspClientDescriptor` before constructing the client.

Use these descriptor fields for both cold start and restart:

```rust
descriptor.launch_spec
descriptor.initialization_options
descriptor.workspace_configuration
descriptor.readiness_policy
descriptor.restart_policy
```

Cold-start sequence:

```text
resolve launch spec
resolve descriptor from profile + user overrides
construct client from descriptor.launch_spec and descriptor.workspace_configuration
initialize with descriptor.initialization_options
send initialized
wait using descriptor.readiness_policy
publish descriptor unchanged
```

Remove parallel config-resolution logic after descriptor construction.

## Production Restart Configuration

Extend `LspRule::Active` with an optional restart section. Use a serde-friendly configuration type rather than serializing `Duration` directly if existing config conventions prefer integer milliseconds.

Recommended schema:

```toml
[lsp.rust-analyzer]
command = ["rust-analyzer"]
extensions = ["rs"]

[lsp.rust-analyzer.restart]
mode = "on-unexpected-exit"
max_attempts = 3
initial_backoff_ms = 500
max_backoff_ms = 8000
reset_after_healthy_ms = 60000
```

Suggested Rust type:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LspRestartPolicyConfig {
    pub mode: Option<LspRestartModeConfig>,
    pub max_attempts: Option<u32>,
    pub initial_backoff_ms: Option<u64>,
    pub max_backoff_ms: Option<u64>,
    pub reset_after_healthy_ms: Option<u64>,
}
```

Merge priority:

```text
explicit user restart config
compatibility profile restart policy
LspRestartPolicy::default()
```

Default must remain disabled.

Validate:

```text
max_attempts > 0 when mode is enabled
initial_backoff <= max_backoff
all millisecond values fit Duration
unknown mode is a config error
```

Do not silently normalize invalid values unless the existing config layer consistently does so.

## Constructor Cleanup

Production call sites should use:

```rust
LspService::new_arc(config)
```

Deprecate, make private, or clearly restrict bare `LspService::new(config)` so new production callers cannot accidentally create a service without automatic exit-receiver activation.

Preferred outcome:

```rust
pub fn new(config: LspConfig) -> Arc<Self>
```

If that public API change is too broad, use:

```rust
pub(crate) fn new_bare_for_test(...)
pub fn new_arc(...)
```

and migrate all production callers.

## Tests

### Config parsing tests

Cover:

```text
restart omitted -> disabled default
restart explicitly disabled
restart enabled with all values
partial override inherits profile/default values
invalid mode rejected
initial_backoff > max_backoff rejected
zero max_attempts rejected when enabled
```

### `cold_start_and_restart_use_identical_descriptor_settings`

Have the fake server capture:

```text
initialize initializationOptions
workspace/configuration response
launch args/env
```

Crash and restart, then assert generation 1 and generation 2 received identical resolved values.

### Constructor activation test

Create the service through every public production constructor and prove that an unexpected exit reaches the exit handler without an explicit `start_exit_receiver()` call.

## Acceptance Criteria

- One descriptor-resolution path exists.
- Cold start and restart use identical resolved settings.
- Restart is configurable in ordinary TOML/config data.
- Default restart remains disabled.
- No production constructor leaves exit handling inactive.

# Pass 5 — Bind Diagnostic Evidence to Real Generations and Preserve Stale Evidence

## Current Problem

New `DiagnosticCacheEntry` values are inserted with:

```text
server_generation = 0
post_restart = false
```

The replacement client has an empty diagnostics map, so marking the new client's cache stale does not preserve evidence from the old client. Root consumers propagate the metadata correctly, but the metadata source is wrong or absent.

## Client Generation Binding

Add an atomic generation field to `LspClient`:

```rust
server_generation: Arc<AtomicU64>
```

Add methods:

```rust
pub fn server_generation(&self) -> u64;
pub async fn bind_server_generation(&self, generation: u64, post_restart: bool);
```

`bind_server_generation` must:

```text
store generation atomically
update any diagnostics received before publication to the same generation
set post_restart for existing entries when generation > 1
```

The background notification dispatcher must capture the same atomic and stamp each new `DiagnosticCacheEntry` using its current value.

Do not infer `post_restart` from `generation > 0`. The first published generation is `1` and is not post-restart. Use:

```text
post_restart = generation > 1
```

unless the key's initial generation semantics are intentionally changed and documented.

## Preserve Old Diagnostics Across Restart

Before replacing/removing the old client, snapshot its diagnostic cache:

```rust
pub async fn diagnostic_cache_snapshot(
    &self,
) -> HashMap<String, DiagnosticCacheEntry>;
```

After constructing and binding the new client, install retained entries as stale:

```rust
pub async fn install_retained_diagnostics(
    &self,
    entries: HashMap<String, DiagnosticCacheEntry>,
    previous_generation: u64,
);
```

Required retained-entry values:

```text
server_generation = previous_generation
post_restart = previous_entry.post_restart
received_at preserved
content_version preserved
source preserved
```

Freshness classification must compare the entry generation against the current client generation, not rely only on `diagnostics_invalidated_at`.

Update classification API as needed:

```rust
classify_diagnostic_freshness(
    entry,
    last_content_change,
    invalidated_at,
    current_generation,
)
```

Required logic:

```text
no entry -> Unavailable
entry.server_generation != current_generation -> Stale
invalidated after entry -> Stale
content changed after entry -> PossiblyStale
otherwise -> Fresh
```

When the replacement server publishes new diagnostics, it overwrites the retained entry with:

```text
server_generation = current generation
post_restart = true
freshness = Fresh or PossiblyStale according to content timing
```

## Empty Diagnostics Are Evidence

A pushed empty diagnostics vector is still current evidence that the file is clean. Preserve the entry and metadata. Do not treat an empty vector as unavailable.

## Service-Level Generation Access

`LspDiagnosticSnapshot.server_generation` must come from the cache entry that produced the evidence. It must not be overwritten later by a best-effort root scan.

Remove or simplify any root-side lookup that attempts to reconstruct diagnostic generation from client keys. The snapshot is authoritative.

## Tests

### `initial_generation_diagnostics_are_generation_one`

Assert:

```text
server_generation == Some(1)
post_restart == false
freshness Fresh
```

### `retained_diagnostics_are_stale_after_restart`

Generation 1 publishes diagnostics, then crashes. Before generation 2 publishes anything, assert:

```text
diagnostics still present
server_generation == Some(1)
post_restart reflects origin generation
freshness Stale
usable_evidence == false
```

### `new_diagnostics_replace_stale_evidence`

Generation 2 publishes diagnostics. Assert:

```text
server_generation == Some(2)
post_restart == true
freshness Fresh
usable_evidence == true
```

### `empty_post_restart_diagnostics_clear_old_errors`

Generation 1 reports an error. Generation 2 reports an empty diagnostics list. Assert old errors disappear and the empty entry is fresh generation-2 evidence.

### `second_restart_preserves_generation_metadata`

Repeat through generation 3 and assert no entry falls back to generation 0.

## Acceptance Criteria

- Live diagnostics never use generation 0 after publication.
- First generation is not marked post-restart.
- Retained old diagnostics are visible but stale.
- New diagnostics replace retained stale evidence.
- Semantic, hunk, and security collectors receive authoritative snapshot metadata.

# Pass 6 — Correct Readiness Semantics and Remove Side Effects from Health Lookup

## Current Problem

`wait_for_progress_end()` returns success immediately when the active token set is empty, even when no progress notification has ever been observed. Rust-analyzer can therefore be marked ready before indexing begins.

The real-server harness implements progress readiness as a fixed sleep rather than the production readiness method.

The semantic collector calls `get_or_create_client()` merely to inspect operational state, which can start a new initialization or race with a restart.

## Progress Tracker State

Extend `ProgressState` with explicit observation state:

```rust
pub struct ProgressState {
    active_tokens: HashSet<String>,
    last_progress_at: Option<Instant>,
    observed_begin: bool,
    observed_any: bool,
    completed_cycle: bool,
}
```

Update on notifications:

```text
begin -> observed_any=true, observed_begin=true, add token
report -> observed_any=true
end -> observed_any=true, remove token; if observed_begin && empty, completed_cycle=true
```

## Readiness Policy Semantics

For `WaitForProgressEndOrTimeout`:

```text
success if a progress cycle was observed and completed
success if server explicitly indicates readiness through a documented equivalent signal
otherwise timeout -> Degraded
```

Do not return immediate success solely because `active_tokens` is empty.

To accommodate servers that emit no progress, add a clearly named profile policy rather than hidden fallback behavior. For example:

```rust
WaitForProgressCycleOrTimeout { timeout }
```

If retaining the current enum name, document the exact semantics.

For `WaitForDiagnosticsOrTimeout`, the first `publishDiagnostics` notification counts even if the diagnostics vector is empty.

## Readiness State Publication

Use:

```text
Initializing -> Indexing while waiting
Indexing -> Ready on observed readiness
Indexing -> Degraded on timeout
```

Do not publish `Ready` before the policy result is known.

Restart must run the same readiness policy after replay. Do not transition directly to `Ready` immediately after replay.

## Real-Server Harness

Call the same client readiness primitives used by production:

```text
wait_for_first_diagnostics
wait_for_progress_end / revised method
warmup delay
initialized-is-ready
```

Do not substitute a fixed sleep for progress readiness.

The compatibility report must distinguish:

```text
readiness reached
readiness timed out / degraded
```

A timeout may be a known limitation only when explicitly classified by the profile; otherwise it must fail the required readiness check.

## Non-Creating Operational Lookup

Add service helpers that never create a client:

```rust
async fn key_for_existing_file(&self, file: &Path) -> Option<String>;
async fn operational_state_for_file(&self, file: &Path) -> Option<LspOperationalState>;
async fn operational_health_for_file(&self, file: &Path) -> Option<LspOperationalHealthSnapshot>;
```

Resolve using, in priority order:

```text
document owner
existing descriptor/root match
existing client/root match
```

Do not call `get_or_create_client()`.

Update semantic, hunk, security, and tool-facing health-note paths to use non-creating lookup.

## Tests

### `progress_readiness_does_not_succeed_before_begin`

No progress notifications. Assert timeout produces `Degraded`.

### `progress_readiness_completes_after_begin_end`

Send begin, report, end. Assert `Ready` only after end.

### `empty_diagnostics_notification_satisfies_diagnostics_readiness`

Publish an empty diagnostics vector and assert readiness succeeds.

### `restart_runs_readiness_policy_before_ready`

During generation 2, delay readiness signal. Assert state remains `Indexing` until the signal arrives.

### `operational_lookup_does_not_spawn_client`

Call the semantic/health lookup for an uninitialized file. Assert:

```text
process start count remains zero
client map remains empty
no descriptor is created
```

### `failed_state_note_does_not_reinitialize`

Set a key to `Failed`, collect semantic context, and assert no new process starts.

## Acceptance Criteria

- Progress readiness requires observed progress completion.
- Empty diagnostics can satisfy diagnostics readiness.
- Restart and cold start use the same readiness code.
- Operational note lookup has no initialization side effect.
- Real-server harness uses production readiness primitives.

# Pass 7 — Persist Exit Metadata and Repair Health Snapshots

## Current Problem

The runtime captures stderr and includes it in `LspProcessExitEvent`, but the monitor removes the runtime before health snapshot construction. `OperationalServerState` does not retain the last exit event or stderr tail, so crash diagnostics disappear when they are most useful.

## Operational Metadata

Extend `OperationalServerState` with persisted observational fields:

```rust
last_exit: Option<LspProcessExitSummary>
last_error: Option<String>
last_stderr_tail: Vec<String>
last_exit_at: Option<Instant or serializable timestamp strategy>
```

Suggested summary:

```rust
#[derive(Debug, Clone)]
struct LspProcessExitSummary {
    generation: u64,
    status: Option<i32>,
    signal: Option<i32>,
    expected: bool,
    reason: String,
}
```

When handling a current-generation exit event, persist the summary and stderr tail before any restart or state transition.

Do not overwrite newer-generation metadata with a stale event.

## Health Snapshot Rules

`operational_health_snapshot()` must return:

```text
live runtime stderr tail when a matching live runtime exists
otherwise persisted last_stderr_tail
```

`last_error` should include meaningful failure/degraded/replay/restart-exhaustion reasons, not only `Failed { reason }`.

Generation must always come from `generation_map`.

When no live client exists:

```text
transport = None
pending_requests = 0
message/diagnostic ages = None
operational state retained
last exit metadata retained
stderr retained
```

## Runtime Removal Ordering

Monitor sequence:

```text
receive exit event
forward/persist event
remove runtime if generation matches
```

Do not remove the only stderr source before the service has persisted it.

A practical pattern is for the monitor to send the event first and let the service remove the matching runtime after recording metadata.

## Tests

### `crash_stderr_survives_runtime_removal`

Fake server writes unique stderr and crashes. Assert health snapshot after removal still contains the line.

### `stale_exit_does_not_replace_newer_exit_metadata`

Persist generation-2 metadata, inject generation-1 event, and assert generation-2 metadata remains unchanged.

### `health_snapshot_exists_during_restart_and_failure`

Assert snapshots remain available in:

```text
RestartScheduled
Restarting
Initializing
Degraded
Failed
Stopped
```

## Acceptance Criteria

- Crash stderr remains visible after process exit.
- Stale events cannot overwrite current exit metadata.
- Health snapshots remain available without a live client.
- Runtime removal is generation-aware and occurs after metadata persistence.

# Pass 8 — Finish the Real-Server Harness and Compatibility Report

## Current Problem

The harness uses real capabilities for checks but writes `LspCapabilitySnapshot::default()` into the report. It starts with an empty stderr vector and does not attach the direct client to the runtime abstraction. The whole-test timeout constant is unused, expected symbol names are not enforced, and version capture is blocking without a timeout.

## Report Real Capabilities

Pass the actual capability snapshot into `build_report()`:

```rust
fn build_report(
    ...,
    capabilities: LspCapabilitySnapshot,
    ...,
)
```

Every early-return report should include whatever capability data is actually available:

```text
before initialize -> default/none is acceptable
successful initialize -> real snapshot required
```

Consider making report capabilities optional if that expresses pre-initialize failure more accurately.

## Runtime/Stderr Integration

The direct smoke-test client should use the same runtime owner as production, or a small test harness wrapper around it.

Required behavior:

```text
spawn process
construct client
start runtime with generation 1
run handshake and checks
on failure, capture runtime.stderr_snapshot
on shutdown, set graceful intent before protocol shutdown
wait, force kill if needed
write stderr tail into report
```

Do not introduce a second child waiter in the test harness.

## Whole-Test Timeout

Wrap each server test body in an outer timeout:

```rust
tokio::time::timeout(TEST_TIMEOUT, run_smoke_suite(...))
```

On timeout:

```text
request force kill
capture stderr
write a failure report if practical
fail the test with server/stage details
```

## Version Capture

Replace blocking unbounded `std::process::Command::output()` with bounded execution:

```rust
tokio::process::Command
tokio::time::timeout
```

Version capture failure should be report metadata, not a test failure unless the server binary itself cannot be executed.

## Symbol Assertions

Use `expected_symbol_names`.

For advertised document symbols:

```text
flatten returned symbols
assert at least the profile/fixture-required names
include missing names in failure detail
```

Do not require incidental implementation-specific symbols.

## Reference Assertions

For references:

```text
require at least one location for single-file fixture
require at least two distinct URIs for the Python cross-file fixture
```

Do not pass merely because the request returned `Ok(Vec::new())`.

## Readiness Reporting

Report:

```text
policy used
elapsed time
signal observed
whether result was Ready or Degraded
```

If the report schema is kept compact, put these details into the readiness check detail field.

## CI Pinning

Keep both Rust jobs on the same pinned toolchain.

For rust-analyzer, either:

- install a specifically pinned standalone release; or
- rename the step so it does not claim a date/version that is not independently enforced.

Keep basedpyright pinned.

Add the workflow path itself to the trigger paths if changes to the workflow should test it:

```yaml
- '.github/workflows/lsp-real-server.yml'
```

## Tests

Add unit tests for report construction:

```text
real capabilities survive serialization
stderr tail survives serialization
required symbol failure fails assertion
outer timeout classifies failure
unsupported binary remains a skip, not a pass
```

## Acceptance Criteria

- Successful reports contain real capabilities.
- Failure reports contain captured stderr when available.
- Whole test is bounded.
- Version capture is bounded.
- Expected symbols are enforced.
- References cannot pass with an empty result.
- CI labels match actual pinning behavior.

# Pass 9 — Workflow Consumers, Documentation, and API Cleanup

## Consumer Audit

Search for all uses of:

```text
LspService::new(
LspService::new_arc(
start_exit_receiver
restart_client
operational_state_for_key
get_or_create_client used only for status
server_generation
post_restart
LspOperationalHealthSnapshot
```

Update every production path to the final APIs.

Consumer requirements:

- semantic context reports `RestartScheduled`, `Restarting`, `Indexing`, `Degraded`, `Failed`, and `Stopped` accurately;
- security context never treats stale diagnostics as high-confidence evidence;
- hunk navigation notes when semantic evidence is stale or unavailable;
- tool output includes operational notes without causing initialization side effects;
- no caller reconstructs generation independently;
- no caller directly accesses child handles or runtime internals.

## API Cleanup

Remove or deprecate obsolete APIs and fields:

```text
OperationalServerState::generation if still present
manual start_exit_receiver requirement
legacy child-wait helpers no longer used outside direct-client tests
vestigial restart_enabled/max_restart_attempts mirrors
old duplicate restart helpers
comments claiming placeholder behavior that is now implemented
```

Do not remove a compatibility alias until all repository callers are migrated.

## Documentation

Update:

```text
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
```

Document:

1. process ownership and shutdown sequence;
2. generation authority and stale-event handling;
3. runtime-map generation checks;
4. automatic and manual restart behavior;
5. restart configuration schema and disabled default;
6. retry-budget and healthy-reset semantics;
7. document replay and preserved versions;
8. diagnostic generation and stale-evidence rules;
9. readiness policies and degraded outcomes;
10. operational health and persisted stderr;
11. Tier 1 compatibility-test expectations;
12. exact verification commands.

Do not describe unimplemented Tier 2 support as complete.

## Acceptance Criteria

- No production caller uses a bare inactive service constructor.
- No status lookup creates a client.
- Obsolete generation/restart mirrors are removed.
- Documentation matches final code behavior.
- The LSP skill gives smaller models the correct lifecycle invariants.

# Pass 10 — Final Verification Matrix

Run formatting first:

```bash
cargo fmt --check
```

Run focused library tests:

```bash
cargo test -p egglsp --features lsp-test-support --lib
```

Run all egglsp integration tests:

```bash
cargo test -p egglsp --features lsp-test-support --tests
```

Run the supervisor suite three consecutive times:

```bash
for i in 1 2 3; do
  cargo test -p egglsp --features lsp-test-support --test supervisor_restart_stdio || exit 1
done
```

Run root composite tests:

```bash
cargo test --features lsp-test-support --test lsp_composite_stdio
cargo test --test lsp
cargo test --test security_review_runner
```

Run workspace checks:

```bash
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If the repository currently has accepted pre-existing clippy warnings, use the repository's established clippy policy rather than silently weakening it. Record any unrelated warnings separately.

Run Tier 1 real-server tests when binaries are available:

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

Inspect generated reports manually for:

```text
server version
real capabilities
readiness result
required check statuses
stderr tail
known limitations
```

## Required Final Invariant Tests

Do not declare completion unless explicit tests prove:

```text
cold init generation 1
restart generation 2
second restart generation 3
stale generation-1 event ignored after generation 2
old monitor cannot remove generation-2 runtime
normal shutdown event expected=true
hung shutdown force-kills and reaps
manual restart terminates old live runtime
restart budget exhausts across rapid crash cycles
healthy interval resets budget
shutdown cancels scheduled restart
replay preserves latest text and version
diagnostics stale across restart
diagnostics fresh after new push
empty diagnostics clear retained errors
progress readiness waits for observed begin/end
operational lookup does not spawn
crash stderr survives runtime removal
cold start and restart use identical descriptor settings
```

# Implementation Discipline for a Smaller Model

Follow these rules throughout the pass:

1. Modify one lifecycle concept per commit or coherent patch.
2. Do not combine generation repair with diagnostics repair in the same initial edit.
3. Run the focused tests listed in each pass before proceeding.
4. Prefer small service helpers over repeated lock-and-map logic.
5. Never hold a service map lock across process I/O, protocol I/O, sleeps, or runtime waits.
6. Preserve the documented lock order.
7. Never call `child.wait()` outside `LspProcessRuntime` after Pass 2.
8. Never remove `runtime_map[key]` without checking generation.
9. Never mutate generation outside the authoritative helper.
10. Never transition operational state through direct assignment after initialization; use the validator.
11. Never report a diagnostic as fresh when its entry generation differs from the live generation.
12. Never use `get_or_create_client()` for observational/status-only queries.
13. Never allow a manual restart to overlap a live old runtime.
14. Treat transition errors, duplicate runtime installation, and generation mismatch as invariant failures in tests.
15. Keep restart disabled by default.
16. Do not weaken required real-server checks to make CI pass.
17. When a real server has a legitimate limitation, classify and document it explicitly.
18. Update comments immediately when implementation behavior changes; stale lifecycle comments are dangerous.

# Recommended Commit Sequence

Use a sequence close to:

```text
1. fix(egglsp): unify per-key generation and generation-aware runtime map
2. fix(egglsp): integrate runtime intent kill and reap into shutdown
3. fix(egglsp): terminate old runtime and enforce restart budgets
4. feat(egglsp): resolve descriptor-first startup and restart config
5. fix(egglsp): bind diagnostics to real server generations
6. fix(egglsp): preserve stale diagnostics across restart
7. fix(egglsp): correct readiness and non-creating health lookup
8. fix(egglsp): persist exit metadata and stderr in health state
9. test(egglsp): harden real-server reports and repeated restart coverage
10. docs(lsp): document completed Phase 3 lifecycle invariants
```

Smaller commits are acceptable. Avoid one giant implementation commit because generation, runtime ownership, and diagnostic freshness need to be reviewable independently.

# Final Handoff Checklist

Before handing the implementation back, provide:

```text
commits created
files changed per pass
tests run with exact results
real-server versions tested
generated compatibility report paths
known unrelated failures
remaining limitations, if any
```

Phase 3 should be considered closed only when the completion definition and final invariant tests above are satisfied. The next roadmap phase should not begin while runtime termination, repeated restart generations, production restart configuration, or diagnostic freshness remain unresolved.
