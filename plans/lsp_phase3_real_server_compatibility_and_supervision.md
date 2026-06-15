# LSP Phase 3: Real-Server Compatibility, Supervision, and Workflow Adoption

## Purpose

Begin Phase 3 after completion of the scripted stdio harness and packaging isolation work through:

```text
0a05a288b230566c5b9d2e374932ee3c29518752
49044c8472cdbf9b7865a8c54897da9b7fcac3de
```

Phase 2 established deterministic confidence in Codegg's production LSP stack using a scripted child process. Phase 3 should now validate and harden Codegg against real language servers with real initialization quirks, capability differences, indexing delays, process crashes, stderr behavior, workspace conventions, and restart requirements.

This phase should not immediately support every server or automatically restart every failure. It should establish a small compatibility matrix, a supervised process lifecycle, stable health reporting, and explicit rollout gates for broader use of LSP evidence in Codegg workflows.

This plan is tailored for execution by a smaller model. Follow the implementation order exactly. Do not broaden scope unless a listed acceptance criterion cannot be met without a narrow supporting change.

## Phase 3 Outcomes

At completion:

1. Codegg has opt-in smoke/integration tests against a small real-server matrix.
2. Server-specific launch and initialization quirks are represented as explicit compatibility profiles rather than scattered conditionals.
3. `LspService` can distinguish healthy, indexing, degraded, failed, restarting, and stopped clients.
4. Unexpected process exit and transport failure are observed by a supervisor.
5. Restart policy is bounded, backoff-controlled, generation-safe, and disabled by default until explicitly enabled.
6. Open documents can be replayed after a successful restart.
7. Pending requests fail promptly on crash and are never silently replayed.
8. Diagnostics and semantic results expose freshness/degraded-state metadata after restart or failure.
9. Real-server tests remain separate from default network-free CI.
10. LSP-backed Codegg workflows adopt the health/freshness information without hiding failures from the user or model.

## Scope

Primary crate files likely involved:

```text
crates/egglsp/src/server.rs
crates/egglsp/src/launch.rs
crates/egglsp/src/client.rs
crates/egglsp/src/service.rs
crates/egglsp/src/error.rs
crates/egglsp/src/diagnostics.rs
crates/egglsp/src/context.rs
crates/egglsp/src/document_sync.rs
crates/egglsp/src/lib.rs
```

Possible new modules:

```text
crates/egglsp/src/compatibility.rs
crates/egglsp/src/supervisor.rs
crates/egglsp/src/health.rs
crates/egglsp/tests/real_server_smoke.rs
crates/egglsp/tests/real_server_restart.rs
```

Root workflow files likely involved:

```text
src/lsp/semantic_context.rs
src/lsp/hunk_nav_collector.rs
src/tool/lsp.rs
```

Documentation and CI:

```text
architecture/lsp.md
docs/LSP.md
.opencode/skills/lsp/SKILL.md
AGENTS.md
README.md
.github/workflows/lsp-real-server.yml        # optional new workflow
```

## Non-Goals

Do not implement during Phase 3:

- universal support for every LSP server;
- automatic downloading of all servers in default CI;
- uncontrolled infinite restart loops;
- replay of arbitrary in-flight semantic requests after restart;
- multi-root workspace support;
- pull diagnostics unless required by a selected server and separately approved;
- incremental sync conversion;
- server multiplexing across unrelated roots;
- direct application of server-provided workspace edits;
- broad TUI redesign;
- speculative performance optimization before real-server measurements exist.

# Part A — Define the Initial Real-Server Matrix

## A1. Required Initial Matrix

Use a small matrix representing different ecosystems and server behaviors.

Required Tier 1 servers:

```text
rust-analyzer
pyright or basedpyright
```

Recommended Tier 2 servers after Tier 1 passes:

```text
typescript-language-server
gopls
clangd
```

Do not begin with all five at once. Implement Tier 1 first.

## A2. Rationale

