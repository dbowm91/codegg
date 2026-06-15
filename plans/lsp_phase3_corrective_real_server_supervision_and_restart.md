# LSP Phase 3 Corrective Pass: Real-Server Validation, Supervision, Restart Safety, and Freshness

## Purpose

Correct and complete the Phase 3 implementation introduced by:

```text
74b0961823643544d56e2fb938b0712420288435
f82bb167b31112702b329d439f89adffe332dea7
```

The current repository has useful Phase 3 foundations:

- compatibility-profile types;
- Tier 1 real-server test scaffolding;
- operational-health DTOs;
- process-exit event types;
- a bounded stderr-ring-buffer implementation;
- an open-document registry;
- initial process-monitor, restart, and replay code;
- compatibility-report JSON output;
- an opt-in real-server workflow.

However, the production and test paths are not yet reliable enough to treat Phase 3 as operationally complete. The highest-priority defects are:

1. Real-server smoke tests construct a client but never execute the protocol `initialize` / `initialized` handshake.
2. The smoke suite queries manifest files instead of language source files and uses generic hard-coded positions.
3. Required semantic checks do not fail the test when they fail.
4. Process-exit handling requires a manual `start_exit_receiver()` call and is therefore not guaranteed to run.
5. Expected-vs-unexpected exit classification incorrectly infers intent from transport failure.
6. The process monitor owns an `Arc<LspClient>` while awaiting the child indefinitely, which can retain a hung process and client.
7. Per-client generation safety is absent; stale exit events can affect newer clients.
8. Restart policy/profile data is not applied to the production service.
9. Restart reconstructs clients from a hard-coded Rust file path.
10. Two restart implementations have diverged; one fails to restore document ownership.
11. Restart failure does not reliably consume and continue the configured retry budget.
12. Health snapshots disappear during restart/failure and still contain placeholder generation/age fields.
13. Stderr capture is not integrated into process-exit events or compatibility reports.
14. Semantic diagnostic generation and post-restart fields are always placeholders.
15. Readiness policy types exist but are not backed by real readiness signals.
16. Production state assignments bypass the centralized transition validator.
17. The real-server CI workflow does not pin server versions.
18. Deterministic scripted supervisor/restart tests are missing.

This plan is tailored for a smaller model. Follow the implementation order exactly. Do not add Tier 2 servers, TUI features, or unrelated LSP operations until this corrective pass is complete.

## Target State

At completion:

1. Tier 1 real-server tests perform a valid LSP handshake and fail on required compatibility regressions.
2. Fixture metadata explicitly identifies source files and exact semantic positions.
3. Supervisor activation is automatic and internal to `LspService`.
4. Expected exit is controlled by explicit shutdown intent, not inferred from transport state.
5. One authoritative process owner can wait, cancel, kill, and reap a child.
6. Every client key has a monotonically increasing generation.
7. Stale process-exit events and stale restart publications are ignored safely.
8. Restart policy is disabled by default but configurable and actually applied.
9. Restart preserves the original server/root/launch context and works for Rust and Python.
10. One restart coordinator owns retry, backoff, replay, ownership restoration, exhaustion, and cancellation.
11. Operational health remains available during restarting and failed states.
12. Stderr tails are captured and attached to exit events and compatibility reports.
13. Diagnostic evidence carries real generation and post-restart metadata.
14. Semantic/security/hunk context exposes operational-state notes and stale evidence accurately.
15. Scripted tests prove crash, restart, exhaustion, cancellation, generation safety, and replay.
16. CI uses pinned Tier 1 server versions and produces meaningful compatibility reports.

## Scope

Primary files likely involved:

```text
crates/egglsp/src/client.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/service.rs
crates/egglsp/src/compatibility.rs
crates/egglsp/src/health.rs
crates/egglsp/src/supervisor.rs
crates/egglsp/src/document_sync.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/semantic_context.rs
crates/egglsp/src/error.rs
crates/egglsp/tests/real_server_smoke.rs
crates/egglsp/tests/production_service_stdio.rs
crates/egglsp/tests/production_protocol_stdio.rs
crates/egglsp/src/test_support.rs
src/lsp/semantic_context.rs
src/lsp/hunk_nav_collector.rs
src/lsp/hunk_nav_prompt.rs
src/tool/lsp.rs
.github/workflows/lsp-real-server.yml
architecture/lsp.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
```

Possible new modules/files:

```text
crates/egglsp/src/runtime.rs
crates/egglsp/src/restart.rs
crates/egglsp/tests/supervisor_restart_stdio.rs
```

