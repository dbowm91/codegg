# LSP Module

The `lsp` module provides Language Server Protocol support for IDE-like features. It implements a **client-side LSP integration** that spawns and manages external LSP server processes.

**Location**: `src/lsp/` (Codegg-side thin re-exports) and `crates/egglsp/` (full implementation; see [native_crates.md](native_crates.md))

## Key Responsibilities

- LSP server lifecycle management (download, launch, initialize)
- Diagnostics collection via publishDiagnostics notifications
- Code operations (goto definition, find references, hover, document symbols, workspace symbols, diagnostics)
- Preview-only semantic edits (renamePreview, formatPreview, sourceActionPreview) — returns unified-diff patches, never writes files
- Temporary overlays (semanticCheckPreview) — accepts full content or a single-file unified diff patch, applies it in memory via OverlaySession, collects diagnostics/symbols, restores disk view, never writes files
- Compact semantic context packets (semanticContext) — combines source excerpt, diagnostics, symbols, optional definition/reference/overlay information into a bounded pre-edit/pre-review context packet
- Security context packets (securityContext) — security-review context packet with deterministic risk markers, security-relevant diagnostics/symbols, optional call hierarchy, and optional overlay diagnostics
- Language detection from file extensions
- Project root detection
- Shallow call/type hierarchy queries (`callHierarchy`, `typeHierarchy`) — read-only, bounded, non-recursive relationship summaries for the symbol at a target position.
- Compact agent-facing output DTOs (not raw LSP JSON)

## Architecture

The full LSP implementation lives in the `egglsp` workspace crate
(`crates/egglsp/`). Codegg-side `src/lsp/mod.rs` is a thin wrapper
that re-exports `egglsp::*` and bridges:

- `crate::config::schema::LspConfig` → `egglsp::LspConfig` (via `From` impl in the wrapper)
- `egglsp::LspError` → `crate::error::LspError` (delegates to the existing codegg-side error variant)

The crate uses a client-per-root pattern: `LspService` maintains a `HashMap<String, ClientEntry>` where the key is `"{project_root}:{server_id}"`.

## Components

### compatibility.rs - Server Compatibility Profiles

Defines per-server compatibility profiles, readiness policies, and restart policies.

```rust
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

pub enum LspReadinessPolicy {
    InitializedIsReady,
    WaitForDiagnosticsOrTimeout { timeout: Duration },
    WaitForProgressEndOrTimeout { timeout: Duration },
    WarmupDelay { duration: Duration },
}

pub struct LspRestartPolicy {
    pub mode: LspRestartMode,
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub reset_after_healthy: Duration,
}

pub enum LspRestartMode {
    Disabled,
    OnUnexpectedExit,
}

pub struct LspServerVersion {
    pub raw: String,
    pub parsed: Option<String>,
}

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

pub enum CompatibilityCheckStatus {
    Passing,
    PassingWithKnownLimits,
    Failing,
    Skipped,
    Unsupported,
}

pub enum CompatibilityRequirement {
    Required,
    RequiredIfAdvertised,
    Optional,
    KnownLimitation,
}

pub struct LspCompatibilityCheck {
    pub name: String,
    pub status: CompatibilityCheckStatus,
    pub requirement: CompatibilityRequirement,
    pub detail: Option<String>,
    pub duration_ms: Option<u64>,
}
```

**Compatibility status model:**

| Status | Meaning |
|--------|---------|
| `Passing` | Server binary found, initializes, basic operations work |
| `PassingWithKnownLimits` | Server works but has documented limitations (e.g. no call hierarchy) |
| `Failing` | Server found but fails to initialize or produce valid responses |
| `Skipped` | Check was skipped (advertised feature not exercised) |
| `Unsupported` | Server binary not found on PATH and no download available |

**Compatibility requirement model (used by `assert_required_checks`):**

| Requirement | Behavior on failure |
|-------------|---------------------|
| `Required` | Test fails |
| `RequiredIfAdvertised` | Test fails if the server advertised the capability |
| `Optional` | Recorded but does not fail the test |
| `KnownLimitation` | Expected failure; not an error |

Key functions:

```rust
pub fn rust_analyzer_profile() -> LspCompatibilityProfile
pub fn pyright_profile() -> LspCompatibilityProfile
pub fn profile_for_server(server_id: &str) -> Option<LspCompatibilityProfile>
pub fn tier1_profiles() -> Vec<LspCompatibilityProfile>
pub async fn require_server_binary(server_id: &str) -> Result<PathBuf, LspError>
```

### health.rs - Operational Health State Machine

Tracks the operational state of each LSP server process through its lifecycle.

```rust
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

`generation` reflects the authoritative per-key generation from `LspService::generation_for_key`; it is bumped by the restart coordinator after a successful reinit + replay, never speculatively. `last_error` is populated only for `Failed { reason }` transitions; healthy clients keep it `None`. The `stderr_tail` is sourced from the live `LspProcessRuntime` and is empty when no runtime is installed. The snapshot is constructible without a live client (during `RestartScheduled`, `Restarting`, `Failed`, `Stopped`).

`LspOperationalState::context_note()` returns `None` for `Ready` and a bounded `Some("LSP state: ...")` for every other state. The note is appended to `SemanticContextResponse.notes`, `SecurityContextPacket.notes`, and hunk source context summary lines so root workflows expose the operational state explicitly.

**State transitions:**

```
Starting → Initializing → Indexing → Ready
Ready → Degraded → RestartScheduled → Restarting → Initializing
Starting/Initializing/Indexing/Ready → Failed → RestartScheduled
RestartScheduled → Restarting → Initializing
Ready → Stopping → Stopped
```

`transition()` is the authoritative validator. All state mutations go through `LspService::transition_operational_state(key, next)` which calls `transition()` and updates timestamps/error metadata. `InvalidTransition` is returned when a requested transition is not valid from the current state (e.g. `Starting` → `Ready` skips `Initializing`).

### supervisor.rs - Process Supervision

Provides process exit event tracking and stderr ring buffering for LSP server processes.

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

pub struct StderrRingBuffer {
    lines: Vec<String>,
    total_bytes: usize,
}
```

`StderrRingBuffer` is capped at 100 lines / 64KB. When the cap is exceeded, oldest lines are dropped. The buffer is drained during initialization to capture startup errors and surfaced in `LspProcessExitEvent.stderr_tail`. The `expected` flag is derived from `LspProcessIntent` at exit time, not from transport state.

### runtime.rs - Authoritative Process Runtime Owner

The single authoritative owner of an LSP server child process. One task owns the child handle, the bounded stderr ring buffer, the shutdown-intent receiver, and the kill channel. The monitor does **not** retain an `Arc<LspClient>` while awaiting the child.

```rust
pub enum LspProcessIntent {
    Running,
    GracefulShutdownRequested,
    ForceKillRequested,
}

pub struct LspProcessRuntime {
    pub server_id: String,
    pub root: PathBuf,
    pub generation: u64,
    pub intent_tx: watch::Sender<LspProcessIntent>,
    pub exit_rx: watch::Receiver<Option<LspProcessExitEvent>>,
    pub kill_tx: mpsc::Sender<()>,
}

pub fn spawn_process_runtime(
    server_id: String,
    root: PathBuf,
    generation: u64,
    child: tokio::process::Child,
    stderr: tokio::process::ChildStderr,
) -> (LspProcessRuntime, tokio::task::JoinHandle<()>)
```

`LspProcessRuntime` is the runtime handle; `spawn_process_runtime` returns it together with the owner's `JoinHandle`. The owner task uses `tokio::select!` over `child.wait()`, the kill channel, and runtime cancellation, then publishes exactly one `LspProcessExitEvent` and terminates. A bounded stderr-reader task appends each line to the shared `StderrRingBuffer` until EOF or cancellation.

`LspClient::shutdown()` sets the intent to `GracefulShutdownRequested`, sends `shutdown` / `exit`, awaits the runtime exit under a bounded deadline, then `ForceKillRequested` and a force kill on timeout. Hung processes are force-killed and reaped.

Expected-vs-unexpected exit is determined by `LspProcessIntent::is_expected()` (true for `GracefulShutdownRequested` or `ForceKillRequested`). Transport state never determines expectedness. A zero exit with no shutdown intent is still unexpected.

### restart.rs - Restart Descriptor and Coordinator

The single source of truth for restart retry/backoff/exhaustion/cancellation. Manual and automatic restart call the same coordinator.

```rust
pub struct LspClientDescriptor {
    pub key: String,
    pub server_id: String,
    pub root: PathBuf,
    pub launch_spec: LspLaunchSpec,
    pub initialization_options: Option<serde_json::Value>,
    pub workspace_configuration: serde_json::Value,
    pub readiness_policy: LspReadinessPolicy,
    pub restart_policy: LspRestartPolicy,
    pub seed_file: Option<PathBuf>,
}

pub enum RestartTrigger {
    Automatic, // honors restart policy (Disabled => no-op)
    Manual,    // always runs
}

pub trait RestartShared { /* service-internal surface */ }

pub fn backoff_delay(attempt: u32, policy: &LspRestartPolicy) -> Duration
pub async fn restart_client_coordinator<S, F>(...) -> Result<(), LspError>
```

`LspClientDescriptor` persists the per-client launch spec on first publish from the server definition, the user config rule, the resolved launch spec, and the compatibility profile. Resolution priority: explicit user config → profile default → server definition default. Restart reconstructs from the descriptor directly — no language detection, no `src/lib.rs` synthesis. The seed file is overwritten by the first currently open document for the key before calling `reinit_fn`.

`restart_client_coordinator<S, F>` owns generation increment, restart-state transition, current-client removal, old runtime shutdown, retry/backoff loop, client reinitialization from the descriptor, readiness wait, document replay, ownership restoration, diagnostics stale marking, and final `Ready` / `Failed` transition. The coordinator aborts with `LspError::ServerRestarted` if a newer generation is observed at any boundary (cancel-pending check, before-spawn gate, post-spawn re-check, or post-replay re-check). On exhausted retries it transitions the operational state to `Failed { reason }` and returns `LspError::LaunchFailed("restart attempts exhausted (max=N)")`.

`backoff_delay(attempt, policy)` is `min(policy.initial_backoff * 2^(attempt-1), policy.max_backoff)`. The 1-indexed `attempt` means attempt 1 is the first try, which still gets `initial_backoff` per the policy-driven algorithm. `reset_after_healthy` lazily resets `restart_attempts` to 0 when the next unexpected exit observes a healthy client.

#### User-configurable restart

User-configurable restart overrides profile defaults via the `[lsp.<server>.restart]` TOML section. The `LspRestartPolicyConfig` struct (in `crates/egglsp/src/config.rs`, mirrored in `crates/codegg-config/src/schema.rs`) has optional fields: `mode`, `max_attempts`, `initial_backoff_ms`, `max_backoff_ms`, `reset_after_healthy_secs`. `LspClientDescriptor::from_profile` merges non-None user fields into the profile defaults using `merge_with_profile()`, so explicit user config wins over profile defaults over server-definition defaults. The merged result is persisted in the `descriptor_map` and read by the restart coordinator on each restart attempt.

### document_sync.rs - Document Replay Registry

Tracks open documents so they can be replayed after a server restart.

```rust
pub struct OpenDocumentRegistry {
    documents: HashMap<String, OpenDocumentSnapshot>,
}

pub struct OpenDocumentSnapshot {
    pub uri: String,
    pub language_id: String,
    pub version: i32,
    pub text: String,
}
```

On restart the coordinator replays `didOpen` for every open snapshot using the snapshot's preserved per-document version (not hard-coded 1), restores the `document_owners` map for each URI, updates the new client's `opened_files` state, and keeps registry entries intact. Closed documents are not replayed. Replay failure transitions the operational state to `Degraded` (not silent `Ready`).

### Generation and Stale-Evidence Semantics

Per-client generation is tracked in `generation_map: Arc<Mutex<HashMap<String, u64>>>` and accessed via `LspService::generation_for_key(key)` / `set_generation(key, gen)`. The first publish sets generation `1`; the restart coordinator bumps it after a successful reinit + replay.

- Stale exit events whose `event.generation != current_generation` are silently dropped by `LspService::handle_exit_event`. Old exit events cannot fail a newer client.
- Restart publication rechecks the expected generation before publishing and aborts with `LspError::ServerRestarted` if a newer generation is observed.
- `DiagnosticCacheEntry.server_generation: u64` (0 is the "never assigned" sentinel) and `post_restart: bool` (monotonically sticky once a restart has been observed) are stamped on every cache entry. `LspDiagnosticSnapshot` exposes both fields; the root `SemanticContextCollector` propagates them to `SemanticDiagnosticEvidence`.
- On restart, `mark_diagnostics_stale_for_key(key)` rewrites retained entries' `server_generation` to `current - 1` so the freshness classifier returns `LspDiagnosticFreshness::Stale` until the new server emits its first push.

### Readiness and Operational Notes

`LspService::wait_for_readiness(key, policy)` honors all four `LspReadinessPolicy` variants and returns `ReadinessResult::Ready { elapsed }` or `Degraded { reason, elapsed }`. The four variants drive the production `Indexing` → `Ready` and timeout → `Degraded` transitions:

| Variant | Behavior |
|---------|----------|
| `InitializedIsReady` | Return `Ready` immediately after `initialized` notification |
| `WaitForDiagnosticsOrTimeout { timeout }` | Wait for first `publishDiagnostics` or timeout |
| `WaitForProgressEndOrTimeout { timeout }` | Wait for a `$window/workDoneProgress` end notification or timeout |
| `WarmupDelay { duration }` | Fixed warmup delay after initialization |

`LspClient` tracks `ProgressState` (active `$/progress` tokens + last progress timestamp) and exposes `progress_snapshot()`, `wait_for_progress_end(timeout)`, `wait_for_first_diagnostics(timeout)`, and `operational_summary()`. These back the `WaitForProgressEndOrTimeout` and `WaitForDiagnosticsOrTimeout` policies.

`LspOperationalState::context_note()` returns `None` for `Ready` and a bounded `Some("LSP state: ...")` for every other state. The note is appended to `SemanticContextResponse.notes`, `SecurityContextPacket.notes`, and hunk source context summary lines so root workflows expose the operational state explicitly. Restarting/failed/degraded states are not silently treated as ready.

### src/lsp/mod.rs - Codegg-side thin wrapper

```rust
pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}

impl Lsp {
    pub async fn open_file(&self, path: &Path, content: &str) -> Result<(), LspError>
    pub async fn update_file(&self, path: &Path, content: &str) -> Result<(), LspError>
    pub async fn close_file(&self, path: &Path) -> Result<(), LspError>
    pub async fn save_file(&self, path: &Path, content: Option<&str>) -> Result<(), LspError>
    pub async fn shutdown(&self)
}
```

### service.rs - Client Management

```rust
pub struct LspService {
    clients: Arc<RwLock<HashMap<String, ClientEntry>>>,
    initializing: Arc<Mutex<HashMap<String, InitSlot>>>,
    active_init_tasks: Arc<Mutex<HashMap<u64, InitTaskControl>>>,
    document_owners: Arc<RwLock<HashMap<String, String>>>,
    document_registry: Arc<OpenDocumentRegistry>,
    operational_state: Arc<RwLock<HashMap<String, OperationalServerState>>>,
    generation_map: Arc<Mutex<HashMap<String, u64>>>,
    exit_tx: tokio::sync::mpsc::Sender<LspProcessExitEvent>,
    exit_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<LspProcessExitEvent>>>>,
    exit_receiver_started: Arc<AtomicBool>,
    runtime_map: Arc<Mutex<HashMap<String, RuntimeEntry>>>,
    descriptor_map: Arc<Mutex<HashMap<String, LspClientDescriptor>>>,
    self_ref: OnceLock<Weak<LspService>>,
    lifecycle: Arc<RwLock<LifecycleState>>,
    lifecycle_tx: watch::Sender<LifecycleState>,
    config: LspConfig,
}
```

`LspService::new(config)` is the **crate-private** (test-only) bare constructor — it returns a `Self` without the cyclic back-reference wired, so the exit-receiver task is NOT auto-started. It is restricted to `pub(crate)` so production callers cannot accidentally create an un-supervised service. `LspService::new_arc(config) -> Arc<Self>` is the production constructor: it builds the service via `Arc::new_cyclic(|weak| Self { ..., self_ref: OnceLock::from(weak.clone()), ... })`, which wires the back-reference and guarantees `ensure_exit_receiver_started` can self-activate from `&self` callers. The test `new_arc_wires_self_ref` proves the production constructor populates `self_ref` (read via the `Weak` upgrade). No public production path creates an un-supervised service.

`generation_map` is the per-key generation map. `LspService::generation_for_key(key)` and `LspService::set_generation(key, gen)` are the public accessors. The first publish sets generation `1`; the restart coordinator bumps it after a successful reinit + replay.

`transition_operational_state(key, next)` is the centralized state mutator. It calls `health::transition()` to validate the move and updates timestamps/error metadata. All state assignments throughout the service, restart coordinator, and shutdown code go through this helper; direct assignments are not allowed.

Exit metadata is persisted per-key in the `runtime_map` and `descriptor_map` so that operational health snapshots remain available even after a client is removed (during `RestartScheduled`, `Restarting`, `Failed`, `Stopped`). The `LspOperationalHealthSnapshot` carries the authoritative `generation` from `generation_map`, real `last_message_age_ms` / `last_diagnostics_age_ms`, `restart_attempts`, and any `last_error` from the most recent `Failed` transition. `stderr_tail` is sourced from the live `LspProcessRuntime` and is empty when no runtime is installed.

`runtime_map` and `descriptor_map` hold the per-key `LspProcessRuntime` handle and persisted `LspClientDescriptor` respectively. The descriptor is built by `LspClientDescriptor::from_profile(...)` with explicit `user > profile > server-definition` priority and read by the restart coordinator to seed a new client.

`InitTaskControl` holds the authoritative terminal completion primitive for each spawned initialization task:

- `attempt_id: u64` — unique per-attempt monotonic counter
- `cancellation: CancellationToken` — cooperative cancellation signal
- `abort_handle: tokio::task::AbortHandle` — forced-abort primitive for stragglers
- `completion: oneshot::Receiver<InitTaskExit>` — **authoritative** terminal signal owned by the wrapper task

The completion receiver is the only authoritative source of truth for "the wrapper task has terminated". The wrapper task owns the paired `Sender` and is required to either send exactly one `InitTaskExit` (`Completed`, `Panicked(String)`, or `Cancelled`) before exiting, or be dropped (which closes the channel and resolves the receiver with `Err`). Shutdown never wraps the real `JoinHandle` in a forwarding task — the receiver is the completion primitive.

Lock ordering: the clients map lock must be acquired before any client-level lock.
Documented on the struct for future contributors.