`rust-analyzer` provides:

- larger initialization payloads;
- workspace indexing;
- progress notifications;
- rich symbols, references, hierarchy, and diagnostics;
- realistic stderr and process lifecycle behavior.

`pyright` or `basedpyright` provides:

- a different runtime and installation mechanism;
- different capability claims;
- Python workspace/root behavior;
- rapid diagnostics and type-analysis workflows;
- potential node/runtime command resolution differences.

Tier 2 adds further diversity only after the harness is stable.

## A3. Compatibility Status Model

Introduce a compatibility status table in documentation:

```text
Server                     Launch   Initialize   Diagnostics   Symbols   References   Hierarchy   Restart
rust-analyzer              unknown  unknown      unknown       unknown   unknown      unknown     unknown
pyright/basedpyright       unknown  unknown      unknown       unknown   unknown      unknown     unknown
```

Allowed states:

```text
unsupported
experimental
passing
passing-with-known-limits
```

Do not label a server supported merely because initialize succeeds.

# Part B — Add Explicit Compatibility Profiles

## B1. Problem

Real servers differ in executable name, arguments, initialization options, root markers, environment requirements, sync expectations, and readiness behavior. These differences must not become ad hoc checks inside `LspClient`.

## B2. New Compatibility Type

Add a type similar to:

```rust
#[derive(Debug, Clone)]
pub struct LspCompatibilityProfile {
    pub server_id: String,
    pub executable_candidates: Vec<String>,
    pub default_args: Vec<String>,
    pub root_markers: Vec<String>,
    pub initialization_options: serde_json::Value,
    pub workspace_configuration: serde_json::Value,
    pub readiness_policy: LspReadinessPolicy,
    pub restart_policy: LspRestartPolicy,
    pub known_limitations: Vec<String>,
}
```

Do not make all fields public if the existing server registry already provides a better encapsulation. The important requirement is one explicit profile object per server.

## B3. Readiness Policy

Add a small readiness enum:

```rust
pub enum LspReadinessPolicy {
    InitializedIsReady,
    WaitForDiagnosticsOrTimeout { timeout: Duration },
    WaitForProgressEndOrTimeout { timeout: Duration },
    WarmupDelay { duration: Duration },
}
```

Use only policies required by the initial servers.

Do not block client construction indefinitely waiting for full indexing. Separate protocol initialization from semantic readiness.

## B4. Server-Specific Profile Requirements

### rust-analyzer

Profile should specify:

- executable candidate `rust-analyzer`;
- root markers such as `Cargo.toml`, `rust-project.json`, `.git`;
- conservative initialization options;
- readiness policy based on initialized state plus optional progress/indexing state;
- known limitation that first semantic requests may be incomplete while indexing.

### pyright or basedpyright

Profile should specify:

- executable candidates in preferred order;
- root markers such as `pyproject.toml`, `pyrightconfig.json`, `setup.py`, `.git`;
- empty or minimal initialization options;
- configuration response appropriate for Python analysis;
- readiness policy based on diagnostics or a bounded warmup period.

## B5. Acceptance Criteria

- Tier 1 server quirks are represented in profiles.
- No `if server.id == ...` branches are added to generic request routing.
- Profiles are unit tested for root markers, executable candidates, and initialization configuration.

# Part C — Build an Opt-In Real-Server Test Harness

## C1. Test Classification

Real-server tests must not run in default CI or ordinary `cargo test --workspace`.

Add a feature:

```toml
lsp-real-server-tests = []
```

Add explicit test targets with:

```toml
required-features = ["lsp-real-server-tests"]
```

Do not reuse `lsp-test-support`; scripted and real-server tests are separate concerns.

## C2. Environment-Based Server Discovery

Support explicit environment overrides:

```text
CODEGG_RA_BIN
CODEGG_PYRIGHT_BIN
CODEGG_BASEDPYRIGHT_BIN
```

Fallback to executable candidates on `PATH` only for local runs.