Prefer modifying existing modules unless a new module clearly reduces duplication.

## Non-Goals

Do not implement during this corrective pass:

- Tier 2 servers;
- automatic server downloads in real-server tests;
- multi-root workspaces;
- incremental text sync;
- pull diagnostics;
- model-facing manual restart commands;
- broad TUI health panels;
- restart jitter;
- arbitrary request replay;
- workspace-edit application;
- performance optimization unrelated to correctness.

# Phase 1 — Repair the Real-Server Harness Handshake

## Current Problem

`run_smoke_suite()` calls `LspClient::new_with_launch_spec()` and treats successful construction as an initialize pass. It does not call:

```rust
client.initialize(...)
client.send_initialized()
```

As a result, capability snapshots are empty/default and subsequent protocol traffic is invalid.

## Required Sequence

The harness must execute this exact order:

```rust
let client = LspClient::new_with_launch_spec(...).await?;
let capabilities = timeout(INIT_TIMEOUT, client.initialize(Some(profile.initialization_options.clone()))).await??;
timeout(REQUEST_TIMEOUT, client.send_initialized()).await??;
```

Then:

1. build the capability snapshot from `capabilities`;
2. open source documents;
3. wait for readiness;
4. run semantic checks;
5. send graceful shutdown.

## Initialize Timing

Record separate timings:

```text
process_launch_ms
initialize_ms
initialized_notification_ms
readiness_ms
```

If the report schema currently has only `initialize_ms`, either:

- add optional fields; or
- define `initialize_ms` as protocol handshake duration and add `launch_ms`.

Do not continue calling process construction “initialize.”

## Initialization Options

Pass:

```rust
Some(profile.initialization_options.clone())
```

Do not pass `{}` unconditionally.

Workspace configuration should remain available to server-request handling through the normal `LspClient` context. Verify `workspace/configuration` returns `profile.workspace_configuration` where supported. If `LspClient::new_with_launch_spec()` currently accepts one configuration value, pass the profile workspace configuration there and initialization options separately to `initialize()`.

## Timeout Handling

Every stage must use `tokio::time::timeout` with actionable error detail:

```text
server_id
binary path
stage
elapsed timeout
stderr tail if available
```

## Acceptance Criteria

- A test fails if the server never responds to `initialize`.
- Capability snapshot comes from the real initialize response.
- `initialized` is sent before `didOpen`.
- No semantic request is sent before initialization completes.

# Phase 2 — Replace Raw Fixture Vectors with Typed Fixture Metadata

## Current Problem

The fixture functions return a flat `Vec<PathBuf>` containing both root markers and source files. The harness uses the first item, which is `Cargo.toml` or `pyproject.toml`, for diagnostics and semantic requests.

## Required Type

Add a test-only fixture type:

```rust
struct RealServerFixture {
    tempdir: TempDir,
    root: PathBuf,
    source_files: Vec<PathBuf>,
    primary_source: PathBuf,
    secondary_source: Option<PathBuf>,
    diagnostics_file: PathBuf,
    symbol_position: lsp_types::Position,
    definition_position: lsp_types::Position,
    references_position: lsp_types::Position,
    hover_position: lsp_types::Position,
    expected_symbol_names: Vec<&'static str>,
}
```

Use separate fixture constructors:

```rust
fn rust_fixture() -> RealServerFixture
fn python_fixture() -> RealServerFixture
```

Do not include manifest files in `source_files`.

## Exact Positions

Choose deterministic positions from fixture source text.

Rust example:

```text
primary_source = src/lib.rs
definition_position = call site of add in caller()
references_position = declaration or call of add
hover_position = add call or Point type
```

Python example:

```text
primary_source = main.py
secondary_source = helper.py
definition_position = imported add use in caller()
references_position = imported add symbol or helper definition
hover_position = Point or add use
```

Avoid guessed line numbers. Keep source strings and positions adjacent in the fixture constructor so changes are obvious.

## Cross-File Reference Contract

Only assert multi-file references when:

- the fixture has a secondary source;
- the server advertises references;
- the chosen symbol is defined in another file.

The assertion should require at least two distinct URIs if that is a required Tier 1 behavior.

## Acceptance Criteria

- No semantic request targets `Cargo.toml` or `pyproject.toml`.
- Positions correspond to actual identifiers.
- Fixture metadata is explicit and language-specific.

# Phase 3 — Make Compatibility Checks Enforceable

## Current Problem

Most failed checks are recorded in the report but do not fail the test.

## Required Check Classification

Extend `LspCompatibilityCheck` or add a test-only requirement enum:

```rust
pub enum CompatibilityRequirement {
    Required,
    RequiredIfAdvertised,
    Optional,
    KnownLimitation,
}
```

Each check should carry:

```text
name
status
requirement
detail
duration
```

## Required Tier 1 Checks

For both servers:

```text
process launch                  Required
initialize handshake            Required
initialized notification        Required
capability snapshot             Required
didOpen                         Required
shutdown                        Required
```

For advertised capabilities:

```text
document symbols                RequiredIfAdvertised
definition                      RequiredIfAdvertised
references                      RequiredIfAdvertised
hover                           RequiredIfAdvertised
```

Diagnostics behavior:

- require at least one intentional diagnostic when the selected server/profile is expected to publish diagnostics;
- otherwise classify an explicit timeout as `PassingWithKnownLimits`, not an unconditional pass.

## Final Assertion Helper

Add:

```rust
fn assert_required_checks(report: &LspCompatibilityReport)
```

It should fail when:

- a `Required` check is not passing;
- a `RequiredIfAdvertised` check was advertised and failed;
- initialize or shutdown is absent.

Include a compact report dump in the assertion message.

## Acceptance Criteria

- A semantic regression fails CI.
- Optional unsupported operations remain explicit skips.
- Reports distinguish pass, fail, skip, and known limitation.

# Phase 4 — Make Supervisor Activation Automatic

## Current Problem

`LspService::new()` creates an exit channel, but exit handling starts only if callers manually invoke `start_exit_receiver()`.

## Required Design

Remove the public correctness requirement.

Preferred options:

### Option A — Lazy one-time activation

Add:

```rust
exit_receiver_started: AtomicBool
```

and an internal method:

```rust
fn ensure_exit_receiver_started(self: &Arc<Self>)
```

Call it from the first public async operation that can create a client.

### Option B — Arc constructor

Change construction to:

```rust
pub fn new(config: LspConfig) -> Arc<Self>
```

and spawn the receiver immediately.

Use Option A if changing constructor ownership would cause broad churn.

## Requirements

- startup is idempotent;
- exactly one receiver task exists;
- test constructors can activate the same path;
- shutdown closes/cancels the receiver task;
- no public caller must remember a second initialization step.

## Acceptance Criteria

- Creating a client guarantees exit events are consumed.
- Calling activation twice does not spawn duplicate receivers.
- A unit/integration test proves exit handling works without explicit setup.

# Phase 5 — Separate Shutdown Intent from Transport State

## Current Problem

The process monitor marks an exit expected when transport state is already failed. This can classify crashes as expected and graceful exits as unexpected.

## Required Runtime Intent Type