```rust
impl LspService {
    pub fn new(config: LspConfig) -> Self
    pub fn new_arc(config: LspConfig) -> Arc<Self>  // production: wires back-reference
    pub async fn ensure_exit_receiver_started(self: &Arc<Self>)
    pub async fn get_or_create_client(&self, file_path: &Path) -> Result<(String, PathBuf), LspError>
    pub async fn get_or_create_client_for_file(&self, file_path: &Path) -> Result<(String, PathBuf), LspError>
    pub async fn ensure_file_open_from_disk(&self, file_path: &Path) -> Result<(String, PathBuf), LspError>
    pub async fn find_existing_client_for_root_hint(&self, root_hint: Option<&Path>, server_id: Option<&str>) -> Result<(String, PathBuf), LspError>
    pub async fn open_file(&self, file_path: &Path, text: &str) -> Result<(), LspError>
    pub async fn update_file(&self, file_path: &Path, text: &str) -> Result<(), LspError>
    pub async fn close_file(&self, file_path: &Path) -> Result<(), LspError>
    pub async fn save_file(&self, file_path: &Path, text: Option<&str>) -> Result<(), LspError>
    pub async fn shutdown_all(&self)
    pub async fn generation_for_key(&self, key: &str) -> u64
    pub async fn set_generation(&self, key: &str, generation: u64)
    pub async fn operational_state_for_key(&self, key: &str) -> Option<LspOperationalState>
    pub async fn wait_for_readiness(&self, key: &str, policy: LspReadinessPolicy) -> ReadinessResult
    pub async fn mark_diagnostics_stale_for_key(&self, key: &str)
    pub async fn operational_health_snapshot(&self, key: &str) -> LspOperationalHealthSnapshot
    pub async fn restart_client(&self, key: &str) -> Result<(), LspError>
    pub async fn descriptor_for_key(&self, key: &str) -> Option<LspClientDescriptor>
    pub async fn set_descriptor_for_key(&self, key: &str, descriptor: LspClientDescriptor)
}
```

**`save_file` freshness tracking**: When `save_file` is called with text content (`text: Some(...)`), it updates the `last_content_change_at` timestamp for the file, marking diagnostics as potentially stale since the server may recompute diagnostics for the new content. A bare save (`text: None`) sends the `didSave` notification without affecting freshness.

### client.rs - LSP Client

Manages JSON-RPC communication with a single LSP server process. A dedicated background reader task owns stdout and routes responses via the `pending` map while independently dispatching notifications (e.g. `publishDiagnostics`):

```rust
pub struct LspClient {
    pub server_id: String,
    pub root: PathBuf,
    pub process: tokio::sync::Mutex<LspProcess>,
    pub request_id: AtomicU64,
    pub capabilities: Mutex<Option<ServerCapabilities>>,
    pub opened_files: Mutex<HashMap<String, i32>>,
    pub last_opened_at: Mutex<HashMap<String, Instant>>,
    pub diagnostics: Arc<Mutex<HashMap<String, Vec<lsp_types::Diagnostic>>>>,
    pub pending: PendingMap,
    _reader_task: tokio::task::JoinHandle<()>,
}

pub struct DiagnosticEntry { ... } // internal
```

**Key operations**:
- File lifecycle: `open_file()`, `update_file()`, `close_file()`, `save_file()`, `ensure_file_open_from_disk()`
- Code intelligence: `go_to_definition()`, `find_references()`, `hover()`, `document_symbols()`, `code_actions()`, `completion()`, `signature_help()`, `code_lens()` (internal), plus preview-only `rename_preview()` / `format_preview()` (see edit.rs)
- Diagnostics: `get_diagnostics()`, `get_all_diagnostics()`, `diagnostics_may_still_be_warming()`
- Communication: `send_request()`, `send_notification()`, `send_initialized()`
- Utilities: `url_to_uri()`, `detect_language_id()`, `classify_json_rpc_message`, `dispatch_notification`

`notif_tx`/`notif_rx` and direct `read_response`/`read_notification` paths have been removed; stdout is exclusively owned by the background reader.

### operations.rs - High-Level Operations

```rust
pub struct LspOperations {
    service: Arc<LspService>,
}

impl LspOperations {
    pub async fn go_to_definition(&self, file_path: &Path, line: u32, column: u32) -> Result<Vec<LocationLink>, LspError>
    pub async fn find_references(&self, file_path: &Path, line: u32, column: u32) -> Result<Vec<Location>, LspError>
    pub async fn hover(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>
    pub async fn document_symbols(&self, file_path: &Path) -> Result<Vec<DocumentSymbol>, LspError>
    pub async fn code_actions(&self, file_path: &Path, start_line: u32, start_col: u32, end_line: u32, end_col: u32, diagnostics: Vec<Diagnostic>, only: Option<Vec<CodeActionKind>>) -> Result<Vec<CodeActionOrCommand>, LspError>
    pub async fn completion(&self, file_path: &Path, line: u32, column: u32, trigger_kind: Option<CompletionTriggerKind>, trigger_char: Option<String>) -> Result<Vec<CompletionItem>, LspError>
    pub async fn signature_help(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<String>, LspError>
    pub async fn code_lens(&self, file_path: &Path) -> Result<Vec<CodeLens>, LspError>  // internal, not model-facing
    pub async fn prepare_rename(&self, file_path: &Path, line: u32, column: u32) -> Result<Option<PrepareRenameResponse>, LspError>
    pub async fn rename_preview(&self, file_path: &Path, line: u32, column: u32, new_name: &str, allowed_root: Option<&Path>) -> Result<WorkspaceEditPreview, LspError>
    pub async fn format_preview(&self, file_path: &Path, allowed_root: Option<&Path>) -> Result<WorkspaceEditPreview, LspError>
    pub async fn source_action_preview(&self, file_path: &Path, action: SourceActionPreviewKind, allowed_root: Option<&Path>) -> Result<WorkspaceEditPreview, LspError>
    pub async fn semantic_check_preview(&self, file_path: &Path, content: &str, allowed_root: Option<&Path>) -> Result<SemanticCheckPreview, LspError>
}
```

**Note**: The `LspOperations::completion` method handles both LSP response types - `CompletionList` (a structured list with `isIncomplete` flag) and plain `Vec<CompletionItem>`. It first attempts to deserialize as `CompletionList`, and if that fails, falls back to parsing as a `Vec<CompletionItem>`. This fallback is handled at the operations layer; the lower-level `LspClient::completion` only handles `CompletionList`.

### diagnostics.rs - Diagnostics Collection

```rust
const DEBOUNCE_MS: u64 = 150;

#[derive(Debug, Clone)]
pub struct FileDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub message: String,
    pub severity: DiagnosticSeverity,
    pub source: Option<String>,
    pub code: Option<String>,
}

pub struct DiagnosticsCollector {
    service: Arc<LspService>,
    last_update: Arc<Mutex<HashMap<String, Instant>>>,
}

impl DiagnosticsCollector {
    pub async fn should_debounce(&self, uri: &str) -> bool
    pub async fn get_diagnostics_for_file(&self, file_path: &Path) -> Result<Vec<FileDiagnostic>, LspError>
    pub async fn get_all_diagnostics(&self) -> Result<HashMap<String, Vec<FileDiagnostic>>, LspError>
    pub async fn has_errors(&self, file_path: &Path) -> Result<bool, LspError>
}
```

### download.rs - Binary Download

```rust
pub async fn ensure_server_binary(server: &LspServerDef) -> Result<PathBuf, LspError>
pub fn cache_dir() -> PathBuf

async fn find_in_path(cmd: &str) -> Option<PathBuf>
async fn is_executable(path: &Path) -> bool
async fn download_server(server: &LspServerDef, spec: &DownloadSpec, dest: &Path) -> Result<PathBuf, LspError>
fn resolve_url(spec: &DownloadSpec) -> String
fn extract_zip(data: &[u8], dest: &Path, binary_name: &str) -> Result<PathBuf, LspError>
fn extract_tar_gz(data: &[u8], dest: &Path, binary_name: &str) -> Result<PathBuf, LspError>
fn extract_tar_xz(data: &[u8], dest: &Path, binary_name: &str) -> Result<PathBuf, LspError>
```

1. First checks PATH for binary
2. Falls back to cached download in `$HOME/.cache/codegg/lsp/`
3. Only rust-analyzer has download specification currently
4. Supports Zip, TarGz, TarXz, and Raw archive types

### launch.rs - Process Spawning

```rust
pub struct LspProcess {
    pub stdin: tokio::process::ChildStdin,
    pub stdout: tokio::process::ChildStdout,
    pub stderr: Option<BufReader<tokio::process::ChildStderr>>,
    pub child: tokio::process::Child,
}

pub async fn spawn_server(command: &str, args: &[&str], env: &[(String, String)], cwd: Option<&Path>) -> Result<LspProcess, LspError>
pub async fn send_request(process: &mut LspProcess, msg: &str) -> Result<(), LspError>
pub fn spawn_stderr_drain(server_id: &str, stderr: tokio::process::ChildStderr)
pub async fn terminate(process: &mut LspProcess)
fn parse_content_length(header: &str) -> Option<usize>
```

Uses Content-Length headers for LSP message framing. Preserves user's PATH from environment. Stderr is drained in a background task (capped at 64KB) to prevent blocking initialization. `read_response` and `read_notification` have been removed; stdout is exclusively owned by the background reader task in `client.rs`.

### language.rs - Language Detection

```rust
pub fn detect_language(path: &str) -> Option<&'static str>
pub fn extension_to_language_id(ext: &str) -> Option<&'static str>
pub fn language_id_to_server_id(lang_id: &str) -> Option<&'static str>
```

Supports ~80 extensions including Rust, Python, JavaScript/TypeScript, Go, Java, C/C++, C#, Ruby, Kotlin, Scala, Dart, Swift, Haskell, Lua, PHP, Perl/Raku, and more.

### root.rs - Project Root Detection

```rust
pub fn find_project_root(start: &Path) -> Option<PathBuf>
```

Detects project roots by looking for marker files like `.git`, `Cargo.toml`, `package.json`, etc.

### server.rs - Server Definitions

```rust
pub struct LspServerDef {
    pub id: &'static str,
    pub languages: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub repo: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub download: Option<DownloadSpec>,
}

pub struct DownloadSpec {
    pub url_template: &'static str,
    pub archive_type: ArchiveType,
    pub binary_name: &'static str,
}

pub enum ArchiveType {
    Zip,
    TarGz,
    TarXz,
    Raw,
}

pub fn server_definitions() -> &'static [LspServerDef]
pub fn find_server(id: &str) -> Option<&'static LspServerDef>
pub fn find_server_for_language(lang: &str) -> Option<&'static LspServerDef>
pub fn find_server_for_extension(ext: &str) -> Option<&'static LspServerDef>

```

### SemanticContextCollector

**Location:** `src/lsp/semantic_context.rs`

A collector/builder that assembles the shared semantic read model for `semanticContext`. It produces `egglsp::semantic_context::SemanticContextResponse` by collecting the shared evidence needed for source excerpt, diagnostics, symbols, definitions, references, and hierarchy summaries from LSP services. Source-action hints and overlay translation are not part of the collector — they remain handler-local.

```rust
pub struct SemanticContextCollector {
    service: Arc<LspService>,
    operations: Arc<LspOperations>,
    diagnostics: Arc<DiagnosticsCollector>,
    allowed_root: PathBuf,
}

impl SemanticContextCollector {
    pub fn new(service, operations, diagnostics, allowed_root) -> Self;
    pub async fn collect(&self, request: SemanticContextRequest)
        -> Result<SemanticContextResponse, String>;
}
```

The collector handles:
- Source excerpt construction (file reading + byte-limited truncation)
- Diagnostic snapshot collection with freshness metadata
- Document symbol flattening and capping
- Definition/reference gathering with capability gating
- Source-action preview hints
- Call/type hierarchy summaries (capability-gated)
- Per-section truncation metadata
- Structured unavailable metadata via `LspCapabilitySnapshot`

Overlay resolution stays handler-local because patch/content expansion is tool-specific; the shared semantic read model carries the resulting overlay summary when the handler chooses to attach one.

Unit tests use fake/static inputs and do not require live LSP servers. Hierarchy flag wiring tests (`semantic_context_request_sets_call_hierarchy_flag`, etc.) are unit-level: they verify request construction and `SemanticContextPacket::from_semantic_response` adapter behavior with static `SemanticContextResponse` fixtures. Root composite tests in `tests/lsp_composite_stdio.rs` exercise the real `SemanticContextCollector` against a fake LSP server end-to-end, covering the full workflow, capability gating, and failure degradation paths. Production preview conversion (rename, format, source-action) is tested through the same composite harness, confirming that `WorkspaceEditPreview` and `FileEditPreview` round-trip correctly through the production `LspClient`/`LspOperations`/`LspService` stack.

## Supported Languages (39 servers)

| Language | Server | Command |
|----------|--------|---------|
| Rust | rust-analyzer | rust-analyzer |
| Python | pyright | pyright-langserver --stdio |
| JavaScript/TypeScript | typescript-language-server | typescript-language-server --stdio |
| Go | gopls | gopls |
| C/C++ | clangd | clangd |
| Java | jdtls | jdtls |
| C# | omnisharp | OmniSharp |
| Ruby | ruby-lsp | ruby-lsp |
| Kotlin | kotlin-language-server | kotlin-language-server |
| Scala | metals | metals |
| Dart | dart-analysis-server | dart language-server --client-id codegg |
| Swift | swift-sourcekit | sourcekit-lsp |
| Haskell | haskell-language-server | haskell-language-server-wrapper --lsp |
| Lua | lua-language-server | lua-language-server |
| PHP | php-language-server | php-language-server |
| Perl/Raku | perl-language-server | perl-language-server |
| Zig | zls | zls |
| V | vls | vls |
| Nim | nimlsp | nimlsp |
| R | r-languageserver | R --slave -e library(languageserver) |
| ... and more | | |

## Tool Integration

LSP is exposed via `LspTool` in `src/tool/lsp.rs`. The tool returns compact agent-facing summaries, not raw LSP JSON.

### Exposed Operations

Only these operations are model-facing:

| Operation | LSP Request | Output Shape |
|-----------|-------------|--------------|
| `goToDefinition` | `textDocument/definition` | `Vec<LocationSummary>` |
| `findReferences` | `textDocument/references` | `Vec<LocationSummary>` (capped at 100) |
| `hover` | `textDocument/hover` | `HoverSummary` (capped at 2000 chars) |
| `documentSymbol` | `textDocument/documentSymbol` | `Vec<SymbolSummary>` (capped at 300) |
| `workspaceSymbol` | `workspace/symbol` | Compact summary list |
| `diagnostics` | (via DiagnosticsCollector) | `Vec<DiagnosticSummary>` (plus warming flag) |
| `renamePreview` | `textDocument/rename` (after ensure open + optional prepareRename) | `WorkspaceEditPreview` (unified diff patches + metadata; preview-only) |
| `formatPreview` | `textDocument/formatting` | `WorkspaceEditPreview` (unified diff patches; preview-only) |
| `sourceActionPreview` | `textDocument/codeAction` (filtered to `source.organizeImports`; full-document range computed from synced file contents) | `WorkspaceEditPreview` (unified diff patches; preview-only) |
| `semanticCheckPreview` | `textDocument/didChange` (OverlaySession + restore) + `textDocument/documentSymbol` | `SemanticCheckPreview` (diagnostics + symbols + error fields; accepts `content` or single-file `patch`, preview-only, no disk writes) |
| `semanticContext` | (combines multiple LSP requests) | `SemanticContextPacket` (source excerpt + diagnostics + symbols + optional definitions/references/overlay + optional source-action hints + optional call/type hierarchy; read-only, never writes files) |
| `securityContext` | (combines multiple LSP requests + risk marker scanning) | `SecurityContextPacket` (source excerpt + risk markers + security-relevant diagnostics/symbols + optional definitions/references/call hierarchy + optional overlay; read-only, never writes files) |
| `hunkSourceContext` | (combines diff parsing + semantic context) | `HunkSourceNavigationResponse` (per-hunk evidence with enclosing symbols, diagnostics, definitions, references; read-only, bounded) |
| `callHierarchy` | `textDocument/prepareCallHierarchy` + `callHierarchy/incomingCalls` + `callHierarchy/outgoingCalls` | `CallHierarchySummary` (items, incoming, outgoing, errors, truncated) |
| `typeHierarchy` | `textDocument/prepareTypeHierarchy` + `typeHierarchy/supertypes` + `typeHierarchy/subtypes` | `TypeHierarchySummary` (items, supertypes, subtypes, errors, truncated) |

`codeLens` is intentionally not exposed in the model-facing schema (remains available in `egglsp::operations` only).

**LSP edit previews are strictly read-only**: `renamePreview`/`formatPreview` (and any future preview ops) return bounded unified-diff patches via `WorkspaceEditPreview` (title, per-file original_hash + TextEditPreview + patch). They never write files. Actual mutation requires the separate mutating `apply_patch` tool (or equivalent). The `lsp` tool remains `ToolCategory::ReadOnly`.

### Preview-only edits

`renamePreview`, `formatPreview`, and `sourceActionPreview` request semantic edits from the language server, convert them into `WorkspaceEditPreview`, and return unified diff patches. They never write files. `sourceActionPreview` currently supports only `source.organizeImports` (with aliases `organizeImports` and `organize_imports`); arbitrary code actions and command execution are intentionally rejected. `CodeAction` values with `command: Some(_)` but `edit: None` are classified as command-only and rejected (command execution is disabled for safety). `format_preview` enforces `allowed_root` at the crate layer — paths outside the root are rejected with `LspError::PathOutsideRoot`. Large patches are structurally marked via `FileEditPreview.patch_omitted` (not by string matching). Applying a preview requires the existing mutating `apply_patch` tool and therefore follows normal Codegg permission handling. `semanticContext` can also include source-action hints (currently limited to `source.organizeImports`) when `include_source_actions` is true, reusing the same preview-only semantics described above. Source-action hints are collected handler-locally by `LspTool::collect_source_action_hints`, not by the shared `SemanticContextCollector`, because they produce `WorkspaceEditPreview` payloads that are preview-rich and tool-specific.

Hidden operations (in `egglsp::operations` for internal use only, not model-facing): `completion`, `signatureHelp`, `codeAction` (arbitrary code actions), `codeLens`, and `goToImplementation`. The `source.organizeImports` source action is the only source action exposed to the model via `sourceActionPreview`.

### Temporary overlays

`semanticCheckPreview` accepts either full proposed `content` or a single-file unified diff `patch`. The patch is applied in memory against `file_path` via `OverlaySession`, gathers diagnostics/symbols, then restores the LSP view back to the current disk content. This allows pre-apply semantic checks without writing files. Multi-file patches are unsupported in this pass. The operation is read-only from Codegg's filesystem permission perspective.

`OverlaySession::apply_overlay(file_path, proposed_text)` reads disk content, opens the file in LSP, sends `didChange` with the proposed content, and returns an `OverlayRestoreToken` capturing the original text, file path, key, and URI. `OverlaySession::restore(token)` sends `didChange` restoring the original disk content. The proposed content is never written to disk; patch input follows the same in-memory path after being expanded against `file_path`.

The overlay flow:
1. `OverlaySession::apply_overlay` reads disk content and sends `didChange` with proposed content
2. Wait 250ms for diagnostics debounce
3. Collect diagnostics and document symbols
4. `OverlaySession::restore` sends `didChange` restoring disk content
5. Return results (diagnostics, symbols, `restored_disk_view` flag, error fields)

Operation-level root enforcement: `semantic_check_preview` accepts `allowed_root: Option<&Path>` and rejects files outside the root with `LspError::PathOutsideRoot`.

Restore runs even if diagnostics or symbol collection fails. Restore failures are logged and surfaced via `restore_error: Option<String>` in the response (and `restored_disk_view: false`). `SemanticCheckPreview` also includes `diagnostics_error: Option<String>` and `symbols_error: Option<String>` — each is non-None when the corresponding LSP request fails, replacing previously swallowed empty-vector fallbacks. `diagnostics_may_still_be_warming` indicates the LSP server may not have fully processed the overlay yet. Diagnostics may be warming or stale (publishDiagnostics is async). The operation is single-file in the first pass; multi-file overlays are unsupported in this pass.

### Hierarchy operations