If no server is found, tests should skip with an explicit message rather than fail default development workflows.

Use a helper:

```rust
fn require_server_binary(env_var: &str, candidates: &[&str]) -> Option<PathBuf>
```

Do not download or install servers from test code.

## C3. Temporary Project Fixtures

Create package-contained fixture templates or generate projects in `TempDir`.

### Rust fixture

Minimum files:

```text
Cargo.toml
src/lib.rs
```

Source should contain:

- one type error for diagnostics;
- one function call for definition/reference tests;
- at least two symbols;
- one small call hierarchy.

Run `cargo metadata` only if rust-analyzer requires it and bound the timeout.

### Python fixture

Minimum files:

```text
pyproject.toml or pyrightconfig.json
main.py
helper.py
```

Source should contain:

- one type mismatch;
- one import/reference relationship;
- multiple symbols.

## C4. Harness Structure

Add:

```rust
struct RealServerHarness {
    tempdir: TempDir,
    root: PathBuf,
    source_files: Vec<PathBuf>,
    client: Arc<LspClient>,
    server_id: String,
}
```

Responsibilities:

- resolve the explicit compatibility profile;
- construct `LspLaunchSpec`;
- initialize and send `initialized` through production APIs;
- open fixture files;
- wait for readiness through the profile policy;
- collect bounded stderr/health diagnostics on failure;
- call production shutdown in teardown.

## C5. Total Timeouts

Every real-server test must have a total timeout.

Recommended defaults:

```text
initialize: 20 seconds
readiness/indexing: 30 seconds
individual semantic request: 10 seconds
shutdown: existing bounded shutdown policy
whole test: 60 seconds
```

Make these configurable through test helper constants, not production defaults.

## C6. Acceptance Criteria

- Real-server tests are fully opt-in.
- Missing server binaries result in explicit skip behavior.
- Test code performs no network installation.
- Failure output includes server ID, binary path, health state, stderr tail, and fixture path.

# Part D — Implement Tier 1 Compatibility Smoke Tests

## D1. Common Smoke Contract

For each Tier 1 server, test:

1. Process launch.
2. Initialize result.
3. Capability snapshot.
4. `initialized` notification.
5. `didOpen`.
6. At least one diagnostics publication or explicit no-diagnostics timeout outcome.
7. Document symbols.
8. Hover or definition.
9. References where supported.
10. Graceful shutdown.

## D2. Assertions Must Be Capability-Aware

Do not fail a server for an operation it does not advertise.

Pattern:

```rust
if capabilities.definition_provider {
    assert!(client.go_to_definition(...).await?.is_some());
}
```

The test should fail when:

- the server advertises capability support but the request path fails;
- Codegg misparses the response;
- transport fails unexpectedly;
- shutdown leaks the process.

## D3. rust-analyzer Test

Required checks:

- root URI points to the temporary crate;
- capability snapshot includes expected common Rust features;
- diagnostics eventually contain the intentional type error or readiness times out with a documented reason;
- document symbols include the fixture symbols;
- definition of the helper call resolves inside the fixture;
- references return at least one location;
- shutdown completes without orphan process.

Do not assert exact diagnostic wording or full capability equality; these vary by server version.

## D4. pyright/basedpyright Test

Required checks:

- correct executable/profile is reported;
- root URI points to the Python fixture;
- diagnostics include the intentional type mismatch if the server emits it;
- symbols include fixture functions/classes;
- definition or references resolve across `main.py` and `helper.py`;
- shutdown completes.

Do not require identical behavior between pyright and basedpyright. Record which implementation is used.

## D5. Version Capture

Before launch or in the harness, run the server's version command with a bounded timeout when supported.

Store:

```rust
pub struct LspServerVersion {
    pub raw: String,
    pub parsed: Option<String>,
}
```

Include version in test logs and compatibility reports.

Do not reject unknown future versions by default.

# Part E — Introduce Operational Health States