Add a dedicated shared state:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LspProcessIntent {
    Running,
    GracefulShutdownRequested,
    ForceKillRequested,
}
```

Store it in a shared watch/atomic/mutex structure owned by the process runtime.

## Exit Classification

The monitor should classify:

```rust
expected = matches!(intent, GracefulShutdownRequested | ForceKillRequested)
```

Transport state must not determine expectedness.

## Shutdown Path

Before sending `shutdown`:

```rust
runtime.set_intent(GracefulShutdownRequested)
```

If the graceful deadline expires:

```rust
runtime.set_intent(ForceKillRequested)
```

Then kill and reap.

## Zero Exit Code

An unexpected exit with status zero is still unexpected if no shutdown intent was set. Do not equate zero exit with expected exit.

## Acceptance Criteria

- Crash after reader failure remains unexpected.
- Graceful protocol shutdown is expected.
- Forced kill during service shutdown is expected.
- Tests cover all three cases.

# Phase 6 — Introduce One Authoritative Process Runtime Owner

## Current Problem

The monitor takes the child from `LspClient`, owns an `Arc<LspClient>`, and waits indefinitely. This prevents clean drop and provides no cancellation/kill path.

## Required Type

Introduce a runtime handle, for example:

```rust
pub struct LspProcessRuntime {
    pub generation: u64,
    intent_tx: watch::Sender<LspProcessIntent>,
    exit_rx: watch::Receiver<Option<LspProcessExitEvent>>,
    kill_tx: mpsc::Sender<ProcessControl>,
}
```

Internal owner task should own:

```text
child process handle
stderr reader/ring buffer
shutdown intent receiver
kill receiver
exit event publication
```

`LspClient` should not own the child directly once the runtime task starts.

## Process Owner Loop

Use `tokio::select!` over:

```text
child.wait()
force-kill command
runtime cancellation
```

The owner must:

1. wait exactly once;
2. kill on explicit request;
3. reap after kill;
4. publish one exit event;
5. include stderr tail;
6. terminate after exit.

## Client Shutdown

`LspClient::shutdown()` should:

1. set graceful intent;
2. send `shutdown` request;
3. send `exit` notification;
4. await runtime exit under a bounded deadline;
5. request force kill on timeout;
6. await final reap under a second bounded deadline.

Keep the service-level global shutdown deadline.

## Acceptance Criteria

- No task owns an `Arc<LspClient>` solely while waiting on a child forever.
- Hung servers are killed and reaped.
- One exit event is emitted.
- No double `wait()` exists.

# Phase 7 — Add Per-Client Generation Tracking

## Current Problem

Exit events carry a generation, but operational state and client publication do not track or compare per-client generations. Health snapshots report generation zero.

## Required Model

Add a monotonically increasing generation per client key:

```rust
struct OperationalServerState {
    generation: u64,
    state: LspOperationalState,
    ...
}
```

On first client publication:

```text
generation = 1
```

On each restart attempt that creates a new runtime:

```text
generation += 1
```

Store generation in:

- operational state;
- process runtime;
- exit event;
- diagnostics cache entries or metadata;
- health snapshots.

## Stale Exit Check

At the beginning of exit handling:

```rust
if event.generation != current_generation {
    debug!(... "ignoring stale process exit event");
    return;
}
```

Do not touch the current client or pending requests for stale events.

## Publication Check

Before publishing a restarted client, verify:

```text
service lifecycle is running
restart token is current
expected generation matches operational generation
no newer manual/client initialization won
```

## Acceptance Criteria

- Health snapshots report real generation values.
- Old exit events cannot fail a newer client.
- Scripted test proves stale-generation rejection.

# Phase 8 — Apply Compatibility and Restart Profiles to Production

## Current Problem

Compatibility profiles exist but the service ignores them.

## Required Profile Resolution

During client initialization, resolve:

```rust
let profile = compatibility::profile_for_server(server.id)
```

Use profile defaults only when explicit user config does not override them.

Priority:

```text
explicit LspConfig rule
compatibility profile default
server definition default
```

Apply:

- executable candidates where applicable;
- default args;
- initialization options;
- workspace configuration;
- readiness policy;
- restart policy;
- known limitations for reports/health notes.

## Restart Configuration

Restart must remain disabled by default.

Add a configuration path such as:

```toml
[lsp.rust-analyzer.restart]
enabled = true
max_attempts = 3
initial_backoff_ms = 500
max_backoff_ms = 8000
reset_after_healthy_ms = 60000
```

If the existing config schema cannot absorb nested restart settings cleanly, add optional fields to the active rule. Do not hard-code `restart_enabled = false` without a way to enable it.

## Acceptance Criteria

- Profiles are used in production initialization.
- Explicit user config wins.
- Restart remains off unless enabled.
- Tests cover precedence.

# Phase 9 — Replace Hard-Coded Restart Reconstruction

## Current Problem

Restart uses `root/src/lib.rs`, which is invalid for Python and many Rust layouts.

## Required Restart Descriptor

Store a stable descriptor per client key:

```rust
struct LspClientDescriptor {
    key: String,
    server_id: String,
    root: PathBuf,
    launch_spec: LspLaunchSpec,
    initialization_options: Option<serde_json::Value>,
    workspace_configuration: serde_json::Value,
    readiness_policy: LspReadinessPolicy,
    restart_policy: LspRestartPolicy,
    seed_file: Option<PathBuf>,
}
```

Persist this descriptor when the client is first created.

Restart must use the descriptor directly. Do not re-detect language or project root from a synthetic path.

## Seed File

If the existing initialization path requires a file:

- use the first currently open document for that client;
- otherwise retain the original file path used to create the client;
- never synthesize `src/lib.rs`.

## Acceptance Criteria

- Restart works for Python fixtures.
- Restart works for binary-only Rust fixture with `src/main.rs`.
- No restart path constructs a language-specific synthetic file.

# Phase 10 — Consolidate Restart into One Coordinator

## Current Problem

`LspService::restart_client()` and `LspServiceClone::restart_client()` duplicate logic and have diverged.

## Required Refactor

Create one coordinator, for example:

```rust
async fn restart_client_inner(
    shared: &LspServiceShared,
    key: &str,
    trigger: RestartTrigger,
) -> Result<(), LspError>
```

Move shared Arcs into a dedicated internal shared struct rather than maintaining a partial clone with copied logic.

The coordinator must own:

1. generation increment;
2. restart-state transition;
3. current-client removal;
4. old runtime shutdown/kill;
5. retry/backoff loop;
6. client reinitialization from descriptor;
7. readiness wait;
8. document replay;
9. ownership restoration;
10. diagnostics stale marking;
11. final ready/failed transition.

Delete duplicate implementations after migration.

## Acceptance Criteria

- One restart algorithm exists.
- Manual and automatic restart call the same coordinator.
- Replay behavior cannot diverge.

# Phase 11 — Implement Correct Retry, Backoff, Exhaustion, and Cancellation

## Required Retry Loop

For a restart sequence:

```rust
for attempt in 1..=policy.max_attempts {
    schedule/backoff
    try restart
    if success -> ready and return
    if shutdown/cancelled -> stopped and return
    if final failure -> failed
}
```

A failed initialization must continue to the next attempt unless exhausted.

## Backoff

Use policy fields:

```text
initial_backoff
max_backoff
```

Do not use a separate hard-coded helper once policy is available.

## Cancellation

Store a cancellation token per scheduled restart.

Cancel when:

- service shutdown begins;
- manual restart supersedes automatic restart;
- a newer generation/client is published;
- client is explicitly removed.

## Reset After Healthy

If the client stays ready longer than `reset_after_healthy`, reset consecutive restart attempts to zero.

This can be implemented lazily when handling the next exit:

```rust
if last_healthy_at.elapsed() >= reset_after_healthy {
    restart_attempts = 0;
}
```

## Acceptance Criteria

- Failed restart initialization consumes the retry budget correctly.
- Exactly `max_attempts` launches occur.
- Shutdown cancels pending backoff.
- Exhaustion leaves a stable failed state.

# Phase 12 — Restore Document Ownership and Replay Correctly

## Required Replay Semantics

After successful reinitialization:

1. send `didOpen` for every currently open snapshot;
2. use the latest text;
3. restore `document_owners` for each URI;
4. update the new client’s `opened_files` state;
5. keep registry entries intact;
6. do not replay closed documents.

## Version Policy

Choose and document one policy.

Preferred:

- preserve service-level snapshot version;
- send the preserved version in replay if servers accept it;
- if generation-local reset is required, add a separate `server_version` field instead of overwriting authoritative version.

Do not silently reset every replay to version 1 without documenting the semantic effect.

## Replay Failure

If any document replay fails:

- record the failure;
- transition to `Degraded` or fail restart according to policy;
- do not mark fully ready silently.

## Acceptance Criteria

- Update/save/close works after restart.
- Latest dirty text is replayed.
- Closed documents are absent.
- Scripted test verifies ownership restoration.

# Phase 13 — Keep Health Available Without a Live Client

## Current Problem

Health snapshot requires a client in the live-client map and therefore returns `None` during restart/failure.

## Required Snapshot Model

Read operational state first. Make client-derived fields optional:

```rust
pub struct LspOperationalHealthSnapshot {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    pub state: LspOperationalState,
    pub transport: Option<ClientTransportSnapshot>,
    pub pending_requests: usize,
    pub open_documents: usize,
    pub last_message_age_ms: Option<u64>,
    pub last_diagnostics_age_ms: Option<u64>,
    pub restart_attempts: u32,
    pub last_error: Option<String>,
    pub stderr_tail: Vec<String>,
}
```

If a live client is absent:

```text
transport = None
pending_requests = 0
```

## Real Age Fields

Track:

- last received protocol message timestamp;
- last diagnostics timestamp.

Do not leave permanent `None` placeholders when data exists.

## Acceptance Criteria

- Snapshot exists for `RestartScheduled`, `Restarting`, and `Failed`.
- Generation is real.
- Last error/stderr are visible.

# Phase 14 — Integrate Stderr Capture

## Current Problem

The ring-buffer type exists, but launch stderr is drained separately and exit events use an empty tail.

## Required Integration

Create one shared:

```rust
Arc<Mutex<StderrRingBuffer>>
```

per process runtime.

The stderr task should:

1. read lines;
2. log bounded line content at an appropriate level;
3. push each line into the ring buffer;
4. terminate on EOF/cancellation.

The process owner should snapshot the ring buffer when creating the exit event.

## Real-Server Reports

Use the client/runtime stderr snapshot directly. Do not read `stderr.log` unless the launch layer actually writes it.

## Acceptance Criteria

- Exit events contain recent stderr.
- Health snapshots expose the tail.
- Compatibility reports include bounded stderr.
- No unbounded stderr storage exists.

# Phase 15 — Make Readiness Policies Real

## Current Problem

Progress readiness is implemented as a fixed sleep, and production state immediately becomes `Ready`.

## Required Notification State

Track work-done progress notifications in `LspClient` or a readiness tracker:

```text
$/progress begin
$/progress report
$/progress end
```

Provide bounded waits:

```rust
wait_for_first_diagnostics(timeout)
wait_for_progress_end(timeout)
```

## Production State

After initialize:

- `InitializedIsReady` -> `Ready`;
- diagnostics/progress policy -> `Indexing`;
- readiness completion -> `Ready`;
- timeout -> `Degraded` with reason, not silent `Ready`.

## Test Harness

Use the same readiness API as production where possible. Do not maintain a separate sleep-only implementation.

## Acceptance Criteria

- rust-analyzer readiness observes progress or transitions degraded on timeout.
- pyright readiness observes diagnostics or transitions degraded on timeout.
- readiness timing is recorded.

# Phase 16 — Enforce Operational State Transitions

## Current Problem

Production code assigns state directly and bypasses `transition()`.

## Required Helper

Add one state mutation method:

```rust
async fn transition_operational_state(
    states: &OperationalStateMap,
    key: &str,
    next: LspOperationalState,
) -> Result<(), LspError>
```

It should:

1. read current state;
2. call `health::transition()`;
3. log valid transitions;
4. reject or loudly log invalid transitions;
5. update timestamps/error metadata.

Allow explicit recovery/reset transitions only by extending the transition table deliberately.

Replace direct assignments throughout service/restart/shutdown code.

## Acceptance Criteria

- State-machine tests cover actual production transitions.
- Invalid transitions cannot silently occur.

# Phase 17 — Propagate Generation and Restart Freshness into Diagnostics

## Current Problem

Semantic evidence fields are always:

```text
server_generation = None
post_restart = false
```

## Required Diagnostic Cache Metadata

Each diagnostic cache entry should carry:

```rust
server_generation: u64
received_at: Instant
post_restart: bool
```

When diagnostics are retained across crash/restart:

- mark them stale;
- preserve old generation;
- set `post_restart = false` for old-generation evidence;
- replace with new-generation diagnostics when received.

Interpret `post_restart` carefully. Preferred meaning:

```text
true if evidence was received from the current generation after at least one restart
```

Document this definition.

## Collector Integration

`SemanticContextCollector` should retrieve generation metadata from the diagnostic snapshot and populate real values.

## Acceptance Criteria

- Generation is present when a client exists.
- Old-generation diagnostics are not presented as current/fresh.
- New diagnostics after restart show the new generation.

# Phase 18 — Integrate Operational Notes into Root Workflows

## Required Behavior

Before or during semantic collection, query operational health for the client key.

Append a note from:

```rust
state.context_note()
```

for non-ready states.

### Semantic context

- `Indexing`: continue best effort with incomplete-results note.
- `Degraded`: continue with reason and stale/freshness warning.
- `RestartScheduled` / `Restarting`: return explicit unavailable/retryable note or error.
- `Failed`: return clear server/root failure information.

### Security context

Carry the semantic notes into the final packet.

### Hunk context

Add operational notes to the response/global summary.

Do not present stale diagnostics as fresh.

## Acceptance Criteria

- Root-level tests assert indexing, restarting, and failed notes.
- Security and hunk summaries preserve these notes.

# Phase 19 — Add Deterministic Scripted Supervisor Tests

## Required Test File

Add:

```text
crates/egglsp/tests/supervisor_restart_stdio.rs
```

Gate with:

```text
lsp-test-support
```

Use the scripted fake server, not real servers.

## Required Scenarios

### 1. Unexpected exit, restart disabled

- initialize successfully;
- issue one pending request;
- server exits unexpectedly;
- pending request fails promptly;
- health becomes `Failed`;
- no second process starts.

### 2. Graceful shutdown

- initialize;
- call service shutdown;
- exit is expected;
- no restart is scheduled;
- process is reaped.

### 3. Successful automatic restart

- restart enabled;
- generation 1 exits;
- generation 2 starts after backoff;
- open document is replayed;
- ownership is restored;
- semantic request succeeds;
- health reports generation 2 and `Ready`.

### 4. Restart initialization failure then recovery

- generation 1 exits;
- generation 2 initialization fails;
- generation 3 initializes successfully;
- attempt count and backoff are correct.

### 5. Restart exhaustion

- every restart fails;
- exactly `max_attempts` attempts occur;
- final state is `Failed`;
- no additional process starts.

### 6. Shutdown cancels scheduled restart

- crash schedules delayed restart;
- shutdown begins before delay expires;
- timer is cancelled;
- no replacement process starts.

### 7. Stale exit event

- generation 1 exit event is delayed;
- generation 2 is already ready;
- delayed generation 1 event arrives;
- generation 2 remains healthy and pending requests survive.

### 8. Replay latest content

- open version 1;
- update to version 2 dirty content;
- crash/restart;
- replay contains version 2 text;
- closed document is not replayed.

### 9. Hung process forced kill

- server ignores shutdown/exit;
- shutdown deadline expires;
- process is killed and reaped;
- service reaches stopped.

## Acceptance Criteria

- Tests use bounded waits, not sleeps alone.
- Process start counts come from transcript/start records.
- All restart invariants are deterministic.

# Phase 20 — Correct and Pin Real-Server CI

## Pinning

Pin explicit versions.

Examples:

```yaml
RUST_TOOLCHAIN: "1.xx.x"
BASEDPYRIGHT_VERSION: "x.y.z"
```

For rust-analyzer, use one reproducible method:

- a pinned Rust toolchain whose component version is known; or
- a pinned release binary with checksum verification.

Do not install latest unpinned basedpyright.

## Matrix Filtering

Run only the selected server test in each matrix job:

```bash
cargo test ... rust_analyzer_smoke
cargo test ... basedpyright_smoke
```

Do not rely on the other test skipping.

## Reports

Sanitize version strings for filenames.

Upload:

```text
compatibility JSON
bounded stderr/logs
```

## Required Assertions

The workflow should fail if required checks fail.

## Acceptance Criteria

- Weekly results are reproducible.
- Each matrix job tests one server.
- Reports are meaningful and versioned.

# Phase 21 — Documentation Corrections

Until the corrective work is complete, documentation should describe Phase 3 as in progress.

Update after completion:

- exact Tier 1 compatibility status;
- profile precedence;
- restart default disabled;
- per-client generation semantics;
- expected-exit intent model;
- forced kill/reap behavior;
- readiness policy behavior;
- diagnostic generation/post-restart definition;
- deterministic supervisor test matrix;
- pinned CI versions.

Do not claim complete restart reliability before scripted tests pass.

# Exact Implementation Order for a Smaller Model

Follow this order exactly.

## Pass 1 — Real-server harness correctness

1. Add typed fixture metadata.
2. Call real `initialize` and `send_initialized`.
3. Pass profile initialization/workspace configuration.
4. Query only source files at exact positions.
5. Add timeout wrappers.
6. Add required-check classification and final assertions.
7. Run Tier 1 tests locally where binaries exist.

## Pass 2 — Supervisor process ownership

1. Add explicit process intent.
2. Introduce authoritative process runtime owner.
3. Integrate bounded stderr capture.
4. Make exit receiver activation automatic.
5. Correct graceful/crash/force-kill classification.
6. Add graceful and hung-process scripted tests.

## Pass 3 — Generation and operational health

1. Add per-client generation to operational state.
2. Add stale-event rejection.
3. make health snapshots available without live clients.
4. populate real generation and age fields.
5. replace direct state assignments with transition helper.
6. add generation/state tests.

## Pass 4 — Restart descriptor and coordinator

1. Persist client descriptor.
2. remove hard-coded `src/lib.rs` reconstruction.
3. consolidate duplicate restart implementations.
4. apply profile/config restart policy.
5. implement retry/backoff/exhaustion/cancellation.
6. add manual internal restart entry point using same coordinator.

## Pass 5 — Document replay and diagnostic freshness

1. replay latest open-document snapshots.
2. restore ownership.
3. define version behavior.
4. mark old diagnostics stale.
5. propagate generation/post-restart metadata.
6. add replay and stale-evidence tests.

## Pass 6 — Readiness and workflow adoption

1. track progress/diagnostic readiness.
2. transition through `Indexing`/`Degraded`/`Ready`.
3. append operational notes to semantic context.
4. propagate to security and hunk context.
5. add root composite tests.

## Pass 7 — CI and docs

1. pin Tier 1 versions.
2. run one server per matrix job.
3. upload valid reports/stderr.
4. update compatibility matrix.
5. run all scripted, real-server, workspace, and Clippy checks.

# Verification Commands

## Real-server harness

```bash
CODEGG_RA_BIN=/path/to/rust-analyzer \
  cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke rust_analyzer_smoke -- --nocapture