`callHierarchy` and `typeHierarchy` are read-only code-intelligence operations. They require `file_path`, `line`, and `column`. Both operations default to `direction="both"`.

`callHierarchy` maps:
- `incoming` → callers of the target symbol
- `outgoing` → calls made by the target symbol

`typeHierarchy` maps:
- `incoming` → supertypes
- `outgoing` → subtypes

The first implementation is shallow and non-recursive. It prepares the target hierarchy item and requests immediate relationships only. Unsupported language servers may return empty sections or per-section error fields.

Hierarchy `from_ranges` truncation (capped at `MAX_HIERARCHY_RANGES = 32` per call) is now included in the summary `truncated` flag. The `truncated` field is `true` when items, edges, or ranges exceed their caps.

Hierarchy prepare operations use `ensure_file_open_from_disk` to open/sync the file from disk before sending the prepare request, ensuring position-sensitive behavior against a document view known to the server.

`semanticContext` can include hierarchy sections with `include_call_hierarchy=true` or `include_type_hierarchy=true`. These flags require `line` and `column`; requests without a target position are rejected.

`securityContext` requests shared call hierarchy from `SemanticContextCollector` when `include_call_hierarchy` is enabled and a target position is supplied; type hierarchy is not currently part of security context. Both `semanticContext` and `securityContext` gate hierarchy calls through `LspCapabilitySnapshot`; unsupported operations are skipped and surfaced as notes or empty sections rather than failing the packet.

### Semantic context packets

`semanticContext` is the preferred agent-facing pre-edit/pre-review context operation. It combines a bounded source excerpt with current diagnostics, document symbols, optional definition/reference information, optional overlay diagnostics for proposed content or a single-file patch, optional source-action hints, and optional call/type hierarchy information. It is read-only and never applies changes.

The shared semantic read model is assembled by `SemanticContextCollector`. Overlay translation remains handler-local by design: patch/content expansion is tool-specific (the handler resolves the overlay via `semanticCheckPreview` and attaches the resulting summary), so the collector never handles overlay requests or responses. `securityContext` reuses the same diagnostic freshness evidence and capability snapshot, but filters results into a security-specific packet instead of a general semantic summary.

Input parameters:
- `file_path` (required): file to analyze
- `line`, `column` (optional, both-or-neither): 1-indexed target position for definitions/references and hierarchy
- `radius` (optional, default 40, max 120): lines above/below target for source excerpt
- `include_references` (optional, default true when line+column): include findReferences results
- `include_definitions` (optional, default true when line+column): include goToDefinition results
- `include_overlay` (optional, default true when content/patch provided): include overlay diagnostics
- `include_source_actions` (optional, default false): include source-action hints (e.g. `source.organizeImports`) in the packet; each hint is a `SemanticSourceActionHint` with `action`, `available`, `preview` (optional `WorkspaceEditPreview`), and `error` (optional); failures are per-hint and do not fail the whole packet
- `include_call_hierarchy` (optional, default false): include call hierarchy information (requires line+column); requests without a target position are rejected with a validation error
- `include_type_hierarchy` (optional, default false): include type hierarchy information (requires line+column); requests without a target position are rejected with a validation error
- `content` / `patch` (optional, mutually exclusive): proposed content for overlay diagnostics

All output sections are bounded:
- Diagnostics: capped at 100
- Symbols: capped at 120
- References: capped at 80
- Overlay diagnostics: capped at 100 (included in `overlay_diagnostics_truncated` limit)
- Source excerpt: capped at 32KB text

The operation gathers existing read-only semantic facts, optionally runs an overlay semantic check, and returns a stable JSON DTO. All sections are best-effort: individual failures do not prevent the rest of the packet from being returned. Per-section errors are surfaced as `definitions_error: Option<String>` and `references_error: Option<String>` (non-None when the corresponding LSP request fails). `result_count` includes overlay diagnostics and overlay symbols in addition to the base counts. Source excerpt truncation is UTF-8-safe — it cuts at character boundaries using `truncate_to_byte_limit_on_char_boundary`, avoiding replacement characters or partial-codepoint corruption. `execute_structured` checks both `/results/restore_error` and `/results/overlay/restore_error` for success detection.

> **Architecture note:** `SemanticContextPacket` is a tool-local presentation type. `SemanticContextCollector` assembles the shared semantic read model, and `SemanticContextPacket::from_semantic_response()` adapts that response into the tool-local packet. Overlay resolution stays handler-local.

### Security context packets

`securityContext` is a read-only context-gathering operation for security review. It is not a vulnerability scanner and does not produce vulnerability verdicts. It never writes proposed content to disk; patch/content input is applied only in memory through the existing semantic overlay path.

Risk markers are deterministic keyword/identifier/context matches with rationale strings. They are prompts for review, not evidence of a confirmed vulnerability.

It combines:
- bounded source excerpt (configurable radius, default 80, max 200);
- deterministic risk markers via pattern matching (11 categories);
- security-relevant diagnostics and symbols (filtered by keyword matching and proximity to risk markers);
- definitions and references when a target position is supplied;
- shallow call hierarchy when a target position is supplied;
- optional overlay diagnostics for proposed full content or a single-file patch.

**Supported risk marker categories:** `auth`, `crypto`, `filesystem`, `network`, `process`, `unsafe`, `serialization`, `sql`, `secrets`, `path_traversal`, `concurrency`

**Limits:**

| Section | Default | Max |
|---------|---------|-----|
| risk markers | 80 | 200 |
| excerpt radius | 80 lines | 200 lines |
| security diagnostics | 80 | 80 |
| security symbols | 80 | 80 |
| references | 80 | 80 |

**Input parameters:**

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `file_path` | string | required | Target file |
| `line` | number | optional | 1-indexed line; both line and column required together |
| `column` | number | optional | 1-indexed column |
| `radius` | number | 80 | Excerpt radius (max 200) |
| `content` | string | optional | Proposed file content for overlay (mutually exclusive with patch) |
| `patch` | string | optional | Single-file unified diff for overlay (mutually exclusive with content) |
| `security_categories` | string[] | all | Filter risk marker categories |
| `max_risk_markers` | number | 80 | Max risk markers (max 200) |
| `include_call_hierarchy` | bool | true when position | Include call hierarchy when line+column provided |
| `security_preset` | string | none | Optional preset: rust_server, rust_cli, web_backend, dependency_review, unsafe_review |

**Risk marker categories:** `auth`, `crypto`, `filesystem`, `network`, `process`, `unsafe`, `serialization`, `sql`, `secrets`, `path_traversal`, `concurrency`

**Output shape:**

- `risk_markers` — deterministic pattern-matched markers with category, label, line, column, matched_text, rationale
- `security_relevant_symbols` — symbols filtered for security relevance (keyword matching + proximity to risk markers)
- `security_relevant_diagnostics` — diagnostics filtered for severity (error/warning) and proximity to risk markers
- `definitions` / `references` — when line+column provided
- `call_hierarchy` — when include_call_hierarchy=true and line+column provided
- `overlay` — when content or patch provided
- `notes` — human-readable context notes including unavailable section errors
- `limits` — truncation flags per section (precise: flags reflect filtered counts, not raw counts)

**Read-only contract:** `securityContext` never writes files. Patch-based overlay is applied in memory only and restored after diagnostics collection.

`securityContext` reuses the same freshness metadata and capability gating used by the semantic-context path. When diagnostics are stale or unavailable, the packet keeps that evidence visible in notes and metadata instead of turning the gap into a clean bill of health.

**Error visibility:** Nonfatal LSP subrequest failures (diagnostics, document symbols, definitions, references) are surfaced in the `notes` array rather than failing the whole packet. This allows partial results when individual LSP operations fail.

**Implementation:** Risk marker scanning, pattern tables, and security-relevant filtering helpers live in `src/tool/lsp_security.rs`. The scanner collects all markers then caps, ensuring precise truncation flags. Diagnostics and symbols are filtered for security relevance before capping, so relevant items after many irrelevant ones are not dropped.

### Security call expansion

`securityContext` can optionally include a bounded call expansion with `call_depth`. This is separate from the shared compact call hierarchy collected by `SemanticContextCollector`: the shared hierarchy provides only immediate incoming/outgoing relationships, while call expansion performs its own recursive BFS expansion handler-locally via `build_call_expansion_summary`. The default is `0`, which disables recursive expansion. Supported depths are `1` and `2`; higher depths are rejected with a clear error. Expansion is breadth-first, dedupes repeated nodes, preserves edges to already-seen nodes, and is capped by `max_call_nodes` (default 32, max 64) and internal edge/range limits (`MAX_CALL_EDGES = 128`, `MAX_HIERARCHY_RANGES = 32`). When caps are reached, expansion prefers returning a partial graph with `truncated=true` rather than failing the entire packet. `call_expansion.truncated` is true when nodes, edges, or per-edge ranges are dropped due to configured or internal caps.

This is not whole-program analysis. It is a shallow LSP-backed neighborhood around the target symbol for review triage.

**Input fields:**

| Field | Type | Default | Max | Description |
|-------|------|---------|-----|-------------|
| `call_depth` | number | 0 (off) | 2 | Call expansion depth. Requires `line`+`column`. |
| `max_call_nodes` | number | 32 | 64 | Maximum nodes in expansion graph. |
| `call_direction` | string | "both" | — | `"incoming"` (callers), `"outgoing"` (callees), or `"both"`. |

**Validation rules:**

- `call_depth > 2` → `ToolError::Execution` (rejected, not clamped)
- `call_depth > 0` without `line`+`column` → `ToolError::Execution`
- `max_call_nodes > 64` → clamped to 64
- Invalid `call_direction` → `ToolError::Execution`

**Read-only boundary:** Call expansion only sends LSP hierarchy requests (`prepareCallHierarchy`, `callHierarchy/incomingCalls`, `callHierarchy/outgoingCalls`). It never writes files or executes code.

**Error handling:** Expansion errors are nonfatal and collected in `call_expansion.errors`. A failure in one child request does not abort the entire expansion. The packet still returns risk markers, diagnostics, and other context even if expansion fails.

**Presets:** No preset enables call expansion by default. All presets keep `call_depth = 0`. Expansion is only activated through explicit `call_depth > 0`.

### SecurityContextPacket fields

| Field | Type | Description |
|-------|------|-------------|
| `file` | string | File path |
| `target` | object/null | Target position (line, column) |
| `excerpt` | object | Source excerpt |
| `risk_markers` | array | Security risk markers |
| `security_relevant_symbols` | array | Security-relevant symbols |
| `security_relevant_diagnostics` | array | Security-relevant diagnostics |
| `definitions` | array | Go-to-definition results |
| `references` | array | Find-references results |
| `call_hierarchy` | object/null | Shallow call hierarchy summary |
| `call_expansion` | object/null | Bounded recursive call expansion (when `call_depth > 0`) |
| `overlay` | object/null | Overlay diagnostics (when content/patch provided) |
| `preset` | string/null | Applied security preset name |
| `notes` | array | Informational notes |
| `limits` | object | Truncation flags |

### Security context presets

`securityContext` supports optional presets through `security_preset`. Presets tune default risk categories, excerpt radius, marker count, and call-hierarchy inclusion. Explicit input fields override preset defaults.

| Preset | Use case | Categories | Radius | Max markers | Call hierarchy |
|--------|----------|------------|--------|-------------|----------------|
| `rust_server` | Rust services/APIs/daemons | auth, network, serialization, filesystem, process, secrets, sql, path_traversal, crypto, unsafe, concurrency | 120 | 120 | true when positioned |
| `rust_cli` | CLI/local automation | process, filesystem, secrets, path_traversal, serialization, crypto, unsafe, concurrency | 100 | 100 | true when positioned |
| `web_backend` | Web handlers/auth/database | auth, network, serialization, sql, secrets, filesystem, path_traversal, crypto | 120 | 120 | true when positioned |
| `dependency_review` | manifests/build/dependency-sensitive files | secrets, filesystem, process, network, serialization, crypto | 80 | 80 | false by default |
| `unsafe_review` | unsafe/FFI/concurrency review | unsafe, concurrency, filesystem, process | 160 | 120 | true when positioned |

Preset defaults are retrieval defaults, not vulnerability policies. They do not change the read-only contract or add external scanners. Explicit user inputs (`security_categories`, `radius`, `max_risk_markers`, `include_call_hierarchy`) always override preset defaults.

### Security review workflow

The security agent uses `securityContext` as evidence-gathering input for defensive code review. It follows this loop:

1. **Target discovery** — Uses `egggit` diff APIs to identify changed files and hunks. Binary/deleted files are skipped. Generated/vendor paths (`target/`, `node_modules/`, etc.) are excluded. Async discovery reuses `build_security_review_targets` and `build_file_level_security_review_target` for consistent positioned targets (`column: Some(1)`).
2. **Preset selection** — Each file is classified into a `securityContext` preset (`rust_server`, `rust_cli`, `web_backend`, `dependency_review`, `unsafe_review`) based on path heuristics and optional content hints.
3. **Preflight checks** — Filename-hint scans (`secret_filename_hint_scan`, `unsafe_filename_hint_scan`) run on target file names (not contents).
4. **Context gathering** — `securityContext` is requested around changed hunks with bounded settings. Call expansion is opt-in (depth 0 by default, escalated to 1 only for high-risk targets via `choose_security_context_escalation`).
5. **Prompt synthesis** — Risk markers always become *review prompts*, never findings. Planned target prompts use `source: changed_hunk` evidence; risk-marker prompts use `source: securityContext.risk_marker` evidence.
6. **Evidence-based synthesis** — `synthesize_evidence_based_findings()` groups evidence by file/line bucket, applies the eligibility gate (2+ dimensions required), and emits findings for eligible groups. Marker-only evidence never creates findings. Findings are heuristic defensive review outputs, not proof of exploitability.
7. **Output** — Review prompts, findings, and parsed hunk refs (`SecurityReviewOutput.hunks`) are returned. The hunk refs carry line-level diff detail for TUI display. The `/security-review` command and `run_security_review_workflow` orchestrator produce all three.

Key types live in `src/security/workflow/` (split into submodules: `mod.rs`, `types.rs`, `diff.rs`, `preflight.rs`, `evidence.rs`, `context.rs`, `report.rs`, `enrichment.rs`). The workflow is read-only and never mutates files.

#### Orchestrator

`run_security_review_workflow(root, base, options)` is an async entry point that runs the full pipeline (discover targets → build prompts → preflight checks → evidence-based synthesis → assemble output). It does NOT execute `securityContext` LSP requests — those are deferred to a subsequent phase. Content preflight uses `root.join(p)` for repo-root-relative reads, so it works correctly when launched from any working directory. `SecurityReviewWorkflowOptions` controls which stages run and caps output counts.

#### LSP enrichment (optional)

`run_security_review_workflow_with_lsp_enrichment(root, base, options, executor)` extends the deterministic stage-1 review with an optional LSP enrichment pass. When `enable_lsp_enrichment` is true, it:

1. Runs deterministic stage-1 review.
2. Calls `run_security_context_enrichment()` which filters escalation plans to non-None levels, caps requests at `max_lsp_requests`, and executes each via a `SecurityContextExecutor` trait with per-request timeout (`lsp_request_timeout_ms`).
3. Converts responses to enriched prompts via `prompts_from_security_context()` and structured evidence via `evidence_from_security_context()` (extracting risk markers, diagnostics, call graph summaries, truncation notices).
4. Reruns synthesis via `synthesize_evidence_based_findings_with_extra_evidence()` with enriched CallPath/Diagnostic/TruncationNotice evidence injected into eligible findings.

Failures, timeouts, and truncation are recorded as notes — they never fail the whole review. Dedicated note helpers handle enrichment status: `note_lsp_enrichment_unavailable` (enrichment requested but no executor available), `note_lsp_enrichment_no_eligible_targets` (no targets met escalation policy), and `note_lsp_enrichment_executed` (reports executed request count). The `SecurityContextExecutor` trait enables mockable testing via `NoopSecurityContextExecutor` (always errors) and `FixtureSecurityContextExecutor` (pre-configured responses). A real adapter `LspSecurityContextExecutor` (in `src/security/lsp_executor.rs`) wraps `LspTool` to delegate `securityContext` operations. It validates requests via `validate_security_context_request()`, injects the operation field, and parses the JSON string response. The `SecurityContextExecutorProvider` trait and `run_security_review_command_with_executor()` enable executor injection at the command level; `run_security_review_command()` delegates to the executor-aware runner with `None`. In local mode the TUI creates a shared `LspTool` at startup (`App.lsp_tool`) and passes a `LspSecurityContextExecutor` to the command handler for `--enrich`. In socket/remote mode `lsp_tool` is `None` and `--enrich` falls back to deterministic stage-1 with an unavailable note.

The TUI dispatches `/security-review` asynchronously so the render thread is never blocked. The handler spawns a tokio task and publishes a `TuiCommand::SecurityReviewRun { id, root, args, lsp_tool }` variant (carrying a `SecurityReviewRunId` newtype and a cloned `Arc<LspTool>`) which is consumed in the `cmd_rx` arm of `run_event_loop` in `src/tui/mod.rs` by a new `async fn handle_security_review_run(...)`. That handler invokes the new `pub async fn run_security_review_background(root: PathBuf, args: SecurityReviewCommandArgs, lsp_tool: Option<Arc<LspTool>>) -> Result<SecurityReviewReceipt, String>` in `src/security/workflow/report.rs`, which owns its inputs (no borrowed `&self` across the await) and constructs the `LspSecurityContextExecutor` internally when `lsp_tool` is `Some`. In remote/socket mode `lsp_tool` is `None` and the call falls back to deterministic stage-1 with `note_lsp_enrichment_unavailable`. A reentrancy guard, `App.security_review_running: Option<SecurityReviewTaskState>` (holding `{ id, abort_handle }`, defined in `src/security/workflow/receipt.rs:301`), is set on dispatch and cleared in both success and failure paths; a second `/security-review` issued while the guard is set is rejected with a warning toast ("Security review already running. Wait for it to finish or cancel it."). On success the full report is pushed to the message timeline as a `UIMessage` with `MessageRole::Assistant` and a `[Security Review]` label, plus a brief success toast; the structured `SecurityReviewReceipt` is stored on `App.latest_security_review` via `App::set_latest_security_review` (`src/tui/app/mod.rs:914`) for later reopening. On failure an error toast is shown. The local-mode `LspSecurityContextExecutor` and the remote/socket deterministic fallback are both preserved.