## E1. Problem

The current transport snapshot distinguishes running and failed states, but Phase 3 needs a service-level operational view that includes indexing, degraded semantic readiness, restart activity, and permanent failure.

## E2. New Health Model

Add a typed enum similar to:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LspOperationalState {
    Starting,
    Initializing,
    Indexing,
    Ready,
    Degraded { reason: String },
    RestartScheduled { attempt: u32, delay_ms: u64 },
    Restarting { attempt: u32 },
    Failed { reason: String },
    Stopping,
    Stopped,
}
```

Do not replace low-level transport state. Operational state sits above it.

## E3. Health Snapshot

Add:

```rust
pub struct LspOperationalHealthSnapshot {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    pub state: LspOperationalState,
    pub transport: ClientTransportSnapshot,
    pub pending_requests: usize,
    pub open_documents: usize,
    pub last_message_age_ms: Option<u64>,
    pub last_diagnostics_age_ms: Option<u64>,
    pub restart_attempts: u32,
}
```

Expose read-only snapshots from `LspService`.

## E4. State Transition Rules

Define explicit transitions:

```text
Starting -> Initializing
Initializing -> Indexing or Ready
Indexing -> Ready or Degraded
Ready -> Degraded or RestartScheduled or Stopping
Degraded -> Ready or RestartScheduled or Failed
RestartScheduled -> Restarting or Stopping
Restarting -> Initializing or Failed
Stopping -> Stopped
```

Unexpected transitions should be logged and unit tested.

## E5. Acceptance Criteria

- Operational state is typed.
- State transitions are centralized, not assigned from scattered call sites.
- Snapshot reads do not mutate state.
- Existing transport snapshot API remains compatible.

# Part F — Add Process Exit Observation and Supervisor

## F1. Supervisor Responsibility

Create a supervisor that observes:

- child process exit;
- stdout EOF;
- transport failure;
- explicit shutdown;
- restart policy state.

Suggested new module:

```text
crates/egglsp/src/supervisor.rs
```

## F2. Exit Event Type

Add:

```rust
pub struct LspProcessExitEvent {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    pub status: Option<i32>,
    pub signal: Option<i32>,
    pub expected: bool,
    pub stderr_tail: Vec<String>,
    pub timestamp: SystemTime,
}
```

Platform-specific signal fields may be optional.

## F3. Observe Without Double-Waiting

Ensure only one task owns `child.wait()` or equivalent process completion observation.

Do not have both shutdown code and supervisor independently wait the same child handle.

Preferred pattern:

- launch layer spawns one process monitor task;
- monitor sends one exit event through `watch`, `broadcast`, or `mpsc`;
- client/supervisor/shutdown consume the event rather than waiting independently.

## F4. Expected vs Unexpected Exit

Mark exits expected when:

- graceful shutdown has begun;
- forced shutdown/abort is in progress;
- service is stopping.

Mark exits unexpected otherwise.

Unexpected exit should:

1. transition transport to failed;
2. fail pending requests;
3. mark diagnostics/semantic evidence stale or degraded;
4. notify the supervisor;
5. schedule restart only when policy allows.

## F5. Acceptance Criteria

- One authoritative process exit event exists.
- Pending requests fail promptly after exit.
- Expected shutdown does not trigger restart.
- Unexpected exit is visible through health snapshots.

# Part G — Add Bounded Restart Policy

## G1. Default Policy

Restart must be disabled by default initially:

```rust
pub enum LspRestartMode {
    Disabled,
    OnUnexpectedExit,
}
```

## G2. Policy Type

Add:

```rust
#[derive(Debug, Clone)]
pub struct LspRestartPolicy {
    pub mode: LspRestartMode,
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub reset_after_healthy: Duration,
}
```

Safe initial defaults when enabled:

```text
max_attempts = 3
initial_backoff = 500 ms
max_backoff = 8 s
reset_after_healthy = 60 s
```

## G3. Backoff

Use bounded exponential backoff:

```text
attempt 1: 500 ms
attempt 2: 1 s
attempt 3: 2 s
```

Cap at `max_backoff`.

Do not add jitter unless deterministic tests can control it. Jitter may be added later.

## G4. Generation Safety

Every restarted client receives a new lifecycle generation.

Before publication, verify:

- service is still running;
- restart generation matches the current expected generation;
- no newer manual initialization replaced it;
- shutdown has not begun.

Reuse the existing lifecycle generation and publication validation patterns from Phase 1.

## G5. Restart Exhaustion

After `max_attempts`:

- transition to `Failed`;
- stop scheduling restarts;
- preserve the last failure reason and stderr tail;
- expose a manual restart path later or in this phase if simple.

## G6. Manual Restart

Add a narrow service API:

```rust
pub async fn restart_client(&self, key: &str) -> Result<(), LspError>
```

Behavior:

1. Stop/remove the current client generation.
2. Preserve open-document snapshots.
3. Start a new generation through the normal initialization coordinator.
4. Replay open documents only after successful initialize.

Do not expose this as a model-facing operation during the first implementation pass.

## G7. Acceptance Criteria

- Restart is disabled unless explicitly configured.
- Backoff and attempt caps are deterministic and unit tested.
- Shutdown cancels scheduled restart timers.
- Stale restart attempts cannot publish clients.
- Exhaustion yields a stable failed state.

# Part H — Track and Replay Open Documents

## H1. Document Registry

Restart requires authoritative open-document state.

Add or extend a registry containing:

```rust
pub struct OpenDocumentSnapshot {
    pub uri: Url,
    pub language_id: String,
    pub version: i32,
    pub text: String,
    pub dirty: bool,
}
```

The registry must reflect successful service-level open/update/save/close operations.

## H2. Replay Rules

After restarted client initialization:

1. Replay `didOpen` for every currently open document owned by the client key.
2. Preserve the latest text.
3. Preserve or reset version according to existing document-version invariants.
4. Do not replay `didSave` unless required by server semantics.
5. Do not replay closed documents.

Recommended version behavior:

- retain the latest monotonic version if the server accepts it;
- otherwise start a new per-server generation version while keeping service-level source version metadata separate.

Choose one behavior and document it.

## H3. Pending Requests

Never replay in-flight requests automatically.

All pending requests from the failed generation must return an error such as:

```rust
LspError::ServerRestarted {
    server_id,
    old_generation,
    new_generation: Option<u64>,
}
```

Callers may explicitly retry at a higher layer.

## H4. Diagnostics After Restart

When a server fails or restarts:

- existing diagnostics become stale;
- retain them temporarily as stale evidence if current policy permits;
- clear or replace them when new diagnostics arrive;
- include generation/freshness metadata.

## H5. Acceptance Criteria

- Open/update/close registry behavior is unit tested.
- Restart replays only currently open documents.
- Latest text is replayed.
- Pending requests fail rather than replay.
- Diagnostics are marked stale across generations.

# Part I — Extend the Scripted Harness for Supervisor Tests

## I1. Use Scripted Server for Deterministic Restart Tests

Do not rely on real servers for crash/restart correctness.

Extend existing scenarios only where needed:

```text
ExitAfterRequest
ExitAfterNotification
ExitAfterDelay
ExitWithCode
StderrThenExit
CountProcessStarts
```

Prefer existing `Exit` and transcript startup records if sufficient.

## I2. Required Supervisor Tests

### Unexpected exit without restart

- restart mode disabled;
- server exits after initialization;
- pending request fails;
- health becomes failed/degraded;
- no new process starts.

### Restart after unexpected exit

- restart mode enabled;
- first generation exits;
- supervisor schedules restart;
- second generation initializes;
- open document is replayed;
- semantic request succeeds on second generation.

### Restart exhaustion

- each generation exits immediately;
- exactly `max_attempts` restarts occur;
- state becomes failed;
- no further process starts.

### Shutdown cancels restart

- restart is scheduled with a delay;
- service shutdown occurs before timer fires;
- no new process launches;
- lifecycle reaches stopped.

### Manual restart

- healthy generation is manually restarted;
- generation increments;
- document replay occurs;
- old client cannot publish or serve requests.

## I3. Transcript Assertions

Use startup and request transcripts to assert exact generation/process counts.

Do not use process-table inspection.

## I4. Acceptance Criteria

- Supervisor correctness is proven with deterministic scripted tests.
- Real-server tests are not responsible for validating restart timing.

# Part J — Add Real-Server Compatibility Reports

## J1. Report Type

Add a serializable test/report type:

```rust
#[derive(Debug, Serialize)]
pub struct LspCompatibilityReport {
    pub server_id: String,
    pub server_version: Option<String>,
    pub platform: String,
    pub initialize_ms: u64,
    pub readiness_ms: Option<u64>,
    pub capabilities: LspCapabilitySnapshot,
    pub checks: Vec<LspCompatibilityCheck>,
    pub stderr_tail: Vec<String>,
    pub known_limitations: Vec<String>,
}
```

Each check:

```rust
pub struct LspCompatibilityCheck {
    pub name: String,
    pub status: CompatibilityCheckStatus,
    pub detail: Option<String>,
    pub duration_ms: Option<u64>,
}
```

## J2. Artifact Output

Real-server tests should optionally write JSON reports to:

```text
target/lsp-compatibility/<server>-<version>.json
```

Do not write reports into the repository working tree.

## J3. CI Artifact

If a dedicated workflow is added, upload compatibility JSON and bounded logs as artifacts.

Do not fail the entire workflow for a known optional capability; encode pass/skip/fail per check.

## J4. Acceptance Criteria

- Reports are deterministic enough to compare across runs.
- Exact server versions are included.
- Known limitations are explicit.

# Part K — CI Strategy

## K1. Default CI

Default CI remains:

- scripted harness;
- unit tests;
- no server downloads;
- no network requirement.

## K2. Opt-In Real-Server Workflow

Add an optional workflow triggered by:

```text
workflow_dispatch
schedule (weekly, optional)
changes to crates/egglsp/** or src/lsp/**
```

Start with Linux only.

Install pinned versions of Tier 1 servers in workflow steps.

Use explicit version pins and update them intentionally.

## K3. Matrix

Initial matrix:

```yaml
server:
  - rust-analyzer
  - basedpyright
```

Do not add Tier 2 until Tier 1 is stable.

## K4. Failure Policy

Initially mark the workflow non-required while compatibility is experimental.

Promote it to required only when:

- two consecutive weeks pass without flaky failures;
- runtime stays bounded;
- server install/version management is stable.

## K5. Acceptance Criteria

- Default CI remains fast and network-free.
- Real-server workflow is reproducible with pinned versions.
- Compatibility artifacts are retained.

# Part L — Integrate Health and Freshness into Codegg Workflows

## L1. Semantic Context

When operational state is not `Ready`, semantic context should include a bounded note such as:

```text
LSP state: indexing
LSP state: degraded — server restarted 1.2s ago
LSP state: failed — rust-analyzer exited with code 1
```

Do not hide stale evidence.

## L2. Diagnostic Evidence

Extend or reuse freshness metadata to include:

```text
server_generation
operational_state
evidence_age
post_restart
```

Do not break current packet schemas unnecessarily. Add optional fields where possible.

## L3. Tool Behavior

Recommended behavior:

- `Starting`/`Initializing`: return explicit warming/unavailable notes.
- `Indexing`: allow requests but mark incomplete evidence.
- `Degraded`: allow best-effort reads with warnings.
- `RestartScheduled`/`Restarting`: fail fast or return retryable unavailable status.
- `Failed`: fail with clear server/root/reason metadata.

## L4. User-Facing Observability

Expose service-level health through existing status/TUI surfaces where low-cost.

Minimum Phase 3 requirement:

- structured service API for health snapshots;
- logs with server/root/generation;
- no mandatory TUI redesign.

## L5. Acceptance Criteria

- Semantic/security/hunk responses do not present stale results as fresh after restart.
- Failure messages identify server and project root.
- Health snapshot can support future TUI display.

# Part M — Logging and Telemetry

## M1. Structured Fields

All lifecycle logs should include:

```text
server_id
root
client_key
generation
restart_attempt
operational_state
```

## M2. Events

Log at appropriate levels:

```text
server launch
initialize completed
readiness reached
indexing started/ended
unexpected exit
restart scheduled
restart started
restart succeeded
restart exhausted
manual restart
shutdown
```

## M3. Stderr

Retain a bounded stderr ring buffer per client.

Suggested bounds:

```text
last 100 lines
maximum 64 KiB total
```

Do not store unbounded stderr.

## M4. Acceptance Criteria

- Failure reports include bounded stderr.
- Logs distinguish generations.
- Sensitive source text is not dumped by default.

# Part N — Exact Implementation Order for a Smaller Model

Execute the work in this order.

## Pass 1 — Profiles and opt-in harness

1. Add `LspCompatibilityProfile` and readiness policy types.
2. Add rust-analyzer and pyright/basedpyright profiles.
3. Add `lsp-real-server-tests` feature and explicit test target.
4. Build `RealServerHarness` with binary discovery and timeouts.
5. Add fixture-project generators.
6. Add initialize/shutdown smoke tests for Tier 1.

## Pass 2 — Capability and semantic compatibility

1. Add capability-aware common smoke assertions.
2. Add diagnostics wait helper.
3. Add symbols/definition/reference checks.
4. Add server version capture.
5. Add JSON compatibility report output.
6. Document passing and known-limit states.

## Pass 3 — Operational health model

1. Add `LspOperationalState`.
2. Add transition helper/state machine.
3. Add `LspOperationalHealthSnapshot`.
4. Wire initialize/readiness/failure/shutdown states.
5. Add unit tests for valid transitions.

## Pass 4 — Process exit observation

1. Add authoritative process exit event.
2. Ensure only one monitor waits for child exit.
3. Propagate unexpected exit to transport and service health.
4. Fail pending requests promptly.
5. Mark diagnostics stale.
6. Add scripted unexpected-exit test with restart disabled.

## Pass 5 — Restart policy

1. Add restart mode/policy types.
2. Implement bounded deterministic backoff.
3. Add scheduled-restart cancellation token.
4. Add generation-safe publication.
5. Add exhaustion behavior.
6. Add manual restart service API.

## Pass 6 — Document replay

1. Add authoritative open-document snapshots.
2. Update registry on open/change/save/close.
3. Replay open documents after restart.
4. Add generation/freshness metadata to diagnostics.
5. Add scripted replay tests.

## Pass 7 — Workflow adoption

1. Include operational state in semantic context notes.
2. Include generation/staleness metadata in diagnostic evidence.
3. Make tool behavior explicit for indexing/restarting/failed states.
4. Add focused root-level tests.

## Pass 8 — CI and docs

1. Add optional pinned Tier 1 workflow.
2. Upload compatibility reports.
3. Update architecture and user docs.
4. Record Tier 2 as deferred.
5. Run full scripted, real-server, and workspace validation.

# Part O — Verification Commands

## Scripted regression suite

```bash
cargo test -p egglsp --features lsp-test-support --tests
cargo test --features lsp-test-support --test lsp_composite_stdio
```

## Real-server Tier 1 tests

With explicit binaries:

```bash
CODEGG_RA_BIN=/path/to/rust-analyzer \
  cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke rust_analyzer -- --nocapture

CODEGG_BASEDPYRIGHT_BIN=/path/to/basedpyright-langserver \
  cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke basedpyright -- --nocapture
```

Adapt command names to the actual server launch convention.

## Supervisor tests

```bash
cargo test -p egglsp --features lsp-test-support supervisor
cargo test -p egglsp --features lsp-test-support restart
cargo test -p egglsp --features lsp-test-support document_replay
```

## Full workspace validation

```bash
cargo fmt --check
cargo check --workspace --all-targets
cargo check --workspace --all-targets --all-features
cargo test --workspace
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

If real-server tests are intentionally skipped because binaries are unavailable, the test output must say so explicitly.

# Review Checklist

## Compatibility profiles

- [ ] rust-analyzer profile exists.
- [ ] pyright or basedpyright profile exists.
- [ ] Profiles own launch/readiness/configuration quirks.
- [ ] Generic client code has no server-ID conditionals.

## Real-server harness

- [ ] Tests are opt-in.
- [ ] Missing binaries skip explicitly.
- [ ] No test downloads servers.
- [ ] Fixture projects are temporary and deterministic.
- [ ] Every wait has a timeout.
- [ ] Version and stderr are captured.

## Tier 1 compatibility

- [ ] Initialize succeeds.
- [ ] Capability snapshot is recorded.
- [ ] Diagnostics behavior is observed.
- [ ] Symbols work.
- [ ] Definition or hover works.
- [ ] References work when advertised.
- [ ] Shutdown is clean.

## Health model

- [ ] Operational state enum exists.
- [ ] Transitions are centralized.
- [ ] Snapshot includes generation and freshness ages.
- [ ] Indexing/degraded states are distinct from transport failure.

## Supervisor

- [ ] One authoritative process monitor exists.
- [ ] Unexpected exit fails pending requests.
- [ ] Expected shutdown does not restart.
- [ ] Stderr tail is bounded.

## Restart policy

- [ ] Disabled by default.
- [ ] Backoff is bounded.
- [ ] Attempts are capped.
- [ ] Shutdown cancels restart timers.
- [ ] Stale generations cannot publish.
- [ ] Exhaustion yields stable failed state.
- [ ] Manual restart exists or is explicitly deferred.

## Document replay

- [ ] Open documents are tracked authoritatively.
- [ ] Latest text is replayed.
- [ ] Closed documents are not replayed.
- [ ] In-flight requests are not replayed.
- [ ] Diagnostics become stale across restart generations.

## Workflow adoption

- [ ] Semantic context reports indexing/degraded/restart state.
- [ ] Security/hunk context does not present stale evidence as fresh.
- [ ] Failures identify server and root.

## CI and reports

- [ ] Default CI remains network-free.
- [ ] Real-server workflow pins versions.
- [ ] JSON compatibility reports are produced.
- [ ] Tier 1 status is documented.

# Completion Criteria

Phase 3 is complete when:

1. Tier 1 real-server smoke tests pass for rust-analyzer and pyright/basedpyright on a documented platform.
2. Compatibility profiles isolate server-specific launch and readiness behavior.
3. Operational health states distinguish initialization, indexing, ready, degraded, restarting, failed, and stopped states.
4. Unexpected process exit is observed authoritatively.
5. Pending requests fail promptly on crash.
6. Restart policy is bounded, generation-safe, disabled by default, and deterministically tested.
7. Open documents replay successfully after restart.
8. Stale diagnostics and semantic evidence are marked across generations.
9. Root semantic/security/hunk workflows surface degraded/restarting state accurately.
10. Real-server CI is opt-in and version-pinned.
11. Compatibility reports record versions, timings, capabilities, and known limitations.
12. Existing scripted Phase 2 suites remain green.

## Handoff Result

After Phase 3, Codegg's LSP subsystem will move from deterministic protocol correctness to operational reliability against real language servers. It will have explicit compatibility profiles, measurable support status, bounded supervision and restart behavior, document replay, health/freshness reporting, and a controlled path for expanding LSP-backed workflows without hiding server instability.