CODEGG_PYRIGHT_BIN=/path/to/basedpyright-langserver \
  cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke basedpyright_smoke -- --nocapture
```

## Supervisor and restart

```bash
cargo test -p egglsp --features lsp-test-support \
  --test supervisor_restart_stdio -- --test-threads=1

cargo test -p egglsp --features lsp-test-support \
  --test supervisor_restart_stdio -- --test-threads=8
```

## Existing scripted regression

```bash
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
```

## Full validation

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo check --workspace --all-targets --all-features
cargo test --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

# Review Checklist

## Real-server harness

- [ ] `initialize` request is sent.
- [ ] `initialized` notification is sent.
- [ ] Capabilities come from real initialize response.
- [ ] Only source files are queried.
- [ ] Exact per-language positions are used.
- [ ] Required semantic failures fail tests.
- [ ] Every stage is bounded.

## Supervisor activation and intent

- [ ] Exit receiver starts automatically.
- [ ] One receiver task exists.
- [ ] Expected exit uses explicit intent.
- [ ] Crash after transport failure remains unexpected.
- [ ] Graceful and force-kill exits are expected.

## Process ownership

- [ ] One task owns child wait/kill/reap.
- [ ] Hung process is killed.
- [ ] No monitor-owned client retention leak remains.
- [ ] Exit event contains stderr tail.

## Generation safety

- [ ] Per-client generation increments.
- [ ] Exit events are generation-checked.
- [ ] Stale events are ignored.
- [ ] Health reports real generation.
- [ ] Restart publication is generation-safe.

## Restart policy

- [ ] Restart defaults disabled.
- [ ] Config can enable restart.
- [ ] Profile/config precedence is tested.
- [ ] Failed restart initialization retries.
- [ ] Backoff is policy-driven.
- [ ] Shutdown cancels timers.
- [ ] Exhaustion is stable.

## Restart reconstruction

- [ ] Client descriptor is persisted.
- [ ] No hard-coded Rust path remains.
- [ ] Python restart test passes.
- [ ] One restart coordinator exists.

## Document replay

- [ ] Latest text is replayed.
- [ ] Ownership is restored.
- [ ] Closed documents are not replayed.
- [ ] Version policy is documented.
- [ ] Replay failures degrade/fail explicitly.

## Health and freshness

- [ ] Health exists during restarting/failed states.
- [ ] Message and diagnostics ages are populated.
- [ ] Last error and stderr are exposed.
- [ ] Diagnostics carry generation.
- [ ] Old diagnostics become stale after restart.
- [ ] Post-restart semantics are documented.

## Readiness

- [ ] Progress readiness observes progress events.
- [ ] Diagnostics readiness observes cache changes.
- [ ] Timeout produces degraded state.
- [ ] Production state uses indexing/ready meaningfully.

## Workflow adoption

- [ ] Semantic context includes operational notes.
- [ ] Security context preserves notes/freshness.
- [ ] Hunk context preserves notes/freshness.
- [ ] Restarting/failed states are not silently treated as ready.

## CI

- [ ] Server versions are pinned.
- [ ] Each matrix job runs only its server test.
- [ ] Required checks fail CI.
- [ ] Reports and stderr artifacts upload.

# Completion Criteria

This corrective Phase 3 pass is complete when:

1. Tier 1 real-server tests perform a valid handshake and required semantic operations pass.
2. Supervisor activation is automatic.
3. Process intent correctly distinguishes graceful shutdown, force kill, and unexpected exit.
4. One authoritative runtime task owns wait/kill/reap.
5. Per-client generation prevents stale exit/restart corruption.
6. Restart policy is configurable, bounded, retry-correct, and disabled by default.
7. Restart reconstructs clients from persisted descriptors rather than synthetic paths.
8. One restart coordinator handles automatic and manual paths.
9. Document replay restores latest content and ownership.
10. Health remains available through restart and failure.
11. Stderr tails are integrated.
12. Readiness policies drive indexing/ready/degraded states.
13. Diagnostic evidence carries real generation and post-restart metadata.
14. Semantic/security/hunk workflows expose operational state and freshness accurately.
15. Deterministic scripted supervisor tests cover crash, restart, exhaustion, cancellation, stale generation, replay, and forced kill.
16. Real-server CI uses pinned versions and fails on required regressions.
17. Existing Phase 2 scripted suites remain green.

## Handoff Result

After this pass, Codegg's LSP Phase 3 implementation will move from structural scaffolding to an operationally trustworthy system: real servers will be tested through valid protocol flows, process ownership will be explicit, restart behavior will be bounded and generation-safe, document state will survive recovery, stale evidence will remain identifiable, and compatibility reports will reflect actual server behavior rather than permissive smoke-test placeholders.