The completion handler in `src/tui/mod.rs:2205` (`handle_security_review_finished`) guards against stale completions by comparing the incoming `id` against `app.security_review_running.id` via `App::security_review_run_id`; mismatches are silently dropped. `/security-review-cancel` aborts the running task via `App::cancel_security_review` (`src/tui/app/mod.rs:936`) which calls `AbortHandle::abort()` and clears the guard; cancellation is best-effort — if the spawned task is in a non-cancellable section (e.g. inside a blocking syscall), its completion may still arrive and is dropped by the id-mismatch guard. `/security-show` reopens `Dialog::SecurityReview` (a master/detail panel at `src/tui/components/dialogs/security_review.rs` with keybindings `j/k`, `PgUp/PgDn`, `f` cycle filter (including `HunkBacked` to show only items with hunk context), `n` notes, `p` prompts, `h` jump to hunk section, `H` copy hunk text to clipboard, `]`/`[` next/previous hunk-backed item, `Enter` opens a read-only source preview dialog for the finding's file (root-scoped via `resolve_security_review_item_path` in `receipt.rs`; shows "Security Review Finding/Prompt" origin label; falls back to clipboard if the file cannot be opened)), `Esc/q` close) from the in-memory receipt without rerunning the review. When a finding or prompt has a matching hunk (derived from the reviewed diff, not live files), the detail section renders hunk context with added/removed/context line styling. If no receipt exists yet, `/security-review-show` surfaces a "No security review result available yet." warning toast. Receipt persistence is in-memory only; the `--panel` flag on `/security-review` auto-opens the result panel on completion.

The `/security-review --enrich` command flag opts into enrichment. The `--panel` flag auto-opens the result panel on completion. Without these flags, behavior is unchanged (deterministic, no LSP execution; report goes to timeline only).

The legacy entry point `plan_security_review_from_diff(diff, repo_root)` remains available. It parses changed hunks via `parse_changed_hunks`, applies path exclusions (`is_security_review_excluded_path` — excludes `vendor/`, `third_party/`, `target/`, `dist/`, `build/`, `node_modules/`, `*.min.js`; notably does NOT exclude `Cargo.toml`, `Cargo.lock`, `build.rs`), selects presets via `select_security_preset`, builds `securityContext` request payloads via `build_security_context_request`, converts risk markers to review prompts via `prompts_from_security_context`, and assembles reports with an explicit "not confirmed findings" note via `assemble_security_review_report`.

#### Escalation policy

`choose_security_context_escalation(target, finding, prompt)` maps risk signals to `SecurityContextEscalationLevel` (None, Basic, CallDepth1, CallDepth2). `build_escalated_security_context_request(target, level)` builds the `securityContext` payload with the chosen depth. `plan_security_context_escalations(targets, ...)` returns a `SecurityContextEscalationPlan` DTO — a policy output that recommends escalation levels per target without executing LSP requests. The plan is a recommendation, not an execution. Escalation is read-only, bounded (max depth 2), and never writes files.

### Hunk/source navigation

`hunkSourceContext` is a read-only context-gathering operation that provides hunk-aware evidence for code review, edit planning, and navigation. It consumes a unified diff (patch) and maps changed hunks to enclosing symbols, nearby diagnostics, definitions, references, and hierarchy data.

**Input parameters:**

| Parameter | Type | Default | Notes |
|-----------|------|---------|-------|
| `file_path` | string | required | Target file |
| `patch` | string | optional | Unified diff text (mutually exclusive with hunks) |
| `include_definitions` | bool | true | Include definitions intersecting hunks |
| `include_references` | bool | true | Include references intersecting hunks |
| `include_call_hierarchy` | bool | false | Include call hierarchy for enclosing symbols |
| `include_type_hierarchy` | bool | false | Include type hierarchy for enclosing symbols |
| `radius` | number | 40 | Excerpt radius for source context |
| `max_hunks` | number | 20 | Maximum hunks to process |

**Output shape:**

- `file_path` — target file path
- `hunks` — per-hunk evidence (enclosing symbol, related symbols, diagnostics, definitions, references, call/type hierarchy, source excerpt, diagnostic freshness). When multiple hunks are present, semantic context is collected centered on the first hunk; definitions, references, and hierarchy are shared across all hunks.
- `limits` — truncation flags (hunks_truncated, symbols_truncated, diagnostics_truncated, references_truncated, excerpt_truncated)
- `notes` — informational notes
- `truncated` — whether output was capped

**Note:** The response does NOT include the full `SemanticContextResponse`. Hunk evidence is derived from a single semantic collection centered on the first hunk; definitions, references, and hierarchy from that collection are distributed to all hunks via range matching.

**Key properties:**

- Read-only: never writes files; patch is parsed in memory
- Pure navigator: `HunkSourceNavigator` consumes `SemanticContextResponse` and does not call LSP directly
- Bounded: per-hunk caps on symbols, diagnostics, references; global cap on hunk count
- Diagnostic freshness is preserved per hunk from the semantic response
- Evidence is best-effort and bounded; not proof of correctness or security

**Implementation:** Diff parsing (`parse_unified_diff`) produces `HunkDescriptor` values. Range primitives (`hunk_nav_ranges`) handle overlap, containment, and symbol/diagnostic matching. `HunkSourceNavigator` assembles per-hunk evidence. `HunkSourceNavigationCollector` coordinates parsing + semantic collection.

#### Hunk evidence routing policy

`HunkSourceContextPolicy` (in `src/lsp/hunk_nav_policy.rs`) controls when `hunkSourceContext` is invoked. The policy is conservative by default: definitions and references are on, hierarchy is off, and multi-file / oversized patches are skipped.

| Field | Default | Description |
|-------|---------|-------------|
| `enabled` | `true` | Master switch |
| `max_patch_bytes` | 64 KB | Maximum patch size before skipping |
| `max_hunks` | 20 | Maximum hunk count per file before skipping |
| `include_definitions` | `true` | Include definitions intersecting hunks |
| `include_references` | `true` | Include references intersecting hunks |
| `include_call_hierarchy` | `false` | Include call hierarchy |
| `include_type_hierarchy` | `false` | Include type hierarchy |

`decide_hunk_source_context(policy, patch, file_path)` returns `HunkSourceContextDecision::Use` or `Skip { reason }`. Skip reasons include: disabled policy, no file path, unsupported file extension (checked against `LSP_LIKELY_EXTENSIONS` — ~25 extensions including rs, py, ts, js, go, java, c, cpp, rb, swift, kt), no extension, oversized patch, zero hunks, or too many hunks. The decision is explicit and testable; skip reasons are logged, never silently swallowed.

#### Compact summary formatter

`format_hunk_source_context_summary(response)` (in `src/lsp/hunk_nav_prompt.rs`) formats a `HunkSourceNavigationResponse` into a compact, bounded text summary suitable for agent-facing review/edit-planning prompts. The summary format is deterministic but the underlying evidence is best-effort and server-dependent. Output includes: file path, diagnostic freshness metadata, per-hunk focus range, enclosing symbol, related symbols (capped at 5), diagnostics in hunk (capped at 5 messages), nearby diagnostics count, definitions count, references count, call/type hierarchy summaries, truncation flags, and per-hunk notes. Does not dump raw JSON.

#### Security review workflow integration

`SecurityReviewWorkflowOptions.enable_hunk_source_context` (default `false`) opts into best-effort `hunkSourceContext` execution during `run_security_review_workflow`. When enabled and an executor is available:

1. `collect_hunk_source_context_all_files()` groups `ChangedHunk`s by file path, processes files in deterministic sorted order, and invokes the `HunkSourceContextPolicy` per file using actual per-file patch data. It returns a `HunkSourceContextCollectionResult` with evidence, summaries, notes, and `HunkSourceContextExecutionStats` (tracking files_considered, files_policy_skipped, requests_attempted/succeeded/failed/timed_out, evidence_items_emitted). Policy evaluation (Option B) happens before request-cap check, keeping skip statistics complete. `files_considered` counts files whose policy was evaluated (within file cap, before any request-cap break). `evidence_items_emitted` is assigned post-loop from `all_evidence.len()` (not incrementally accumulated). Request caps count actual executor calls, not loop position. The `HunkSourceContextExecutor` trait (`src/security/workflow/context.rs`) defines the boundary; `LspHunkSourceContextExecutor` (`src/security/lsp_executor.rs`) is the real adapter that calls `LspTool::execute_hunk_source_context_typed()` directly with a typed `HunkSourceNavigationRequest` — no JSON round-trip. The adapter uses an internal `TypedHunkSourceContextTarget` trait (production: `LspTool`) with a `#[cfg(test)]` recording target for forwarding verification without a live LSP server. The model-facing tool schema remains patch-only; internal pre-parsed hunk descriptors are used via the typed API.
2. `evidence_from_hunk_source_context()` converts `HunkSourceNavigationResponse` into `StructuredSecurityEvidence` items with kind `HunkNavigation` (enclosing symbols, definitions, reference counts) or `Diagnostic` (in-hunk and nearby diagnostics). Only real `HunkSourceNavigationResponse` produces `HunkNavigation` evidence — policy skip decisions are routing metadata, never security evidence.
3. Evidence is injected into `synthesize_evidence_based_findings_with_extra_evidence()` for eligibility gating. The tightened gate requires `HunkNavigation` to appear alongside `RiskMarker` or `Preflight` (or other supporting dimensions) — `ChangedHunk + HunkNavigation` alone is not finding-eligible.

Multi-file diffs are processed one file at a time (capped at 8 files). The workflow is the `/security-review --hunk-context` flag path, not model-initiated.

Fail-open: per-file errors are noted (appended to output `notes`) and never block the workflow.

#### HunkNavigation evidence kind

`SecurityEvidenceKind::HunkNavigation` (in `src/security/workflow/types.rs`) represents evidence from `hunkSourceContext`: enclosing symbols, definitions intersecting changed ranges, and reference counts. Each item carries `file_path`, `line`, `summary`, and `detail` (hunk id). `HunkNavigation` is not standalone finding-eligible — it requires `RiskMarker`, `Preflight`, or another supporting dimension to form a finding. Policy skip decisions never produce `HunkNavigation` evidence.

#### ChangedHunk → HunkDescriptor conversion

`ChangedHunk::to_hunk_descriptor(hunk_index)` (in `src/security/workflow/types.rs`) converts a security-workflow `ChangedHunk` into an egglsp `HunkDescriptor` for the typed internal execution path. The `old_range` and `new_range` are computed from the hunk's start/count fields. The `hunk_index` parameter provides the deterministic hunk id prefix. These pre-parsed descriptors are passed directly to `LspTool::execute_hunk_source_context_typed()` via the `HunkSourceContextExecutor` trait — the model-facing tool schema remains patch-only.

#### Fail-open behavior

All hunk source context operations are fail-open: errors during policy evaluation, semantic collection, or evidence conversion are recorded as notes in the output and do not prevent the rest of the security review from completing. LSP results remain server-dependent and fail-open. Policy skip reasons are logged at debug level.

#### Default caps

- `max_patch_bytes`: 64 KB (patch size limit; policy uses actual per-file patch data)
- `max_hunks`: 20 (per-file hunk count limit)

#### Known limitations

- **Single-file hunk context only**: `hunkSourceContext` processes one file's hunks at a time. The security review workflow groups multi-file patches by file path and processes them independently in deterministic sorted order.
- **First-hunk-centered semantic collection**: Semantic context (definitions, references, hierarchy) is collected centered on the first hunk's position. Results are distributed to all hunks via range matching. Hunks far from the first may have less precise context.
- **LSP results are server-dependent**: LSP results remain server-dependent and fail-open. Policy skips and LSP errors produce notes, never block the caller.

### Position Convention

Model-facing line and column are **1-indexed**. The wrapper converts to LSP 0-indexed via `to_lsp_position()`. Missing required fields return clear `ToolError::Execution` messages.

### Compact DTOs

All output is wrapped in `LspToolOutput<T>` with `operation`, `file_path`, `result_count`, `truncated`, and `results` fields. Individual results use `LocationSummary`, `DiagnosticSummary`, `SymbolSummary`, or `HoverSummary` with 1-indexed positions and file paths (not URIs). Additionally, `SemanticContextPacket` wraps a bounded source excerpt (`SourceExcerpt` with `start_line`, `end_line`, `text`), diagnostics, symbols, definitions, references, optional per-section error fields (`definitions_error`, `references_error`), optional `source_actions` array of `SemanticSourceActionHint` objects (`action`, `available`, `preview`, `error`), and a `SemanticContextLimits` struct tracking truncation (including `overlay_diagnostics_truncated`).

### Diagnostics

The `diagnostics` operation is first-class. It reads from the shared diagnostics cache populated by `publishDiagnostics` notifications. Diagnostics use 1-indexed line/column in output. If no diagnostics have arrived yet, an empty list is returned.

The `diagnostics` tool output includes freshness metadata (`freshness`, `source`, `age_ms`, `usable_evidence`) so callers can judge diagnostic reliability. `age_ms` is the age in milliseconds since diagnostics were received from the language server. Freshness is classified as `Fresh`, `PossiblyStale`, `Stale`, or `Unavailable`. See the Diagnostics Cache Lifecycle section below for details.

### Capability-Gated Operations

The `semanticContext` and `securityContext` handlers check `LspCapabilitySnapshot` before making optional expensive LSP calls (definitions, references, call hierarchy, type hierarchy). When a capability is unsupported:

- **definitions**: `definitions_error` is set to `"definition not supported by server"` and no LSP request is made.
- **references**: `references_error` is set to `"references not supported by server"` and no LSP request is made.
- **call hierarchy** (semanticContext): the `call_hierarchy` field is `None` (no request made).
- **call hierarchy** (securityContext): a note `"call hierarchy not supported by server"` is appended.
- **call expansion** (securityContext): a note `"call expansion not supported by server (call hierarchy required)"` is appended and `call_expansion` is `None`.
- **type hierarchy** (semanticContext): the `type_hierarchy` field is `None` (no request made).

When no capability snapshot is available (e.g., server not yet initialized), operations default to attempting the call (fail-open). This ensures degraded-but-functional behavior when capabilities cannot be determined.

### Capability Discovery and Normalization

`LspCapabilitySnapshot` provides a normalized boolean view of a server's capabilities after initialization. Each boolean field corresponds to a specific LSP feature or operation, derived from the `ServerCapabilities` reported by the server during the `initialize` handshake.

```rust
pub struct LspCapabilitySnapshot {
    pub publish_diagnostics: bool,
    pub document_symbols: bool,
    pub workspace_symbols: bool,
    pub goto_definition: bool,
    pub find_references: bool,
    pub hover: bool,
    pub completion: bool,
    pub call_hierarchy: bool,
    pub type_hierarchy: bool,
    pub semantic_tokens: bool,
    pub code_actions: bool,
    pub formatting: bool,
    pub rename: bool,
    pub signature_help: bool,
}
```

`LspSemanticOperation` enumerates the semantic operations available through the tool interface:

```rust
pub enum LspSemanticOperation {
    Diagnostics,
    DocumentSymbols,
    WorkspaceSymbols,
    Definition,
    References,
    Hover,
    Completion,
    CallHierarchy,
    TypeHierarchy,
    SemanticTokens,
    SecurityContext,
}
```

`LspUnavailable` is a structured fallback response returned when an operation is not supported by the server:

```rust
pub struct LspUnavailable {
    pub operation: LspSemanticOperation,
    pub reason: String,
    pub server_id: String,
}
```

The `capabilities` LspTool operation returns the snapshot for the server associated with a given file path. Capability detection uses actual initialized server capabilities where available; if the server has not yet initialized, the snapshot reflects the server definition's known defaults. The snapshot carries real `server_name` and `language_id` metadata from the initialized server, not placeholders. `SecurityContext` is always treated as available — it is a composite operation that relies on multiple underlying LSP requests and risk marker scanning, not a single capability.

### Diagnostics Cache Lifecycle

`DiagnosticCacheEntry` (in `crates/egglsp/src/client.rs`) stores per-file diagnostics with `received_at`, `source`, `content_version`, `server_generation`, and `post_restart` metadata. The cache is updated asynchronously when `publishDiagnostics` notifications arrive from the LSP server.

- `server_generation: u64` — the authoritative per-key generation at the time diagnostics were received. `0` is the "never assigned" sentinel for pre-Phase-3 entries and unit tests. After a server restart, the restart coordinator re-keys retained diagnostics to `current - 1` via `LspService::mark_diagnostics_stale_for_key` so the freshness classifier returns `LspDiagnosticFreshness::Stale` until the new server emits its first push.
- `post_restart: bool` — `true` when the entry was produced by a server that has been restarted at least once since the start of the client key. Monotonically sticky (once set, it stays set across subsequent restarts).

`LspClient::diagnostic_snapshot()` classifies freshness based on these fields:

`age_ms` is zero for unavailable snapshots and elapsed diagnostic age for all cached diagnostic snapshots, including stale cached snapshots.

`LspDiagnosticSnapshot` represents a point-in-time view of diagnostics for a single file:

```rust
pub struct LspDiagnosticSnapshot {
    pub file_path: PathBuf,
    pub diagnostics: Vec<FileDiagnostic>,
    pub age_ms: i64,
    pub source: LspDiagnosticSource,
    pub freshness: LspDiagnosticFreshness,
    pub server_generation: Option<u64>,
    pub post_restart: bool,
}
```

`LspDiagnosticFreshness` indicates how current the cached diagnostics are:

```rust
pub enum LspDiagnosticFreshness {
    Fresh,
    PossiblyStale,
    Stale,
    Unavailable,
}
```

`LspDiagnosticSource` tracks how diagnostics were obtained:

```rust
pub enum LspDiagnosticSource {
    Pushed,
    Pulled,
    Unknown,
}
```

**Invalidation rules:**

- Diagnostics transition to `PossiblyStale` on file content changes (the server has not yet republished after the change).
- Diagnostics transition to `Stale` on server restart (the cache is cleared and repopulated asynchronously).
- `Unavailable` indicates no diagnostics have been received for the file.

`PossiblyStale` and `Stale` diagnostics should not be treated as high-confidence evidence for code analysis or security findings. The freshness field allows consumers to make informed decisions about diagnostic reliability.

`DiagnosticsCollector::get_diagnostic_snapshot_for_file()` is the primary API for obtaining a snapshot. It ensures the file is open from disk, then delegates to `LspService::get_diagnostic_snapshot_for_key()` which consults the client's diagnostic cache.

`DiagnosticsCollector::get_all_diagnostic_snapshots()` returns a `HashMap<String, LspDiagnosticSnapshot>` for freshness-aware bulk diagnostics. `get_all_diagnostics()` is a legacy freshness-blind view that returns raw diagnostics without freshness metadata.

`LspDiagnosticSnapshot::diagnostics_may_still_be_warming()` is a derived method that returns `true` when freshness is `PossiblyStale` and diagnostics are empty, indicating the server may still be processing.

### Diagnostic Evidence in Context Packets

Both `SemanticContextPacket` and `SecurityContextPacket` include an optional `diagnostic_evidence` field carrying freshness metadata:

```rust
struct DiagnosticEvidenceMeta {
    freshness: LspDiagnosticFreshness,
    source: LspDiagnosticSource,
    age_ms: i64,
    usable_evidence: bool,
}
```

The `age_ms` field is the age in milliseconds since diagnostics were received from the language server, not an absolute generation timestamp. The `usable_evidence` field is `true` when freshness is `Fresh` or `PossiblyStale`. Consumers should treat stale/unavailable diagnostic evidence as low-confidence. The `securityContext` handler appends notes when diagnostics are stale or unavailable:

- `"diagnostics stale: treating diagnostics as low-confidence evidence"` (for `Stale`)
- `"diagnostics unavailable: no LSP diagnostic evidence available"` (for `Unavailable`)

This allows the security review workflow to make informed decisions about diagnostic reliability when synthesizing findings.

## Shared Semantic Context API

The shared semantic context API provides domain-agnostic DTOs for assembling LSP evidence. `SemanticContextResponse` is the **internal semantic read model** for `semanticContext`; `securityContext` reuses the shared diagnostic evidence and capability snapshot but assembles its own security-filtered packet.

### SemanticContextRequest

Describes what the caller wants to know:

```rust
pub struct SemanticContextRequest {
    pub file_path: String,
    pub line: Option<u32>,          // 1-indexed
    pub column: Option<u32>,        // 1-indexed
    pub intent: SemanticContextIntent,
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_diagnostics: usize,
    pub call_depth: u8,
    pub include_overlay: bool,
    pub include_source_actions: bool,
    pub include_definitions: bool,
    pub include_references: bool,
    pub excerpt_radius: u32,
}
```

### SemanticContextResponse

The assembled semantic context. This is the internal read model that `semanticContext` and `securityContext` adapt from:

```rust
pub struct SemanticContextResponse {
    pub file_path: String,
    pub symbol: Option<SemanticSymbolSummary>,        // First symbol (backward-compatible)
    pub all_symbols: Vec<SemanticSymbolSummary>,      // All document symbols (flattened, capped)
    pub diagnostics: Vec<FileDiagnostic>,             // 0-indexed diagnostics
    pub definitions: Vec<SemanticLocation>,
    pub references: Vec<SemanticLocation>,
    pub call_hierarchy: Option<SemanticCallGraphSummary>,
    pub type_hierarchy: Option<SemanticTypeGraphSummary>,
    pub source_excerpt: Option<SemanticSourceExcerpt>,
    pub diagnostic_evidence: Option<SemanticDiagnosticEvidence>,
    pub overlay: Option<SemanticOverlay>,
    pub source_actions: Vec<SemanticSourceActionHint>,
    pub section_truncations: Vec<SemanticSectionTruncation>,
    pub limits: SemanticContextLimits,
    pub notes: Vec<String>,
    pub truncated: bool,
    pub unavailable: Vec<LspUnavailable>,
}
```

### Supporting DTOs

- `SemanticSourceExcerpt`: Source text excerpt with `start_line`, `end_line`, `text`, `truncated`
- `SemanticDiagnosticEvidence`: Freshness metadata (`freshness`, `source`, `age_ms`, `usable_evidence`)
- `SemanticOverlay`: Overlay diagnostics/symbols from proposed content preview
- `SemanticSectionTruncation`: Per-section truncation metadata (`section`, `original_count`, `emitted_count`, `limit`)
- `SemanticContextLimits`: Truncation flags per section
- `SemanticSymbolSummary`, `SemanticLocation`: Compact symbol/location summaries (1-indexed)
- `SemanticCallGraphSummary`, `SemanticTypeGraphSummary`: Hierarchy summaries

All line/column values in shared DTOs are **1-indexed** for consistency with the presentation layer.

### Conversion

The `semanticContext` handler follows this flow:

```rust
let request = SemanticContextRequest::from_tool_input(...)?;
let response = collector.collect(request).await?;
let packet = SemanticContextPacket::from_semantic_response(response, target, overlay, source_actions, limits);
serialize(packet)
```

`SemanticContextPacket::from_semantic_response()` is the adapter that converts the shared response into the tool-local presentation packet, handling 0→1-indexed diagnostic conversion, excerpt adaptation, and note→error field mapping.

### Remote/Core Ownership Model (Phase 7)

In the headless-core architecture:

- The **headless core** owns all LSP server processes, capability snapshots, diagnostics caches, and file synchronization state. LSP servers are spawned and managed exclusively by the core.
- **Frontends** (TUI, web, IDE extensions) request semantic context over the core protocol (`CoreRequest::SemanticContext` or equivalent). They never start their own LSP server processes for the same workspace unless explicitly configured as local-only.
- All requests pass through **root authorization** — the core enforces that requested file paths fall within an allowed root directory before dispatching to LSP.
- A remote frontend that connects to a headless core with no LSP support for the requested language receives a structured `LspUnavailable` response rather than an opaque error. The response includes the server ID and a human-readable reason.
- When the core has no LSP server for the file's language (e.g., unsupported language, no server configured), the `SemanticContextResponse.unavailable` field contains one or more `LspUnavailable` entries. The frontend can render these as informational notes.
- Diagnostics cache ownership remains with the core. Frontends receive `LspDiagnosticSnapshot` with freshness metadata and can display staleness indicators.

### Backend config (MCP fallback semantics)

The `lsp` tool's registration is decided by `[tool_backends.lsp]` in
the loaded `Config`. The matrix is applied by `ToolRegistry::with_options`
and mirrored exactly by `ToolRegistry::backend_report(...)`:

| `[tool_backends.lsp]` setting | Registered tool | `backend_report` status |
|-------------------------------|-----------------|-------------------------|
| `backend = "native"` (default) or `"builtin"` | real `LspTool` wrapper around `egglsp::LspService` | `ready` |
| `backend = "mcp", fallback_to_native = true` (default for `mcp`) | real `LspTool` wrapper (the live path is the native crate, not an MCP server) | `fallback-native` |
| `backend = "mcp", fallback_to_native = false` | hidden `DisabledTool` stub — model never sees `lsp` | `unavailable` (`ConfiguredButUnavailable`) regardless of MCP server connectivity |
| `backend = "disabled"` | hidden `DisabledTool` stub — model never sees `lsp` | `disabled` |

The `DisabledTool` stub is registered (callable for diagnostics) but
filters itself out of the model-facing catalog via
`Tool::expose_in_definitions() == false`. Production session
construction uses `ToolRegistry::with_session_config_defaults(&config,
...)` so the resolved config is preserved; the legacy
`with_session_defaults(...)` is documented as a footgun for
config-aware paths.

## Protocol Peer Hardening (Phase 1)

Codegg's LSP runtime operates as a **bidirectional JSON-RPC peer**, not merely a client that sends requests and consumes diagnostics. The server can send requests back to the client (e.g. `workspace/configuration`, `workspace/workspaceFolders`, `client/registerCapability`), and Codegg answers them correctly.

### Incoming Message Taxonomy

The `classify_json_rpc_message` function classifies incoming JSON-RPC messages using strict structural analysis:

| Shape | Classification |
|-------|---------------|
| `id` + `method` | Server request |
| `id` + valid error object (with numeric `code` and string `message`) | Error response |
| `id` + `result` field present | Success response |
| `method` without `id` | Notification |
| Otherwise (including id-only objects, malformed errors) | Unknown |

The classifier is strict: an `id` without `method`, without a valid error object, and without a `result` field is classified as `Unknown`, not as a response. Malformed error objects (e.g., missing `code`, non-numeric `code`, missing `message`) also fall through to `Unknown`.

`JsonRpcId` preserves both numeric (`Number(i64)`) and string (`String(String)`) IDs per JSON-RPC spec. Client-originated IDs are tracked in the `pending` map; server-originated IDs are answered but never inserted into `pending`.

### Supported Server-Originated Requests

Codegg handles these server requests via `dispatch_server_request` in `server_request.rs`:

| Method | Behavior |
|--------|----------|
| `workspace/configuration` | Returns configuration values scoped to the server/root; `null` for unknown sections |
| `workspace/workspaceFolders` | Returns the current root as a single-element workspace folder array |
| `client/registerCapability` | Records registration in `DynamicRegistrationState` (bounded at 256); acknowledges with `null` |
| `client/unregisterCapability` | Removes registration by ID; tolerates unknown IDs |
| `window/workDoneProgress/create` | Acknowledges with `null` |
| `workspace/applyEdit` | **Always rejected** as an application-level result with `applied: false` and a `failureReason` string — not a JSON-RPC error. Codegg does not permit implicit language-server edits. |
| Unknown methods | Returns JSON-RPC error `-32601` (Method not found) |
| Malformed params | Returns `-32602` (Invalid params) |

### Dynamic Registration

`DynamicRegistrationState` tracks server-requested capability registrations bounded at 256 entries. Recording a registration does **not** mean Codegg claims operational support for that feature — `LspCapabilitySnapshot` is derived from `ServerCapabilities` (the `initialize` response) only.

`client/registerCapability` processes the full `registrations` array via `register_batch()`, which pre-checks capacity before any mutation: all entries are validated first (rejecting the entire request if any entry is malformed), deduplicated by ID (last-write-wins within a single request), then applied. This atomic batch approach prevents partial application when a batch exceeds the capacity limit. Replacements of existing IDs bypass the 256 cap; only new IDs are counted against it.

`client/unregisterCapability` accepts either the `unregisterations` array (LSP spec), the `unregistrations` compat spelling, or a single `id` field for backward compatibility. Unknown IDs are silently tolerated.

### Shared Serialized Writer

`LspWriter` (`writer.rs`) provides a shared, `Arc<Mutex<...>>`-wrapped writer for all protocol output. Both client requests/notifications and the background server-request dispatcher use the same writer, ensuring serialized writes without interleaving frames. Content-Length framing uses UTF-8 byte length.

### Timeout Cancellation

On request timeout:
1. The pending entry is removed from the map
2. A best-effort `$/cancelRequest` notification is sent to the server with the original request ID
3. If the cancel write fails, `fail_transport()` marks the transport failed and drains any remaining pending requests
4. The timeout error is returned to the caller

Cancellation failures do not replace the timeout error, but they do retire the transport so later calls fail fast.

### Single-Flight Client Initialization

`LspService::get_or_create_client` uses explicit `InitRole` election: the first caller becomes `Leader` and spawns an owned initialization task (`run_initialization_attempt`); concurrent callers for the same `{project_root}:{server_id}` key become `Waiters`. The `InitSlot` stores one leader sender plus a waiter list, and completion is fanned out to every sender with the same `Arc<LspClient>` on success or the same `SharedInitError` on failure. An `ATTEMPT_COUNTER: AtomicU64` generates monotonic attempt IDs stored in the `InitSlot`.

#### Start-Registration Barrier

The wrapper task does not begin its initialization body until its `active_init_tasks` entry has been installed. This is enforced by a one-shot start barrier:

1. The leader creates `(start_tx, start_rx)` and `(completion_tx, completion_rx)` channels.
2. The wrapper task is spawned with `start_rx` and `completion_tx` and **awaits** `start_rx` first.
3. The leader installs the `InitTaskControl` (containing `completion_rx`) into `active_init_tasks` under its own lock acquisition.
4. The leader re-validates the slot under the `initializing` lock — these are sequential lock acquisitions, not nested.
5. The leader sends on `start_tx`, releasing the wrapper to begin its body.
6. If the slot was invalidated in step 4, the leader drops `start_tx` (causing the wrapper's `start_rx.await` to resolve to `Err`), aborts the wrapper defensively, removes the just-installed `active_init_tasks` entry, and notifies any waiters via `abort_and_finalize_unstarted_task`.

This eliminates the spawn-before-registration race: a fast task cannot complete before its bookkeeping record exists.

#### Authoritative Completion Primitive

Each spawned initialization task is wrapped in `run_init_task_wrapper`, which:

1. **Awaits** `start_rx` to receive the registration-completion signal.
2. **Owns** the `completion_tx` end of the authoritative terminal signal.
3. **Executes** the inner init attempt, with `AssertUnwindSafe + catch_unwind` to convert panics into a `SharedInitError` for any waiters and an `InitTaskExit::Panicked` exit value.
4. **Explicitly removes** its `active_init_tasks` entry before sending the terminal exit (primary cleanup path).
5. **Disarms** the `ActiveTaskGuard` fallback so the guard's `Drop` is a no-op.
6. **Sends** exactly one `InitTaskExit` (`Completed`, `Panicked(msg)`, or `Cancelled`) via `completion_tx`.

The completion receiver in `InitTaskControl` is the authoritative source of truth for "the wrapper task has terminated". The receiver resolves to `Ok(exit)` on the normal path, or to `Err(RecvError)` if the sender (and therefore the wrapper) was dropped without sending — e.g. by forced abort. Shutdown awaits this receiver through `await_init_task_completions`; it never holds the real `JoinHandle` via a forwarding task.

On initialization failure, the slot is cleaned up by attempt ID (compare-and-remove prevents stale cleanup from deleting newer slots), and all waiting callers receive `SharedInitError` (preserving error category and message), allowing retries. Before a successful client is published, the init task rechecks `LifecycleState` and only inserts when the phase is still `Running` and the generation matches the captured generation; if publication is invalidated or loses to an existing client, the unpublished client is disposed via `dispose_unpublished_client(...)` with a bounded shutdown timeout. This differs from `OnceCell` which would cache the failure permanently. `SharedInitError` with `SharedInitErrorKind` enum (`ServerNotFound`, `DownloadFailed`, `LaunchFailed`, `InitializeFailed`, `Timeout`, `Cancelled`, `Protocol`, `Other`) is used for all oneshot channel results instead of raw `LspError`, making concurrent error propagation thread-safe and cloneable. The `#[cfg(test)]` `test_new()` constructor accepts injectable test factories for deterministic testing without live LSP servers.

#### Active-Task Entry Cleanup

`active_init_tasks` entries are removed through three complementary mechanisms:

1. **Explicit removal** (primary path): the wrapper acquires the `active_init_tasks` lock and removes its own entry before sending the terminal exit. This is the path for normal completion and ordinary failure. The wrapper then calls `ActiveTaskGuard::disarm()` to suppress the fallback.

2. **ActiveTaskGuard fallback**: if the wrapper is dropped before explicit removal (e.g. due to forced abort, panic propagation that bypasses explicit cleanup, or unexpected future drop), the guard's `Drop` runs and **spawns a follow-up cleanup task** that locks the map and removes the entry. This is robust to lock contention at drop time. The guard no longer relies on `try_lock` for the fallback path — that approach silently abandoned cleanup if the lock was held.

3. **Coordinator-owned drain**: `shutdown_all` is the additional safety net. After awaiting all completion receivers (via `await_init_task_completions`), the drain clears the map one final time to guarantee the postcondition regardless of which path any individual wrapper took.

This eliminates the prior defect where successful, failed, or invalidated attempts could leave stale task-control entries until shutdown drained the map.

#### Registration Lock Ordering

Between slot creation and active-task registration, the slot may be removed by a concurrent shutdown. The `Leader` branch resolves this race without nested locks:

1. Acquire `initializing` lock; check slot validity for this `attempt_id`; release `initializing` lock.
2. Acquire `active_init_tasks` lock; install `InitTaskControl`; release `active_init_tasks` lock.
3. Acquire `initializing` lock again; re-check slot validity; release `initializing` lock.
4. If still valid, send on `start_tx` to release the wrapper.
5. If invalidated at any point, run `abort_and_finalize_unstarted_task` to drop the start signal, abort the wrapper defensively, remove the active-task entry, and notify any waiters.

No path holds `active_init_tasks` while awaiting `initializing`, and no path holds either lock across task/client I/O. The two lock acquisitions are sequential, not nested.

#### Cooperative Cancellation in Test Factories

The injected test factory is wrapped in a `tokio::select!` so cancellation propagates to test factories by default:

```rust
tokio::select! {
    biased;
    res = init_fn(server, &root) => res,
    _ = cancellation.cancelled() => Err(LspError::InitializationCancelled("shutting down".into())),
}
```

The standard `blocking_factory` and similar are cancellation-aware. Tests that exercise forced abort (e.g. via a stuck factory) use factories whose inner future ignores the outer `select!`'s cancellation arm, exercising the `AbortHandle` path through `await_init_task_completions`.

### Global Map Lock Discipline

Non-mutating service methods use `clients.read().await` to avoid serializing unrelated clients behind process I/O. These methods include: `open_file`, `update_file`, `close_file`, `save_file`, `is_file_open`, `get_diagnostics_for_key`, `get_all_diagnostics_for_key`, `diagnostics_may_still_be_warming`, `get_diagnostic_snapshot_for_key`, `send_request`, `client_keys`, and `get_capabilities_for_key`. Each follows the pattern:

1. Acquire the map read lock
2. Clone the `Arc<LspClient>`
3. Release the map lock
4. Await the client operation

Write guards (`clients.write().await`) are reserved for slot election/publication (inserting a new client entry after initialization) and shutdown drain (removing clients during `shutdown_all`). This separation ensures read-heavy workloads (diagnostics, file operations, capability checks) never contend with write operations.

`close_file` and `save_file` use deterministic O(1) ownership lookup via the `document_owners` map (URI → client key) rather than searching cloned handles or relying on `HashMap` iteration order.

### Shutdown Coordination

`LspService` tracks a `LifecycleState` containing both `ServiceLifecycle` phase and a monotonic `generation: u64`. The service also holds a `tokio::sync::watch` channel (`lifecycle_tx`) that retains the latest lifecycle state for late subscribers; this replaces the previous `Notify`-based coordination which was susceptible to lost wakeups at the `ShuttingDown → Stopped` transition. `shutdown_all()` atomically transitions to `ShuttingDown` and increments the generation, broadcasting the change on the watch channel. The spawned initialization task rechecks the phase and generation before publication, preventing stale results from being published after shutdown and disposing any unpublished client that loses the race. `get_or_create_client()` rejects new client acquisition when the lifecycle is not `Running`, returning `LspError::InitializationCancelled`.

#### Quiescent Shutdown Sequence

`shutdown_all()` follows a bounded, multi-phase sequence driven by an **absolute deadline** (computed once at entry: `Instant::now() + SHUTDOWN_GLOBAL_TIMEOUT`). Each stage receives a remaining-time bound; the deadline propagates rather than being re-wrapped in a timeout that can silently abandon finalization.

1. **Transition to ShuttingDown** — atomically sets phase and increments generation; broadcasts on `lifecycle_tx` (watch channel). A second caller observing `ShuttingDown` enters the race-free `await_stopped()` path.
2. **Clear document ownership** — `document_owners` is cleared.
3. **Drain init slots** — all pending `InitSlot` entries are removed; their senders are notified at step 9.
4. **Drain active tasks** — `active_init_tasks` is drained; each entry's `InitTaskControl` (containing its `CancellationToken`, `AbortHandle`, and authoritative completion receiver) is moved into the shutdown's local vector.
5. **Concurrent cooperative cancel** — all cancellation tokens are signalled simultaneously.
6. **Aggregate grace wait** — `await_init_task_completions` awaits all completion receivers concurrently using `FuturesUnordered` under one aggregate grace deadline (`SHUTDOWN_CANCELLATION_GRACE` = 300ms, capped by the global deadline). The future for each control uses `tokio::select!` to race the receiver against the deadline. On timeout, the control (with its real receiver intact) is returned in the pending set. On receiver resolution, the exit value is logged. **No forwarding task wraps the real `JoinHandle`**: the receiver is the authoritative terminal signal.
7. **Concurrent abort of stragglers** — for any controls still pending after the grace, `AbortHandle::abort()` is called on each, then `await_init_task_completions` re-awaits the same set of completion receivers under the remaining global deadline. The receiver resolves either when the wrapper sends its terminal exit (rare under forced abort) or when the sender is dropped (the task future was dropped by the abort, closing the channel). Every aborted task's real completion is observed.
8. **Concurrent ready-client shutdown** — ready clients are drained from the map and shut down concurrently (`futures::future::join_all`). Each per-client timeout is capped by `SHUTDOWN_CLIENT_TIMEOUT` (2s) and the global deadline, so the total shutdown duration is independent of client count. Three result variants are logged: `Ok(Ok(()))` (graceful), `Ok(Err(_))` (graceful shutdown error), and `Err(_)` (timeout).
9. **Notify init-task waiters** — the senders drained in step 3 receive a `Cancelled` `SharedInitError`.
10. **Forced finalization** — if the absolute deadline has expired, a `warn!` is logged. The `active_init_tasks`, `initializing`, and `document_owners` maps are drained defensively to guarantee postconditions. This is the documented **pathological deadline fallback**: the service state is finalized after abort was requested, with unresolved task completion logged as a severe invariant failure. The shutdown contract distinguishes the **normal contract** (all task termination observed via completion receivers) from the **deadline fallback** (state forced after the global deadline, with the explicit caveat that Tokio may not deliver a terminal event for an aborted task in pathological cases).
11. **Transition to Stopped** — final lifecycle phase; broadcast on `lifecycle_tx` so concurrent waiters can return.

Total bounded duration: `SHUTDOWN_GLOBAL_TIMEOUT` (6s). Per-stage budgets are derived from the absolute deadline.

#### Concurrent Shutdown Callers

A second caller observing `ShuttingDown` enters `await_stopped()`:

1. Subscribe to the `lifecycle_tx` watch channel.
2. Re-check the current state.
3. If `Stopped`, return immediately.
4. If `ShuttingDown`, await state changes until `Stopped`.

This race-free pattern eliminates the lost-wakeup window that the previous `Notify`-based coordination had at the `ShuttingDown → Stopped` transition. Late subscribers always observe the latest retained state.

### New Tests

The tracked initialization and quiescent shutdown features are covered by targeted tests:

| Test | What it verifies |
|------|-----------------|
| `shutdown_cancels_blocked_factory` | Cooperative cancellation: a factory blocked in `initialize` is cancelled via `CancellationToken` during shutdown |
| `shutdown_aborts_uncooperative_task` | Hard abort: a task that ignores cooperative cancellation is aborted via `AbortHandle` after grace period. The `FutureExitProbe` RAII guard asserts the factory future body was actually dropped before shutdown returned. |
| `concurrent_shutdown_callers` | Two concurrent `shutdown_all()` calls both observe the final `Stopped` state via the watch channel |
| `concurrent_shutdown_lost_wakeup_boundary` | Late subscribers to the watch channel do not miss the `ShuttingDown → Stopped` transition |
| `read_lock_concurrency` | Non-mutating operations (`open_file`, `diagnostics`, etc.) use read locks and do not contend with each other |
| `publication_race_remains_safe` | Publication under shutdown races: an init task that finishes after `ShuttingDown` does not publish a stale client |
| `normal_completion_removes_active_task_entry` | Explicit cleanup path: the wrapper removes its `active_init_tasks` entry without requiring shutdown |
| `ordinary_failure_removes_active_task_entry` | Same, for ordinary initialization failures |
| `forced_abort_is_awaited` | The aborted task's completion receiver is awaited; the task body actually exits before shutdown returns. The `FutureExitProbe` proves the factory future was dropped. |
| `global_deadline_finalizes_state` | A task that does not complete within the global deadline is still drained; lifecycle reaches `Stopped` and all maps are empty |
| `fast_completion_cannot_beat_registration` | The start-registration barrier prevents a fast-completing task from racing past the `active_init_tasks` insertion. Run repeatedly in a bounded loop to expose scheduler races. |
| `cooperative_cancellation_is_observed` | The factory future body is dropped (RAII probe increments) before shutdown returns; the `InitTaskExit` resolution is observed via the authoritative receiver. |
| `many_tasks_share_one_grace_period` | The aggregate grace wait in `await_init_task_completions` is applied across all in-flight tasks; total shutdown time is bounded by one grace period rather than N × grace. |
| `no_stale_active_entries_under_contention` | Concurrent fast success attempts (single-flight) leave `active_init_tasks` empty without requiring shutdown. |
| `lock_order_no_deadlock_under_overlap` | Concurrent registration and shutdown overlap via test gates; neither path deadlocks and both complete within bounded time. |
| `global_deadline_fallback_asserts_all_signals` | A stuck factory is forcibly aborted, the abort signal is observed, all maps are drained, and the lifecycle is `Stopped` — all within the global deadline. |
| `forced_abort_after_grace_period` | Genuinely survives cooperative cancellation past the 300ms grace interval using a test-only `InitTaskBehavior::IgnoreCancellationUntilAbort` hook. Asserts the real `AbortHandle::abort()` path is reached and the factory future is dropped before shutdown returns. |
| `aggregate_grace_across_independent_tasks` | Multiple independent initialization keys (distinct roots) each with blocked factories. Confirms `active_init_tasks.len() == N` and total shutdown time is bounded near one aggregate grace period rather than N × grace. |
| `deadline_fallback_with_unresolvable_completion` | Constructs `InitTaskControl` values with receivers whose senders are intentionally retained (never resolving). Drives `await_init_task_completions` to the global deadline and verifies unresolved controls are logged/returned and state finalization continues. |
| Phase 2: initialization handshake | `production_protocol_stdio::initialization_handshake` | Real stdio init/initialized/shutdown/exit through fake server |
| Phase 2: server request during init | `production_protocol_stdio::server_requests_during_init_and_dynamic_registration` | workspace/configuration interleaved with initialize |
| Phase 2: apply-edit refusal | `production_protocol_stdio::apply_edit_refusal_keeps_client_usable` | workspace/applyEdit rejected with applied:false |
| Phase 2: concurrent responses | `production_protocol_stdio::concurrent_out_of_order_responses_and_notifications` | Multiple requests, out-of-order responses |
| Phase 2: timeout and cancellation | `production_protocol_stdio::request_timeout_and_late_response_are_dropped` | Production $/cancelRequest emission |
| Phase 2: malformed frames | `production_protocol_stdio::malformed_frames_fail_transport` | 8 malformed framing cases → transport failure |
| Phase 2: server exit | `production_protocol_stdio::server_exit_before_response_and_error_response` | Server exits without responding |
| Phase 2: typed semantic | `production_semantic_stdio::typed_semantic_requests_collect_context_and_freshness` | Hover, definition, references, symbols, completion, code actions |

The `FutureExitProbe` test-only RAII guard (`src/lsp/../service.rs`) is constructed at the top of test factory futures to prove that the future body was actually dropped. It is robust to all three exit paths (normal return, cooperative cancellation, forced abort) and is used by `shutdown_aborts_uncooperative_task`, `cooperative_cancellation_is_observed`, `forced_abort_is_awaited`, and `forced_abort_after_grace_period`.

The flaky transport test (`timeout_cancel_failure_marks_transport_failed_and_writes_writer_closed`) has been fixed by replacing OS-pipe-dependent behavior with deterministic writer injection.

### Writer Failure Propagation

The background reader tracks `ClientTransportState` (`Running` or `Failed { reason }`). All terminal transport failures (stdout EOF, server-request result/error write failure, `send_request` write failure, `send_notification` write failure, and timeout-cancel write failure) transition to `Failed` exactly once via the centralized `fail_transport()` helper. The helper atomically transitions to `Failed` (idempotent), releases the transport lock, then drains all pending requests with errors. Subsequent `send_request` / `send_notification` calls return `LspError::WriterClosed` immediately, avoiding writes to a broken pipe.

### Integral Error Code Validation

`is_structural_error()` in `client.rs` validates JSON-RPC error codes as integers using `as_i64().is_some()`, rejecting fractional codes (e.g. `3.5`) that would fail JSON-RPC error semantics. This prevents misclassification of malformed error responses.

### Limitations

- `workspace/applyEdit` is always rejected as an application-level result (`applied: false`) — servers cannot implicitly write files through Codegg
- Dynamic registrations are tracked but do not expand model-facing capability claims
- Configuration responses are bounded to the server's configured section — no environment secrets are exposed
- Server requests are handled synchronously within the background reader with a 5-second timeout. A timeout produces a JSON-RPC error response with code `-32603` (Internal error) rather than silently abandoning the request. Current handlers are fast and local.

## Error Handling

Overlay-specific behavior: `semanticCheckPreview` restore failures are logged and surfaced via `restore_error: Option<String>` in the response (alongside `restored_disk_view: false`) rather than returning `LspError`. `diagnostics_error` and `symbols_error` are similarly non-None when their respective LSP requests fail, rather than silently returning empty vectors. The original disk content is never written by this operation, so a restore failure leaves the LSP in-memory state stale but the filesystem untouched. The wrapper's `execute_structured` sets `success=false` when `restore_error` is present.

```rust
pub enum LspError {
    ServerNotFound(String),
    DownloadFailed(String),
    LaunchFailed(String),
    NotInitialized(String),
    RequestFailed(String),
    RequestTimeout(String),
    UnsupportedLanguage(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedEdit(String),
    PathOutsideRoot(String),
    Utf16Position(String),
    OverlappingEdits,
    UnsupportedSourceAction(String),
    CommandOnlySourceAction(String),
    NoEditForSourceAction(String),
    AmbiguousSourceAction(String, String),
    Protocol(String),
    WriterClosed(String),
    InitializationCancelled(String),
    ServerRestarted { server_id: String, old_generation: u64, new_generation: u64 },
    ServerUnavailable(String),
    ServerDegraded(String),
}
```

**New Phase 3 error variants:**

- `ServerRestarted` — returned when a request targets a server that has been restarted since the request was queued. Carries the server ID and generation numbers so callers can decide whether to retry.
- `ServerUnavailable` — returned when a server is in a non-operational state (e.g. `Failed`, `Restarting`, `Stopped`) and cannot serve requests.
- `ServerDegraded` — returned when a server is in the `Degraded` state and some operations may not work correctly.

### SharedInitError

A cloneable error type used for concurrent initialization waiters. `SharedInitError` with `SharedInitErrorKind` enum (`ServerNotFound`, `DownloadFailed`, `LaunchFailed`, `InitializeFailed`, `Timeout`, `Cancelled`, `Protocol`, `Other`) carries the error category and message across threads via oneshot channels. Converts via `From<&LspError> for SharedInitError` and `into_lsp_error()` back to `LspError`. This replaces raw `LspError` in the `InitSlot` oneshot results, making concurrent initialization error propagation thread-safe and cloneable.

`HierarchyDirection` parsing is available via `HierarchyDirection::parse(direction)` — accepts `"incoming"`, `"outgoing"`, `"both"`, or omitted (defaults to `"both"`). Invalid values return an error.

## Implementation Notes

- **PATH parsing**: Uses `std::env::split_paths()` for correct cross-platform PATH handling
- **PHP mapping**: Correctly maps to `php-language-server`
- **Request timeout**: 30-second timeout in `send_request()` with `LspError::RequestTimeout`
- **Hardcoded PATH**: Preserves user's actual PATH from environment
- **Stderr handling**: Background task drains stderr (capped at 64KB) to prevent blocking initialization
- **Notification handling**: Notifications received during request/response matching are parsed through `parse_publish_diagnostics` and update the shared diagnostics cache
- **Diagnostics parser**: `parse_publish_diagnostics` is a pure function that parses `publishDiagnostics` JSON-RPC notifications, testable without a real LSP server
- **Compact output**: Model-facing output uses DTOs (`LocationSummary`, `DiagnosticSummary`, etc.) with 1-indexed positions, not raw LSP JSON
- **Position conversion**: `to_lsp_position()` converts 1-indexed model input to 0-indexed LSP positions exactly once at the wrapper boundary
- **Client routing**: `workspaceSymbol` resolves client via `get_or_create_client_for_file` or `get_or_create_client_for_root_hint`, not arbitrary first-key selection
- **Doctor subsystem**: `codegg doctor --subsystem lsp` provides non-mutating LSP diagnostics

## Phase 2: Scripted Stdio Integration Testing (Complete)

The `egglsp` package carries production-harness integration tests under `tests/production_protocol_stdio.rs`, `tests/production_semantic_stdio.rs`, and `tests/production_service_stdio.rs`, plus `tests/scenario_engine.rs` containing the inlined fake-server self-tests (no longer `include!` from outside the package). The root crate carries composite tests in `tests/lsp_composite_stdio.rs` that bridge the gap between `egglsp`-only tests and the real root-crate collectors (`SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`). The fake LSP server binary is named `codegg-lsp-test-server` for root tests (via `CARGO_BIN_EXE_codegg-lsp-test-server`); the `egglsp` package uses `egglsp-test-server` via `CARGO_BIN_EXE_egglsp-test-server`. Both are built as `[[bin]]` targets from the `egglsp` package; they read Content-Length framed JSON-RPC from stdin, execute scripted scenarios, and write machine-readable transcripts. The fake server supports captured-ID mode for genuinely out-of-order concurrent responses, enabling deterministic testing of concurrent request handling. All integration tests use bounded condition waits (polling loops) instead of fixed sleeps. The scenario engine lives within the `egglsp` crate as the `test_support` module (feature-gated behind `lsp-test-support`, `#[doc(hidden)]`); both binary wrappers are thin `main()` functions that call `egglsp::test_support::run_or_exit()`. `base64` and `libc` are optional dependencies gated on `lsp-test-support`.

### Architecture

```
Integration test
    |
    | creates Scenario JSON file
    v
LspWriter / frame reader
    |
    | launches child process through spawn_server
    v
egglsp-test-server binary
    |
    | reads scenario, exchanges real framed messages
    v
transcript + assertions
```

### Scenario Format

Scenarios are JSON files with steps like `ExpectRequest`, `ExpectNotification`, `AllowRequest`, `AllowNotification`, `SendNotification`, `Delay`, and `ExitNow`. Steps can trigger actions like `RespondResult`, `RespondError`, `SendRequest`, `SendRawBytes`, or grouped-frame/raw-header helpers.

### Binary Discovery

Cargo exposes the test binary to the `egglsp` package integration tests via `CARGO_BIN_EXE_egglsp-test-server`. Root-crate composite tests use `CARGO_BIN_EXE_codegg-lsp-test-server`. The `EGGLSP_TEST_SERVER` env var can override the path for CI or manual runs.

### Test Counts

- **11 production protocol tests** in `tests/production_protocol_stdio.rs` — all passing ✅
- **3 production semantic tests** in `tests/production_semantic_stdio.rs` — all passing ✅
- **5 production service tests** in `tests/production_service_stdio.rs` — all passing ✅
- **26 root composite tests** in `tests/lsp_composite_stdio.rs` — all passing ✅
- **331 unit tests** in the `egglsp` crate (with `lsp-test-support` feature; 330 without); supervisor/restart scripted scenarios live in the integration tests below
- **3 scenario-engine tests** in `tests/scenario_engine.rs` — inlined fake-server self-tests for strict allow-listing, raw bytes, and grouped-frame fixtures

### Test Organization

- `tests/production_protocol_stdio.rs` — Production-harness protocol coverage for launcher-path behavior and transport edge cases
- `tests/production_semantic_stdio.rs` — Production-harness semantic and edit-preview coverage
- `tests/production_service_stdio.rs` — Production-harness LspService lifecycle coverage
- `tests/lsp_composite_stdio.rs` — 26 root-crate composite tests exercising `SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`, and security context tool orchestration against the fake server via the production `LspClient`/`LspService` stack; includes workspace-edit-preview safety tests (out-of-root, overlapping, command-only, no-edit, ambiguous, resource-operation), semantic-context collector workflow/capability-gating/failure-degradation tests, security context tool tests (orchestration with risk markers, call expansion, cycle suppression, node-limit truncation, depth-limit enforcement, diagnostic evidence filtering, and graceful degradation on call hierarchy error), and hunk-source-context collector test (unified diff with real LSP operations). Hunk path normalization uses canonical containment with error propagation.
- `tests/common/harness.rs` — Reusable fake-server test harness with temp directory and scenario management
- `tests/common/production_harness.rs` — Real-project harness for production launcher-path coverage
- `tests/scenario_engine.rs` — Inlined fake-server self-tests (strict allow-listing, raw bytes, grouped-frame fixtures)

### Test Coverage Matrix (Phase 2)

| Section | Plan ID | Tests | Status |
|---------|---------|-------|--------|
| Initialization handshake | C1 | `initialization_handshake` | ✅ |
| Server requests during init + dynamic registration | C2 | `server_requests_during_init_and_dynamic_registration` | ✅ |
| Apply-edit refusal | C3 | `apply_edit_refusal_keeps_client_usable` | ✅ |
| Interleaved notifications | C4 | `concurrent_out_of_order_responses_and_notifications` | ✅ |
| Concurrent out-of-order responses | C5 | `concurrent_out_of_order_responses_and_notifications` (captured-ID for genuine out-of-order) | ✅ |
| Diagnostics lifecycle | C6 | `diagnostics_lifecycle_tracks_file_changes` | ✅ |
| Cancellation write failure | C9 | Deterministic unit test in `client.rs` (OS-pipe flake avoided) | ✅ |
| Graceful shutdown | C10 | `server_exit_before_response_and_error_response` | ✅ |
| Ungraceful shutdown / EOF | C11 | `server_exit_before_response_and_error_response` | ✅ |
| Server error response | — | `error_response_is_reported` | ✅ |
| Malformed frames | — | `malformed_frames_fail_transport` (8 cases) | ✅ |
| Unknown frames | — | `unknown_json_rpc_frames_are_ignored` | ✅ |
| Grouped/split writes | — | `grouped_frames_and_split_writes_are_processed` | ✅ |
| Timeout and cancellation | C8 | `request_timeout_and_late_response_are_dropped` | ✅ |
| Document lifecycle | D1 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Hover | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Definition | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| References | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Document symbols | D2 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Call hierarchy | D3 | `hierarchy_context_requests_round_trip_through_real_client` (typed `LspClient` methods: `prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`) | ✅ |
| Type hierarchy | D3 | `hierarchy_context_requests_round_trip_through_real_client` (typed `LspClient` methods: `prepare_type_hierarchy`, `supertypes`, `subtypes`) | ✅ |
| Rename (WorkspaceEdit) | D4 | `edit_round_trips_do_not_mutate_disk` | ✅ |
| Code action (edit-bearing) | D4 | `typed_semantic_requests_collect_context_and_freshness` | ✅ |
| Rename preview (composite) | D5 | `rename_preview_converts_through_production_path` | ✅ | child-process |
| Format preview (composite) | D5 | `format_preview_converts_through_production_path` | ✅ | child-process |
| Source-action preview (composite) | D5 | `code_action_source_action_preview_converts_through_production_path` | ✅ | child-process |
| Preview safety: out-of-root | D5 | `preview_safety_out_of_root_rejected` | ✅ | child-process |
| Preview safety: overlapping | D5 | `preview_safety_overlapping_edits_rejected` | ✅ | child-process |
| Preview safety: command-only | D5 | `preview_safety_command_only_code_action_rejected` | ✅ | local |
| Preview safety: no-edit | D5 | `preview_safety_no_edit_code_action_rejected` | ✅ | local |
| Preview safety: ambiguous | D5 | `preview_safety_ambiguous_source_actions_rejected` | ✅ | local |
| Preview safety: resource operation | D5 | `preview_safety_resource_operation_rejected` | ✅ | local |
| Semantic context composite | D6 | `semantic_context_collector_exercises_real_workflow` | ✅ |
| Security context composite | D6 | `semantic_context_security_review_intent_collects_security_source` (renamed from `security_context_workflow_uses_semantic_collector`) | ✅ |
| Security context tool orchestration | D6 | `security_context_tool_exercises_risk_filtering_and_call_expansion` (exercises real `LspTool::execute("securityContext")` with risk markers, call expansion, cycle suppression) | ✅ |
| Security context: call hierarchy error degradation | D6 | `security_context_tool_degrades_on_call_hierarchy_error` (outgoingCalls error is recorded, packet returned, nodes/evidence preserved) | ✅ |
| Security context: node-limit truncation | D6 | `security_context_tool_enforces_call_node_limit_and_truncation` (max_call_nodes enforced, truncation flags set) | ✅ |
| Security context: depth-limit enforcement | D6 | `security_context_tool_enforces_call_depth_limit` (call_depth enforced independently of node budget, chain entry→level1→level2→level3 stops at depth 2) | ✅ |
| Security context: diagnostic evidence | D6 | `security_context_tool_filters_and_preserves_diagnostic_evidence` (security-relevant diagnostic survives filtering, diagnostic evidence metadata asserted: freshness, source, usability; style-only diagnostic filtered) | ✅ |
| Hunk source context composite | D7 | `hunk_source_context_collector_exercises_real_workflow` | ✅ |
| Semantic context: capability gating | D6 | `semantic_context_collector_capability_gating` | ✅ |
| Semantic context: failure degradation | D6 | `semantic_context_collector_failure_degradation` | ✅ |
| LspService single-flight | — | `single_flight_init_uses_a_real_child` | ✅ |
| LspService document lifecycle | — | `document_lifecycle_ownership_tracks_open_update_save_close` | ✅ |
| LspService diagnostics | — | `service_diagnostics_warming_then_populated` | ✅ |
| LspService delayed init shutdown | — | `shutdown_during_delayed_init_cancels_callers` | ✅ |
| LspService in-flight shutdown | — | `shutdown_with_inflight_request_is_bounded` | ✅ |

Phase 2 deliberately skips the following items (deferred to Phase 3 or omitted as nondeterministic at the OS-pipe level):
- **C7** (configuration / dynamic registration with real-server matrix) — deferred to Phase 3
- **C12** (malformed framing byte-level) — covered by `malformed_frames_fail_transport` + unit tests in `writer.rs`
- **C13** (malformed JSON-RPC shapes) — covered by `classify_json_rpc_message` unit tests in `client.rs`
- **C14** (server-response write failure end-to-end) — covered by deterministic writer unit test
- **C15** (stderr drainage) — drain is in `launch::spawn_stderr_drain`; bounded by line cap (not yet a Phase 2 test)

### Running

```bash
# Run Phase 2 integration tests (parallel-safe, require lsp-test-support feature)
cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio
cargo test -p egglsp --features lsp-test-support --test production_semantic_stdio
cargo test -p egglsp --features lsp-test-support --test production_service_stdio
cargo test -p egglsp --features lsp-test-support --test scenario_engine

# Run root composite tests (semantic/security/hunk collectors + preview safety)
cargo test --features lsp-test-support --test lsp_composite_stdio

# Run Phase 3 supervisor and restart tests (deterministic scripted, require lsp-test-support feature)
cargo test -p egglsp --features lsp-test-support --test supervisor_restart_stdio

# Force single-threaded to validate sequential stability
cargo test -p egglsp --features lsp-test-support --tests -- --test-threads=1
```

### Phase 3 Corrective Pass — Supervisor and Restart Tests

`crates/egglsp/tests/supervisor_restart_stdio.rs` carries 13 deterministic scripted tests (11 base + 2 Pass 9 race tests for manual supersession and back-to-back deadlock) that exercise the new `LspProcessRuntime`, `restart_client_coordinator`, per-client generation safety, and readiness policy transitions against the fake server. The tests use bounded condition waits (polling loops) instead of fixed sleeps.

| Test | Coverage |
|------|----------|
| `unexpected_exit_with_restart_disabled_becomes_failed` | Unexpected exit with `LspRestartMode::Disabled` transitions to `Failed`; no second process starts |
| `graceful_shutdown_completes_and_does_not_restart` | `LspClient::shutdown()` triggers `GracefulShutdownRequested` intent; exit is expected; no restart scheduled |
| `automatic_restart_after_unexpected_exit_succeeds` | Generation 1 exits; coordinator bumps to generation 2 after backoff; documents are replayed with preserved version; ownership is restored; `health` reports generation 2 and `Ready` |
| `restart_initialization_failure_then_recovery` | Generation 2 init fails; generation 3 initializes successfully; attempt counter and backoff are correct |
| `restart_exhaustion_leaves_failed_state` | Every restart fails; exactly `max_attempts` launches occur; final state is `Failed`; no additional process starts |
| `shutdown_cancels_scheduled_restart` | Crash schedules a delayed restart; `shutdown_all()` cancels the timer; no replacement process starts |
| `stale_exit_event_does_not_affect_newer_generation` | Generation 1 exit event is delayed; generation 2 is already ready; delayed event arrives and is silently dropped; pending requests survive |
| `replay_uses_latest_content` | Open version 1; update to version 2 dirty content; crash/restart; replay contains version 2 text; closed document is not replayed |
| `hung_process_is_force_killed_on_shutdown` | Server ignores `shutdown`/`exit`; shutdown deadline expires; process is killed and reaped; service reaches `Stopped` |
| `two_consecutive_restarts_use_monotonic_generations` | Generation 1 crash on `didOpen` → gen 2 hover crash → gen 3 recovers; generation map reaches 3; exactly 3 process starts; final state is `Ready` |
| `generation_is_identical_across_health_and_exit_event` | Health snapshot generation matches the published process-exit generation; a stale gen-1 exit event injected after gen-2 is `Ready` is silently dropped and does not change the health snapshot (Pass 11 test-timing fix writes the gen-3 scenario only after gen-2 is observed) |

### Real-Server CI

`.github/workflows/lsp-real-server.yml` runs one Tier 1 server per matrix job (`rust-analyzer` and `basedpyright`) against `crates/egglsp/tests/real_server_smoke.rs` with the `lsp-real-server-tests` feature. The workflow pins `rust-toolchain@1.81.0` for the rust-analyzer job and installs `basedpyright@1.13.1` for the basedpyright job. Each matrix job runs only its own server test (e.g. `-- rust_analyzer` or `-- basedpyright`); artifact filenames are sanitized via the matrix job name.

Phase 2 tests are parallel-safe (unique tempdir per test, per-process scenario/transcript paths). The harness does not require `--test-threads=1`; that flag was only needed by the pre-Phase-2 test layout.

### Phase 2 Final Closure Notes

- **Hermetic binary strategy**: Root-crate composite tests use `codegg-lsp-test-server` (via `CARGO_BIN_EXE_codegg-lsp-test-server`), while `egglsp`-only integration tests use `egglsp-test-server` (via `CARGO_BIN_EXE_egglsp-test-server`). Both are `[[bin]]` targets with thin `main()` wrappers calling `egglsp::test_support::run_or_exit()`. The scenario engine is the `test_support` module within `egglsp` (feature-gated behind `lsp-test-support`, `#[doc(hidden)]`). `base64` and `libc` are optional dependencies gated on `lsp-test-support`.
- **Hunk path normalization**: `normalize_diff_relative_path()` strips `a/`/`b/` diff prefixes and rejects path-traversal (`..`), `RootDir`, and `Prefix` components. `normalize_request_relative_path()` canonicalizes paths against the allowed root via `Path::canonicalize()` and rejects paths outside the root or resolving to the root itself. Errors are propagated from the collector's `collect()` method via `.map_err()`. The collector compares normalized hunk paths against the normalized target path to reject multi-file patches. Tests use real `TempDir` fixtures for canonical containment verification.
- **Inspection APIs**: `health_snapshot()` returns an `LspClientHealthSnapshot` with a typed `ClientTransportSnapshot` field (`Running` or `Failed { reason }`) and `pending_requests` count. `transport_state_snapshot()` and `pending_request_count()` are the individual observational health APIs for diagnostics. `dynamic_registration_snapshot()` is test-support/internal (`#[doc(hidden)]`).
- **Packaging**: `cargo package -p egglsp` succeeds with all target paths contained inside the package. The `egglsp-test-server` binary is a thin wrapper calling `egglsp::test_support::run_or_exit()`. The scenario engine is the `test_support` module within `egglsp` (feature-gated behind `lsp-test-support`, `#[doc(hidden)]`), shared by both binary wrappers.
- **Diagnostic evidence assertions**: The diagnostic evidence test now asserts structural metadata (freshness, source, usability) rather than just non-null presence. The test uses `service.open_file()` for initialization instead of consuming a `semanticContext` call, and waits for diagnostics via bounded polling.
- **Depth-limit enforcement**: A dedicated test (`security_context_tool_enforces_call_depth_limit`) proves call_depth limiting independently of node-budget truncation using a chain `entry→level1→level2→level3` with `call_depth=2, max_call_nodes=16`.
- **Hunk path tests**: Containment tests now use real temporary sibling files and are platform-neutral, replacing `/etc/passwd` references and nonexistent paths.

## Phase 3: Real-Server Compatibility & Resilience

> **Phase 3 supervision and restart lifecycle complete for Tier 1 servers; broader language/server compatibility remains future work.** See **Phase 3 Final Closure** above for the runtime termination, generation-safe supervision, restart budget, readiness, and fresh-evidence invariants. The sections below describe the Phase 3 structural scaffolding (compatibility profiles, health state machine, runtime owner, restart coordinator, document replay) that the final closure pass locked down.

Phase 3 builds on Phase 2's wire-protocol confidence by adding real-server compatibility testing, operational health tracking, process supervision, and document replay for crash recovery.

### New Modules (crates/egglsp/src/)

| Module | Purpose |
|--------|---------|
| `compatibility.rs` | Per-server compatibility profiles (`LspCompatibilityProfile`), readiness policies (`LspReadinessPolicy` — 4 variants: `InitializedIsReady` / `WaitForDiagnosticsOrTimeout` / `WaitForProgressEndOrTimeout` / `WarmupDelay`), restart policies (`LspRestartPolicy`, `LspRestartMode` — `Disabled` / `OnUnexpectedExit`), version detection (`LspServerVersion`), compatibility reports (`LspCompatibilityReport`, `CompatibilityCheckStatus`), check requirements (`LspCompatibilityCheck` with `CompatibilityRequirement` — `Required` / `RequiredIfAdvertised` / `Optional` / `KnownLimitation`), tier-1 profiles, and binary requirement checks |
| `health.rs` | Operational state machine (`LspOperationalState`: Starting → Initializing → Indexing → Ready → Degraded → RestartScheduled → Restarting → Failed → Stopping → Stopped), invalid transition detection (`InvalidTransition`), `context_note()` for semantic/security/hunk context propagation, and health snapshots (`LspOperationalHealthSnapshot` with `transport: Option<...>`, real `last_message_age_ms` / `last_diagnostics_age_ms`, `restart_attempts`, `last_error`, `stderr_tail`) |
| `runtime.rs` | Authoritative process runtime (`LspProcessRuntime` — single owner of the child, stderr ring buffer, intent receiver, kill channel) and explicit shutdown intent (`LspProcessIntent` — `Running` / `GracefulShutdownRequested` / `ForceKillRequested`) with `is_expected()` classifier |
| `restart.rs` | Per-client launch spec persistence (`LspClientDescriptor` with `from_profile` precedence), restart trigger enum (`RestartTrigger` — `Automatic` / `Manual`), `RestartShared` trait surface, and the single restart coordinator (`restart_client_coordinator<S, F>`) owning retry/backoff/exhaustion/cancellation |
| `supervisor.rs` | Process exit event tracking (`LspProcessExitEvent` — carries generation, status, signal, expected flag, stderr tail) and stderr ring buffering (`StderrRingBuffer`, 100 lines / 64KB cap) |
| `document_sync.rs` | Open document registry (`OpenDocumentRegistry`) and document snapshots (`OpenDocumentSnapshot` — preserves per-document version for replay after server restart) |

### New Error Variants

- `ServerRestarted { server_id, old_generation, new_generation }` — request targeted a server that has restarted; callers can retry against the new generation
- `ServerUnavailable(String)` — server in non-operational state (`Failed`, `Restarting`, `Stopped`)
- `ServerDegraded(String)` — server in `Degraded` state; some operations may not work

### New Feature Flag

```toml
[features]
lsp-real-server-tests = []  # separate from lsp-test-support
```

### Compatibility Status Model

| Status | Meaning |
|--------|---------|
| `Unknown` | Not yet checked or server not found |
| `Passing` | Server binary found, initializes, basic operations work |
| `PassingWithKnownLimits` | Server works but has documented limitations (e.g. no call hierarchy in pyright) |
| `Failing` | Server found but fails to initialize or produce valid responses |
| `Unsupported` | Server binary not found on PATH and no download available |

### Health State Model

```
Starting → Initializing → Indexing → Ready
                              ↓
Ready → Degraded → RestartScheduled → Restarting → Initializing
Starting/Initializing/Indexing/Ready → Failed → RestartScheduled
RestartScheduled → Restarting → Initializing
Ready → Stopping → Stopped
```

All transitions are validated by `health::transition()`; invalid transitions return `InvalidTransition`. All state mutations go through `LspService::transition_operational_state(key, next)`.

### Supervisor and Restart Policy

`LspRestartMode` controls whether a server is automatically restarted:

- `Disabled` (default) — no automatic restart; `Manual` triggers still run
- `OnUnexpectedExit` — restart on unexpected process exit

`LspRestartPolicy` extends the mode with `max_attempts`, `initial_backoff`, `max_backoff`, and `reset_after_healthy`. The coordinator applies the policy-driven backoff `min(initial_backoff * 2^(attempt-1), max_backoff)` between attempts and lazily resets `restart_attempts` after the client has been healthy for `reset_after_healthy`.

`LspProcessRuntime` is the single authoritative process owner (see `runtime.rs` above). `LspProcessExitEvent` records the exit code, signal, generation, expected flag, and stderr tail for crash analysis. The expected flag is derived from `LspProcessIntent` at exit time, not from transport state.

### Document Replay

When a server restarts, the restart coordinator replays all previously open documents via `textDocument/didOpen` notifications using the snapshot's preserved per-document version (not hard-coded 1), restores the `document_owners` map, updates the new client's `opened_files` state, and keeps registry entries intact. Closed documents are not replayed. Replay failure transitions the operational state to `Degraded` (not silent `Ready`). `OpenDocumentSnapshot` captures the URI, language ID, version, and full text of each open document.

### Real-Server Smoke Tests

```bash
# Run real-server smoke tests (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke

# Run with specific server binaries on PATH
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust-analyzer
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- pyright
```

The smoke tests (`crates/egglsp/tests/real_server_smoke.rs`) exercise rust-analyzer and pyright/basedpyright against the production launcher, frame parser, and request routing. They are slow (200ms-2s startup plus indexing), non-hermetic (require installed binaries), and expensive in CI, so they are gated behind the `lsp-real-server-tests` feature and run only on demand.

### Target Compatibility Matrix

| Server | Language | Key Operations | Known Limitations |
|--------|----------|----------------|-------------------|
| **rust-analyzer** | Rust | hover, definition, references, symbols, call hierarchy, rename, code actions, semanticContext, securityContext, hunkSourceContext | Initial indexing may be slow on large workspaces; diagnostics may need warm-up delay |
| **pyright** | Python | hover, definition, references, symbols, rename | No `prepareCallHierarchy` (Python doesn't have function-level call hierarchy); `codeAction` limited to organize imports |
| **typescript-language-server** | TypeScript / JavaScript | hover, definition, references, symbols, rename, code actions | `prepareCallHierarchy` may be empty; large workspaces slow |
| **gopls** | Go | hover, definition, references, symbols, rename, code actions | Call hierarchy not yet supported by gopls; securityContext will degrade gracefully |
| **clangd** | C / C++ | hover, definition, references, symbols, rename, code actions | No call hierarchy; slow on large TUs |

## Phase 3 Final Closure: Runtime Termination, Generation-Safe Supervision, Restart Budgets, Readiness, and Fresh Evidence

**Phase 3 supervision and restart lifecycle complete for Tier 1 servers; broader language/server compatibility remains future work.** Phase 3 final closure is the corrective pass that turned the structurally complete Phase 3 scaffolding into an operationally trustworthy lifecycle. The 10-pass sequence (Pass 1 through Pass 10) makes the runtime, restart, and freshness invariants explicit, the 11-pass addendum locks down the remove-before-signal handshake, and the 12-pass final cleanup makes the async release cancellation-safe. The 14 supervisor/restart scenarios pass repeatedly, the production test surface is green, and the 26 root composite tests pass.

### Generation-Aware Runtime Map

`runtime_map` stores `RuntimeEntry { generation: u64, runtime: LspProcessRuntime }` instead of bare runtimes, so insertion, lookup, and removal are all generation-aware:

```rust
#[derive(Clone)]
struct RuntimeEntry {
    generation: u64,
    runtime: LspProcessRuntime,
}

type RuntimeMap = Arc<Mutex<HashMap<String, RuntimeEntry>>>;
```

Three internal helpers enforce the invariant:

- `install_runtime(runtime_map, key, generation, runtime) -> Option<RuntimeEntry>` — replaces the prior entry only when the existing entry's generation is strictly older. Same- or newer-generation replacement is logged at warn and rejected (the old generation's runtime is responsible for removing itself).
- `runtime_for_generation(runtime_map, key, generation) -> Option<LspProcessRuntime>` — returns the runtime only when the stored generation matches.
- `remove_runtime_if_generation(runtime_map, key, generation) -> Option<RuntimeEntry>` — removes the entry only when the stored generation matches.

Monitor ordering uses the helpers throughout. After publishing the exit event, the monitor calls `remove_runtime_if_generation` (not bare `map.remove`) so a delayed old monitor cannot remove a newer generation's runtime. The unit tests `old_monitor_cannot_remove_new_runtime` and `runtime_removal_requires_exact_generation` lock this down.

### Runtime Termination Sequence

`LspClient::shutdown()` is split from the runtime-termination helper. The client method sends only `shutdown` / `exit` notifications (now exposed as `request_protocol_shutdown`); it never waits on the child once the runtime owns it. The service runs the bounded termination helper:

```rust
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

The required sequence (recorded for `LspProcessIntent` before any protocol write):

1. Look up the runtime only when the stored generation matches.
2. `runtime.request_graceful_shutdown()` — set `LspProcessIntent::GracefulShutdownRequested` BEFORE the protocol shutdown request.
3. Send the protocol shutdown under the graceful deadline.
4. `await runtime.wait_for_exit()` — under the graceful deadline.
5. On timeout, `runtime.request_force_kill()` and `await runtime.wait_for_exit()` under the absolute deadline.
6. Persist exit metadata if the exit receiver has not already done so.
7. Remove the runtime only if the stored generation still matches.
8. Return whether a force kill was required.

`RuntimeTerminationReason` distinguishes `ServiceShutdown`, `ManualRestart`, and `FailedPublication`. `RuntimeTerminationOutcome { runtime_present, exited, forced, event }` is recorded for diagnostics.

`shutdown_all()` snaps clients with their authoritative generations, then terminates all runtimes concurrently under one absolute deadline (the same 6s global bound). The lifecycle reaches `Stopped` only after the runtimes are reaped or forced.

### Single Generation Owner

`LspService::next_generation_for_key(key) -> u64` is the single source of truth for replacement generation. The reinit closure receives the generation as an argument:

```rust
FnMut(&LspClientDescriptor, u64) -> BoxFuture<'static, Result<Arc<LspClient>, LspError>>
```

The coordinator calls `next_generation_for_key` exactly once per restart, then invokes the reinit closure with the supplied generation. The closure may construct, initialize, bind, and install — but it must not compute generation independently. The publication order is:

```
construct and initialize replacement
bind supplied generation
install generation-aware runtime
publish client
set authoritative generation
replay documents
run readiness
publish Ready/Degraded
```

`restart_attempts` is incremented by `LspService::increment_restart_attempts(key)` BEFORE the coordinator runs and is shared across crash cycles.

### Manual Restart Termination

`LspService::manual_restart_client(key)` is a new public API. Manual restart always runs (it bypasses `LspRestartMode::Disabled`). The old runtime is terminated with `RuntimeTerminationReason::ManualRestart` BEFORE the replacement is started so a manual restart cannot leave two live processes. The old client is drained from the live map before the reinit closure runs to keep the reinit's `install` unambiguous.

A manual restart issued while an automatic restart is in progress supersedes it: the automatic restart's `reinit` is observed to be stale (the generation map has been advanced) and aborts with `LspError::ServerRestarted`. The manual restart proceeds and is the only live coordinator for `key`. Tests `manual_restart_terminates_old_process_before_new_start`, `manual_restart_supersedes_scheduled_automatic_restart`, and `manual_restart_leaves_one_runtime` lock the invariant down.

### Shared Crash-Cycle Restart Budget

The `restart_attempts` counter is shared across rapid crash cycles. Every actual replacement spawn consumes one attempt; a successful short-lived replacement does NOT reset the counter. The counter resets only after the client remains healthy for `reset_after_healthy`. The reset is evaluated lazily on the next unexpected exit:

```rust
if last_healthy_at.elapsed() >= reset_after_healthy {
    restart_attempts = 0;
}
```

`LspService::set_last_healthy_now(key)` is called when readiness reaches `Ready`; `LspService::reset_restart_attempts_if_healthy_inherent(key, reset_after_healthy)` returns `Some(prev)` when the lazy reset applies and `None` otherwise. When `restart_attempts >= max_attempts` no new process is launched and the operational state transitions to `Failed { reason: ... }`. Tests `rapid_crash_loop_exhausts_shared_budget`, `healthy_interval_resets_budget`, and `failed_initialization_and_post_ready_crash_share_budget` lock the budget invariant down.

### Retained Stale Diagnostics

Old diagnostics are transferred to the replacement client as explicit stale evidence. The new flow:

```rust
let retained = old_client.diagnostic_cache_snapshot().await;
new_client.install_retained_diagnostics("restart", retained).await;
```

`LspClient::install_retained_diagnostics(_source, entries)` (in `crates/egglsp/src/client.rs`) updates existing entries only when the incoming generation is newer, preserving the old `server_generation` and `post_restart` flags. The `DiagnosticCacheEntry.server_generation: u64` (0 sentinel for "never assigned") and `post_restart: bool` (monotonically sticky once observed) survive the transfer. The freshness classifier then returns `LspDiagnosticFreshness::Stale` because `entry.server_generation != current_generation`. A new `publishDiagnostics` from the new generation N (including an empty vector) overwrites retained generation N-1 evidence. `LspService::snapshot_diagnostics_for_restart(key)` returns the live cache snapshot for the old client (or an empty map when no client exists).

`post_restart` is now defined consistently as `generation > 1` everywhere — `LspClient::bind_server_generation(generation)` and `DiagnosticCacheEntry::with_generation(generation)` both compute it that way. Generation 1 is never `post_restart`; generation 2+ always is. Tests `retained_diagnostics_visible_as_stale_after_restart`, `new_generation_diagnostics_replace_retained_entries`, `empty_new_diagnostics_clear_old_errors`, `generation_one_is_not_post_restart`, and `generation_two_and_three_are_post_restart` lock the diagnostic-transfer semantics.

### Observed-Cycle Progress Readiness

`LspClient::wait_for_progress_end(timeout) -> bool` now requires `state.completed_cycle == true`. A zero timeout succeeds only if a completed cycle was already observed. The empty-token-set case (no progress notifications received yet) is NOT sufficient — `wait_for_progress_end` returns `false` until a `begin`/`end` cycle is observed. The `ProgressState` is per-client generation; a replacement client starts with a fresh tracker and no reset API is needed.

`LspService::wait_for_readiness(key, policy)` honors all four `LspReadinessPolicy` variants. The real-server harness calls `client.wait_for_progress_end(*timeout)` instead of fixed sleeps; the rust-analyzer and basedpyright suites now exercise the production primitive end-to-end. Tests `progress_wait_does_not_succeed_before_begin`, `progress_wait_succeeds_after_begin_end`, `progress_report_without_begin_does_not_complete_cycle`, and `restart_remains_indexing_until_generation_two_progress_ends` lock the observed-cycle semantics.

### Validated Restart Configuration and Descriptor Parity

`LspRestartPolicyConfig::try_to_domain(&self, base: &LspRestartPolicy) -> Result<LspRestartPolicy, LspError>` (in `crates/egglsp/src/config.rs`) validates user overrides and rejects:

- `mode = OnUnexpectedExit` AND `max_attempts == 0` — `LspError::InvalidConfig("restart mode OnUnexpectedExit requires max_attempts > 0")`.
- `initial_backoff_ms > max_backoff_ms` — `LspError::InvalidConfig(...)`.
- Any duration that overflows `Duration::MAX`.

Merge precedence is explicit user > profile > system default. `LspRestartPolicyConfig::merge_with_profile` copies non-`None` fields from the profile, so partial user overrides inherit unspecified profile values rather than resetting to generic defaults. `LspClientDescriptor::from_profile` produces one resolved descriptor per (root, server) pair, and both cold start and restart consume the same persisted descriptor — they receive identical `launch_spec`, `initialization_options`, `workspace_configuration`, `readiness_policy`, and `restart_policy`. The fake server captures `initialize.initializationOptions`, `workspace/configuration` responses, launch args, and environment; the test `cold_start_and_restart_receive_identical_configuration` asserts generation 1 and generation 2 match exactly.

### Real-Server Stderr Capture

`LspProcessRuntime::stderr_tail_capped(max_lines) -> Vec<String>` (in `crates/egglsp/src/runtime.rs`) returns the most recent `max_lines` lines from the bounded `StderrRingBuffer` (100 lines / 64KB cap) in chronological order. The real-server smoke harness (`crates/egglsp/tests/real_server_smoke.rs`) attaches an `LspProcessRuntime` to each smoke client, takes the child and stderr at construction, and on protocol shutdown calls `runtime.request_graceful_shutdown()` + `client.request_protocol_shutdown()` + `runtime.wait_for_exit()` with a force-kill fallback. At report construction the harness reads `runtime.stderr_tail_capped(20)` and populates `LspCompatibilityReport.stderr_tail`. Stage-timeout error messages now include the captured stderr tail as actionable detail.

For advertised references, the smoke harness now requires a non-empty result. A zero-length `findReferences` response is a `RequiredIfAdvertised` failure for the rust fixture, and the Python cross-file fixture continues requiring at least two distinct URIs. The test `references_assertion_fails_for_zero_results` locks this down.

### Supervised Constructor Invariant

`LspService::new(config)` is the bare constructor — it returns a `Self` without the cyclic back-reference wired, so the exit-receiver task is NOT auto-started. As of LSP Phase 3 final closure (Pass 7), this constructor is **crate-private** (`pub(crate)`) so production callers cannot accidentally create an un-supervised service. It is retained for tests that explicitly assert on the un-supervised path. `LspService::new_arc(config) -> Arc<Self>` is the production constructor: it builds the service via `Arc::new_cyclic(|weak| Self { ..., self_ref: OnceLock::from(weak.clone()), ... })`, which wires the back-reference and guarantees `ensure_exit_receiver_started` can self-activate from `&self` callers. The test `new_arc_wires_self_ref` proves the production constructor populates `self_ref` (read via the `Weak` upgrade). No public production path creates an un-supervised service.

### Test Timing Fix

`generation_is_identical_across_health_and_exit_event` previously overwrote the generation-3 scenario before generation 2 started, causing the gen-2 process to read the gen-3 scenario. The test now writes the gen-3 scenario only AFTER `service.generation_for_key(&key) >= 2` is observed, and the gen-2 process is verified `Ready` before the gen-3 file is staged. The gen-3 process is also verified `Ready` before a stale gen-1 exit event is injected.

### Restart Ownership and Live Outcome Semantics (Pass 1-10)

The Pass 1-10 sequence added explicit ownership and outcome semantics on top of the supervisor/restart scaffolding. The goal is to make every restart attempt observable and bounded, so that two simultaneous restart requests cannot silently corrupt the live client.

**Pass 1 — Restart completion channel and supersession waiter.** `RestartCompletion` is a `tokio::sync::watch` channel that tracks the in-flight coordinator's lifecycle. `RestartOwnerWaiter { completion: RestartCompletion }` is what `cancel_restart_ownership` returns — it lets a caller observe when the in-flight coordinator actually completes. `acquire_restart_ownership` returns `RestartLease { token }`; the coordinator checks the token at every cancellation boundary (pre-backoff, mid-sleep, post-spawn, pre-publish, pre-replay).

**Pass 2 — Manual supersession with bounded wait.** `LspService::manual_restart_client(key)` now acquires the manual lease *first*, then waits under `MANUAL_SUPERSESSION_OWNER_TIMEOUT = 3s` for any in-flight automatic-restart owner to complete via its `RestartCompletion::Finished` signal. On timeout, the manual caller aborts without touching the live client; on observed completion, the manual caller proceeds and is the only live coordinator for `key`.

**Pass 3 — Generation-aware runtime install.** `RuntimeInstallResult` (`Installed` / `Replaced { prior }` / `Rejected { existing_generation, requested_generation }`) is returned from `install_runtime`. A monitor that observes its own generation has been superseded terminates the orphan runtime rather than leaving it to drive future publication.

**Pass 4 — Unpublished replacement and generation-scoped cleanup.** The coordinator's reinit closure now returns `UnpublishedReplacement { client, generation }` (with manual `Debug`) instead of `Arc<LspClient>`. Cleanup helpers `remove_unpublished_client_if_generation` and `terminate_unpublished_runtime` only touch unpublished resources when the stored generation matches the supplied one. A delayed old monitor cannot remove a newer generation's replacement.

**Pass 5 — Unified internal restart entry.** `OwnedRestartOptions` (`automatic()` / `manual()` constructors) is the internal options type; `restart_client_owned` is the unified internal entry. `manual_restart_client` and `restart_client` are now thin delegators; manual teardown cannot bypass the ownership layer.

**Pass 6 — Degraded as a live outcome.** `restart_client_coordinator` now returns `Result<RestartOutcome, LspError>` where `RestartOutcome = Ready | Degraded { reason }`. `ReadinessResult::Degraded` no longer maps to `LspError::LaunchFailed`; it is a *live* outcome. The live client remains published, the consumed attempt remains consumed, and `last_healthy_at` is NOT updated. The orchestrator then converts `RestartOutcome` back to `Result<(), LspError>` for the public API surface.

**Pass 7 — Empty diagnostics readiness.** The fake-server scenario engine's default `emit_progress = true` emits an empty `textDocument/publishDiagnostics { uri: "file:///dummy", diagnostics: [] }` on the `initialized` notification, which accidentally satisfies `wait_for_first_diagnostics`. The new test file `crates/egglsp/tests/empty_diagnostics_readiness.rs` has two tests: `empty_publish_diagnostics_satisfies_readiness` (the realistic case) and `missing_diagnostics_notification_times_out` (sets `emit_progress: false` in scenario to verify the timeout path).

**Pass 8 — User restart policy round-trip.** `LspClientDescriptor::from_resolved(...)` is the production constructor that accepts an explicit `readiness_policy` and `restart_policy`. It is called from `LspService` when the user has configured `[lsp.<server>.restart]`; the user override is validated via `LspRestartPolicyConfig::try_to_domain(&base_policy)` and merged with the profile's defaults. Invalid overrides fall back to the profile default with a warning. `from_profile(...)` is retained for the no-user-override path.

**Pass 9 — Race tests for manual supersession.** Two new scripted supervisor tests in `supervisor_restart_stdio.rs`: `manual_waits_for_cancelled_automatic_completion` (verifies that a manual restart issued while the automatic coordinator is still in flight returns a typed `InitializationCancelled` / `ServerRestarted` / `LaunchFailed` or succeeds after bounded wait — never panics, never corrupts the live client) and `manual_restart_back_to_back_does_not_deadlock` (two manual restarts on the same key both return within timeout — no deadlock under rapid back-to-back issuance). Both tests accept `Ok`, `InitializationCancelled`, `ServerRestarted`, or `LaunchFailed` (budget exhausted by the auto) as valid outcomes; the critical invariant is bounded execution and a coherent service state.

**Pass 10 — Documentation sync.** This section. The architecture, skill guide, README, and AGENTS.md are all updated to reflect Pass 1-10 semantics.

**Pass 11 (Phase 3 final closure) — remove-before-signal handshake.** `RestartLease::release` is now `async fn` and the per-key slot is removed from the per-key map **before** `RestartCompletion::Finished` is broadcast on the watch channel. This inverts the previous "send Finished, then remove" ordering: `Finished` is now the *consequence* of slot removal, not the trigger. Lock contention cannot produce a false `InitializationCancelled` result after a successful owner completion, because the broadcast is what guarantees the slot is free.

The final handshake sequence is:

1. Caller cancels the lease token (or the coordinator observes one of its cancellation boundaries).
2. The owner unwinds its critical sections and reaches the `release` point.
3. The owner locks `restart_tasks`, **removes** the owner-ID-matched `RestartTaskControl` from the per-key map, and releases the lock.
4. **Only then** does the owner broadcast `RestartCompletion::Finished` on the watch channel.
5. The waiter (e.g. a manual supersession caller) observes `Finished`, returns `Ok`, and immediately proceeds to acquire a new lease.
6. A new owner acquires the now-free slot without racing the old owner.

**Waiter simplification.** Because removal happens *before* the broadcast, `RestartOwnerWaiter::wait` no longer needs a separate `verify_slot_free` post-check after observing `Finished`. The completion signal is the slot-release signal — by the time the waiter wakes up, the slot is provably free. If removal was skipped (entry absent, or owned by a newer owner) the broadcast is deliberately suppressed: the sender is dropped, any waiter observes channel closure, and the closure is treated as an invariant failure rather than a success.

**`Drop` safety fallback.** `Drop` of `RestartLease` is a safety net for panic / early-return paths. It marks the lease as released, spawns an async cleanup task, and runs the same remove-before-signal ordering inside the spawned task. Production ownership paths MUST `await` `RestartLease::release` directly; `Drop` exists only to guarantee the slot is not leaked if the caller forgets. Because `Drop` cannot move `self` out of `&mut self`, the fallback clones the `key`, `owner_id`, `restart_tasks` `Arc`, and `completion_tx` sender, and lets the original `self` continue to drop naturally.

**Adversarial unit tests.** Four new unit tests in `crates/egglsp/src/restart.rs` lock the invariant down:

- `finished_is_not_observable_until_slot_is_removed` — establishes a watch subscriber before the owner releases, then asserts the subscriber does not see `Finished` until the per-key map entry is gone.
- `drop_fallback_removes_before_finished` — drops the lease without calling `release`, asserts the cleanup task removes the slot before signaling.
- `old_owner_release_does_not_signal_for_new_owner` — simulates a newer owner acquiring the slot; the old owner's `release` observes the generation mismatch, suppresses the broadcast, and the new owner is never falsely told the slot is free.
- `completion_channel_close_without_finished_is_error` — drops the sender without sending; the waiter observes `RecvError` and treats it as an invariant failure (not a silent `Ok`).

**Pass 12 (Phase 3 final cleanup) — async release cancellation safety.** The Pass 11 release path commits `released = true` **inside** the ownership-map lock block, not before it. The flag commit and the slot removal are now part of the same synchronous critical section under the lock, so cancelling the `release()` future while it is parked on the lock await leaves `released == false` and routes cleanup to `Drop`'s safety fallback (which already runs the remove-before-signal ordering). Cancelling or aborting the future *after* lock acquisition cannot interrupt the critical section because there are no further `await` points between the flag commit and the completion broadcast.

Concretely:

- Cancellation before lock acquisition → `Drop` sees `released == false` → spawns the cleanup task → slot is removed and `Finished` is broadcast under the spawned task's own lock acquisition.
- Cancellation after lock acquisition → the synchronous critical section (flag commit → map.remove → broadcast) runs to completion. There is no `await` to interrupt; the slot is removed and `Finished` is broadcast before the future can be cancelled.

Production ownership paths continue to `await` `RestartLease::release` explicitly. The cancellation-safety change only affects the ordering of the `released` flag commit and the lock acquire; it does not alter any production call site or any release-side handshake semantics.

Two new adversarial unit tests in `crates/egglsp/src/restart.rs` lock the invariant down:

- `cancelled_async_release_falls_back_to_drop_cleanup` — spawns a release task that is blocked on the ownership-map lock (held by a separate `lock_holder` task), aborts the release task while it is parked, then verifies that the lease's `Drop` fallback removes the slot and the waiter observes `Finished`. The test is deterministic across 10 serial runs.
- `completion_channel_close_error_names_owner` / `completion_timeout_error_names_owner` — verify that both waiter error variants embed the in-flight `owner_id` (and the timeout duration for the timeout path) so the caller can correlate failures with the original coordinator.

The `RestartOwnerWaiter::owner_id` field is no longer `#[allow(dead_code)]`; both error variants now use it for diagnostics.

**Status.** Phase 3 supervision and restart lifecycle is complete for Tier 1 servers; broader language/server compatibility remains future work.

### Final Invariant Checklist

- [x] Old monitor cannot remove new runtime.
- [x] Runtime-map removal checks generation.
- [x] Shutdown sets graceful intent before protocol request.
- [x] Hung server is force-killed and reaped.
- [x] Runtime map is empty after shutdown.
- [x] Only coordinator chooses replacement generation.
- [x] Manual restart terminates old runtime first.
- [x] One restart coordinator exists per key.
- [x] Rapid crash cycles exhaust one shared budget.
- [x] Healthy interval resets budget.
- [x] Old diagnostics survive as stale evidence.
- [x] New diagnostics replace stale evidence.
- [x] Generation 1 has `post_restart = false`.
- [x] Generation 2+ has `post_restart = true`.
- [x] Progress wait requires completed cycle.
- [x] Real-server readiness uses production primitive.
- [x] Restart config is validated.
- [x] Partial user config inherits profile values.
- [x] Cold start and restart receive identical resolved settings.
- [x] Real-server reports include stderr when emitted.
- [x] Zero references fails advertised-reference check.
- [x] No public unsupervised service constructor remains.
- [x] Restart completion channel is the only completion signal.
- [x] Manual supersession waits under bounded timeout for in-flight owner.
- [x] Runtime install rejects same- or newer-generation replacement.
- [x] Unpublished replacement cleanup is generation-scoped.
- [x] Manual and automatic restarts share one internal entry point.
- [x] Degraded readiness is a live outcome, not an error.
- [x] Empty-diagnostics readiness and missing-diagnostics timeout are tested independently.
- [x] User restart policy override round-trips through validation.
- [x] Race tests cover manual supersession under in-flight automatic owner.
- [x] Race tests cover back-to-back manual restarts (no deadlock).
- [x] Documentation accurately states Phase 3 status.
- [x] Slot removal occurs before `Finished`.
- [x] Waiter cannot complete while old owner entry remains installed.
- [x] Waiter success permits immediate new acquisition.
- [x] Drop fallback preserves remove-before-signal ordering.
- [x] Delayed old-owner cleanup cannot remove a newer owner.
- [x] Channel closure without `Finished` remains an error.
- [x] All explicit release call sites await async release.
- [x] Ten serial and five parallel race runs pass.
- [x] No fake-server child process leaks.
- [x] Documentation describes the final ordering correctly.
- [x] `released` is committed only inside the ownership-map lock block (cancellation safety).
- [x] Aborting a blocked `release()` future triggers the `Drop` fallback cleanup.
- [x] Waiter timeout and closure errors embed the in-flight `owner_id` for diagnostics.
- [x] Ten serial abort-while-blocked runs pass deterministically.

## See Also

- [.opencode/skills/lsp/SKILL.md](../.opencode/skills/lsp/SKILL.md) - LSP skill guide
- [tool.md](tool.md) - LSP tool wrapper
- [plans/lsp_phase1_cleanup_and_phase2_scripted_stdio_harness.md](../plans/lsp_phase1_cleanup_and_phase2_scripted_stdio_harness.md) - Phase 1 + Phase 2 plan
