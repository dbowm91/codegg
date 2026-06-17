---
name: lsp
description: LSP client-side integration for Language Server Protocol support
version: 1.5.0
tags:
  - lsp
  - language-server
  - diagnostics
  - code-intelligence
  - client-side
---

# LSP (Language Server Protocol) Guide

This skill covers the LSP module for language server integration in opencode-rs.

## Overview

The LSP implementation lives in the `egglsp` workspace crate (`crates/egglsp/`). `src/lsp/mod.rs` is a thin compatibility shim that re-exports `egglsp::*` and bridges config/error types. The model-facing tool is at `src/tool/lsp.rs`. Phase 2 integration tests now live under `crates/egglsp/tests/`: the production-harness tests use `ProductionClientHarness`, and `scenario_engine.rs` includes the fake-server self-tests.

LSP is exposed as a native tool via `LspTool`, returning compact agent-facing summaries (not raw LSP JSON). Model-facing line and column are 1-indexed; the wrapper converts to LSP 0-indexed.

## Directory Structure

```
crates/egglsp/src/          # Authoritative LSP implementation
├── client.rs               # LspClient - JSON-RPC, diagnostics cache, notification parser
├── compatibility.rs        # LspCompatibilityProfile, readiness/restart policies, version detection, CompatibilityRequirement
├── config.rs               # LspConfig, LspRule types
├── diagnostics.rs          # DiagnosticsCollector
├── document_sync.rs        # OpenDocumentRegistry, document replay after restart
├── edit.rs                 # Workspace edit preview, text edit application, unified diff generation
├── download.rs             # Binary download/cache
├── error.rs                # LspError
├── health.rs               # LspOperationalState, health state machine, snapshots, context_note()
├── language.rs             # Language detection from file extensions
├── launch.rs               # Process spawning, Content-Length framing, background stderr drain
├── operations.rs           # LspOperations - goto definition, hover, etc.
├── overlay.rs              # OverlaySession, OverlayRestoreToken, semantic check preview (content or patch)
├── restart.rs              # LspClientDescriptor, RestartTrigger, restart_client_coordinator
├── root.rs                 # Project root detection
├── runtime.rs              # LspProcessRuntime, LspProcessIntent, spawn_process_runtime
├── server.rs               # 39 server definitions
├── service.rs              # LspService - client management, file-based routing, readiness, generation_map
├── supervisor.rs           # LspProcessExitEvent, StderrRingBuffer (100 lines / 64KB cap)
└── tests/                  # Phase 2 stdio integration tests (fake-server + production harness)

src/lsp/mod.rs              # Thin re-export shim (compatibility only)
src/tool/lsp.rs             # Model-facing LSP tool with compact DTOs
```

## Key Types

### Lsp (`mod.rs`)

Main entry point combining service, operations, and diagnostics:

```rust
pub struct Lsp {
    pub service: Arc<LspService>,
    pub operations: Arc<LspOperations>,
    pub diagnostics: Arc<DiagnosticsCollector>,
}
```

### LspClient (`client.rs`)

JSON-RPC client managing LSP server process. Uses a background reader
task for message dispatch (no more request-owned reads):

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
```

The `pending` map routes response IDs to oneshot senders. The
`_reader_task` continuously reads framed JSON-RPC messages from stdout
and classifies them via `classify_json_rpc_message`. Responses are
routed to pending senders; notifications are dispatched via
`dispatch_notification`.

**Request ID Generation:**
- Uses `AtomicU64` for wrap-around safety (was `AtomicI64`)
- `fetch_add(1, Ordering::SeqCst)` for sequential IDs
- No special wrap-around check needed with unsigned integer

### Edit Preview Types (`edit.rs`)

```rust
pub struct WorkspaceEditPreview {
    pub title: String,
    pub files: Vec<FileEditPreview>,
    pub total_files: usize,
    pub total_edits: usize,
    pub truncated: bool,
}

pub struct FileEditPreview {
    pub file: PathBuf,
    pub original_hash: String,
    pub edits: Vec<TextEditPreview>,
    pub patch: String,
    pub patch_omitted: bool,
}

pub struct TextEditPreview {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub replacement_preview: String,
}
```

These types are re-exported from `egglsp` at the crate root (e.g. `egglsp::WorkspaceEditPreview`).

### LspServerDef (`server.rs`)

Server definition with 39 server implementations:

```rust
pub struct LspServerDef {
    pub id: &'static str,           # e.g., "rust-analyzer"
    pub languages: &'static [&'static str],
    pub extensions: &'static [&'static str],
    pub repo: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    pub download: Option<DownloadSpec>,
}
```

### SemanticContextCollector

**Location:** `src/lsp/semantic_context.rs`

A shared semantic-read-model builder for `semanticContext`. Produces `SemanticContextResponse` by collecting diagnostics, symbols, definitions, references, and hierarchy data. Overlay translation and source-action hints remain handler-local by design: patch/content expansion is tool-specific, and source-action hints produce `WorkspaceEditPreview` payloads that are preview-rich and tool-specific, so the collector never handles either.

```rust
pub struct SemanticContextCollector { ... }
impl SemanticContextCollector {
    pub fn new(service, operations, diagnostics, allowed_root) -> Self;
    pub async fn collect(&self, request: SemanticContextRequest)
        -> Result<SemanticContextResponse, String>;
}
```

The collector handles: source excerpt construction, diagnostic snapshots with freshness metadata, document symbol flattening, definition/reference gathering with capability gating, call/type hierarchy summaries, per-section truncation, and structured unavailable metadata. Overlay translation and source-action hints are intentionally excluded — the tool handler owns both because overlay patch/content handling and source-action `WorkspaceEditPreview` payloads are tool-specific.

### LspCompatibilityProfile (`compatibility.rs`)

Per-server compatibility profile with readiness and restart policies:

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
```

```rust
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
```

```rust
pub enum CompatibilityRequirement {
    Required,
    RequiredIfAdvertised,
    Optional,
    KnownLimitation,
}
```

Key functions: `rust_analyzer_profile()`, `pyright_profile()`, `profile_for_server(id)`, `tier1_profiles()`, `require_server_binary(id)`. The real-server harness classifies each `LspCompatibilityCheck` with a `CompatibilityRequirement` and calls `assert_required_checks(report)` to fail the test on `Required` regressions.

### LspCompatibilityStatus (`compatibility.rs`)

| Status | Meaning |
|--------|---------|
| `Passing` | Server binary found, initializes, basic operations work |
| `PassingWithKnownLimits` | Server works but has documented limitations (e.g. no call hierarchy) |
| `Failing` | Server found but fails to initialize or produce valid responses |
| `Skipped` | Check was skipped (advertised feature not exercised) |
| `Unsupported` | Server binary not found on PATH and no download available |

### LspOperationalState (`health.rs`)

Operational state machine for LSP server processes:

```rust
pub enum LspOperationalState {
    Starting,
    Initializing,
    Indexing,
    Ready,
    Degraded { reason: String },
    RestartScheduled { reason: String },
    Restarting,
    Failed { reason: String },
    Stopping,
    Stopped,
}
```

**Transitions:**
- `Starting → Initializing → Indexing → Ready`
- `Ready → Degraded → RestartScheduled → Restarting → Initializing`
- `Starting/Initializing/Indexing/Ready → Failed → RestartScheduled`
- `Ready → Stopping → Stopped`

`InvalidTransition` is returned for invalid state changes. `LspOperationalHealthSnapshot` carries the current state, generation, uptime, restart count, and last error.

### StderrRingBuffer (`supervisor.rs`)

Capped ring buffer (100 lines / 64KB) for capturing LSP server stderr. Oldest lines are dropped when the cap is exceeded. `LspProcessExitEvent` records the exit code, signal, generation, expected flag, and stderr tail for crash analysis. The `expected` flag is derived from `LspProcessIntent` at exit time, not from transport state.

### LspProcessRuntime (`runtime.rs`)

Authoritative process owner. One task owns the child handle, stderr ring buffer, intent receiver, and kill channel. The runtime task is created via `spawn_process_runtime(server_id, root, generation, child, stderr)` and returns an `LspProcessRuntime` handle plus the owner's `JoinHandle`. The monitor never retains an `Arc<LspClient>` while waiting on the child.

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
```

`LspClient::shutdown()` is the higher-level helper that combines the protocol shutdown (`request_protocol_shutdown` — sends only `shutdown` / `exit`) with the service's runtime-termination sequence. The service helper `terminate_runtime(key, generation, client, graceful_deadline, absolute_deadline, reason)` sets the runtime intent BEFORE the protocol request, awaits graceful exit, force-kills on timeout, and reaps under the absolute deadline. `LspProcessRuntime::stderr_tail_capped(max_lines)` returns the most recent lines from the bounded stderr ring buffer; the real-server smoke harness populates `LspCompatibilityReport.stderr_tail` from this accessor. Exit events whose `event.generation != current_generation` are rejected as stale by the service.

### LspClientDescriptor and Restart Coordinator (`restart.rs`)

Persisted per-client launch spec used to reconstruct a client on restart. Built by `LspClientDescriptor::from_profile(key, server_id, root, launch_spec, seed_file, user_initialization, user_workspace_configuration)`. Resolution priority: explicit user config → profile default → server definition default.

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
```

`restart_client_coordinator<S, F>(shared, key, trigger, attempt, descriptor, reinit_fn)` is the single source of truth for retry/backoff/exhaustion/cancellation. The coordinator owns generation increment (via `RestartShared::next_generation_for_key(key)`), restart-state transition, current-client removal, old runtime shutdown, retry/backoff loop, client reinitialization from the descriptor, readiness wait, document replay, ownership restoration, diagnostics stale marking, and final `Ready` / `Failed` transition. The reinit closure receives the generation as an argument (`FnMut(&LspClientDescriptor, u64) -> BoxFuture<...>`) and must not compute generation independently. The coordinator calls `next_generation_for_key` exactly once per restart. On exhausted retries it returns `LspError::LaunchFailed("restart attempts exhausted (max=N)")`; on stale generation it returns `LspError::ServerRestarted`.

`backoff_delay(attempt, policy)` is `min(policy.initial_backoff * 2^(attempt-1), policy.max_backoff)`. `LspService::set_last_healthy_now(key)` records the timestamp when readiness reaches `Ready`; `LspService::reset_restart_attempts_if_healthy_inherent(key, reset_after_healthy)` lazily resets the shared `restart_attempts` counter to 0 when the next unexpected exit observes a healthy interval. `LspService::increment_restart_attempts(key)` is called once per actual replacement spawn and is shared across rapid crash cycles — a successful short-lived replacement does NOT reset the counter on its own.

`LspRestartPolicyConfig::try_to_domain(&self, base: &LspRestartPolicy) -> Result<LspRestartPolicy, LspError>` (in `crates/egglsp/src/config.rs`) validates user overrides before they reach the descriptor. It rejects `mode = OnUnexpectedExit` with `max_attempts == 0`, rejects `initial_backoff_ms > max_backoff_ms`, and rejects durations that overflow `Duration::MAX`. `LspRestartPolicyConfig::merge_with_profile` copies non-`None` fields from the profile, so partial user overrides inherit unspecified profile values. Cold start and restart use the same persisted `LspClientDescriptor` — they receive identical `launch_spec`, `initialization_options`, `workspace_configuration`, `readiness_policy`, and `restart_policy`.

### LspService generation and operational health (`service.rs`)

- `LspService::new(config)` returns the bare value; it is the test-only constructor and does NOT wire the back-reference, so the exit-receiver task is not auto-started. `LspService::new_arc(config)` is the only public production constructor — it returns an `Arc<Self>` with the back-reference set via `Arc::new_cyclic` so `ensure_exit_receiver_started` can self-activate from `&self` callers. Production paths MUST use `new_arc`.
- `generation_for_key(key)` / `set_generation(key, gen)` are the per-key generation accessors. The first publish sets generation `1`; the restart coordinator bumps it after successful reinit + replay.
- `next_generation_for_key(key) -> u64` is the single source of truth for replacement generation. The restart coordinator calls it exactly once per restart and passes the result to the reinit closure as an argument; the closure must not compute generation independently.
- `manual_restart_client(key)` is the public manual-restart API. It bypasses `LspRestartMode::Disabled` and terminates the old runtime with `RuntimeTerminationReason::ManualRestart` before starting the replacement so a manual restart cannot leave two live processes. A manual restart issued while an automatic restart is in progress supersedes it.
- `transition_operational_state(key, next)` is the centralized state mutator. It calls `health::transition()` to validate the move and rejects invalid transitions with `InvalidTransition`.
- `operational_health_snapshot(key)` returns an `LspOperationalHealthSnapshot` even when no live client exists (during `Restarting`, `Failed`, `Stopped`). The snapshot carries `transport: Option<ClientTransportSnapshot>`, real `last_message_age_ms` / `last_diagnostics_age_ms`, `restart_attempts`, `last_error: Option<String>`, and `stderr_tail: Vec<String>`.
- `wait_for_readiness(key, policy)` honors all four `LspReadinessPolicy` variants and returns `ReadinessResult::Ready { elapsed }` or `ReadinessResult::Degraded { reason, elapsed }`. The production state machine uses this to drive `Indexing` → `Ready` and timeout → `Degraded` transitions.
- `mark_diagnostics_stale_for_key(key)` re-keys retained diagnostics to `current - 1` so the freshness classifier returns `Stale` until the new server emits its first push.
- `snapshot_diagnostics_for_restart(key)` captures the live diagnostic cache for the current client (or returns an empty map if no client exists). The restart coordinator passes the captured map to the new client's `LspClient::install_retained_diagnostics("restart", retained)`, preserving original `server_generation` and `post_restart` flags. The retained entries classify as `Stale` because their generation differs from the new current generation.
- `increment_restart_attempts(key)` is called once per actual replacement spawn (before the coordinator runs) and is shared across crash cycles.
- `set_last_healthy_now(key)` records the timestamp when readiness reaches `Ready`; it feeds the lazy healthy-reset evaluation.
- `reset_restart_attempts_if_healthy_inherent(key, reset_after_healthy)` lazily resets `restart_attempts` to 0 when the next unexpected exit observes a healthy interval. Returns `Some(prev)` when the reset applies, `None` otherwise.
- `LspClient::progress_snapshot()`, `wait_for_progress_end(timeout)`, `wait_for_first_diagnostics(timeout)`, and `operational_summary()` provide the per-client progress/diagnostics observability that backs the readiness policies. `wait_for_progress_end` requires `ProgressState.completed_cycle == true` — empty `active_tokens` alone is not sufficient.

### Generation-Aware Runtime Map and Termination (`service.rs` / `runtime.rs`)

`runtime_map` is now `HashMap<String, RuntimeEntry>` where `RuntimeEntry { generation: u64, runtime: LspProcessRuntime }`. Insertion, lookup, and removal go through three internal helpers that all check the stored generation:

- `install_runtime(runtime_map, key, generation, runtime)` — replaces the prior entry only when the existing entry's generation is strictly older; rejects same- or newer-generation replacement at warn.
- `runtime_for_generation(runtime_map, key, generation)` — returns the runtime only when the stored generation matches.
- `remove_runtime_if_generation(runtime_map, key, generation)` — removes the entry only when the stored generation matches. A delayed old monitor cannot remove a newer generation's runtime through this helper.

`LspClient::shutdown()` was separated from the runtime-termination helper. The client method sends only `shutdown` / `exit` notifications (now exposed as `request_protocol_shutdown`); it never waits on the child once the runtime owns it. The service helper `terminate_runtime(key, generation, client, graceful_deadline, absolute_deadline, reason)` runs the bounded sequence: lookup matching runtime → `runtime.request_graceful_shutdown()` (sets `LspProcessIntent::GracefulShutdownRequested` BEFORE the protocol shutdown) → send protocol shutdown under the graceful deadline → `runtime.wait_for_exit()` → on timeout, `runtime.request_force_kill()` (sets `LspProcessIntent::ForceKillRequested`) → `runtime.wait_for_exit()` under the absolute deadline → remove runtime if generation still matches. `RuntimeTerminationReason` distinguishes `ServiceShutdown` / `ManualRestart` / `FailedPublication`. `shutdown_all()` snaps clients with their authoritative generations and terminates all runtimes concurrently under one absolute deadline.

`LspProcessRuntime::stderr_tail_capped(max_lines) -> Vec<String>` returns the most recent `max_lines` lines from the bounded `StderrRingBuffer` (100 lines / 64KB cap) in chronological order. The real-server smoke harness attaches an `LspProcessRuntime` to each smoke client and populates `LspCompatibilityReport.stderr_tail` from this accessor.

### OpenDocumentRegistry (`document_sync.rs`)

Tracks open documents for replay after server restart. `OpenDocumentSnapshot` captures URI, language ID, version, and full text. On restart, the coordinator replays `didOpen` for every open document using the snapshot's preserved version (not hard-coded 1), restores the `document_owners` map, and updates the new client's `opened_files` state. Closed documents are not replayed. Replay failure transitions to `Degraded` (not silent `Ready`).

## Supported LSP Servers

| Language | Server ID | Command |
|----------|-----------|---------|
| Rust | `rust-analyzer` | `rust-analyzer` |
| Python | `pyright` | `pyright-langserver --stdio` |
| JS/TS | `typescript-language-server` | `typescript-language-server --stdio` |
| Go | `gopls` | `gopls` |
| C/C++ | `clangd` | `clangd` |
| Java | `jdtls` | `jdtls` |
| C# | `omnisharp` | `OmniSharp` |
| Ruby | `ruby-lsp` | `ruby-lsp` |
| Kotlin | `kotlin-language-server` | `kotlin-language-server` |
| Scala | `metals` | `metals` |
| Dart | `dart-analysis-server` | `dart language-server --client-id codegg` |
| Swift | `swift-sourcekit` | `sourcekit-lsp` |
| Haskell | `haskell-language-server` | `haskell-language-server-wrapper --lsp` |
| Lua | `lua-language-server` | `lua-language-server` |
| PHP | `php-language-server` | `php-language-server` |
| Perl/Raku | `perl-language-server` | `perl-language-server` |
| Zig | `zls` | `zls` |
| ... and more | | |

## Key Operations

### File Lifecycle

```rust
// Open file
lsp.service.open_file(path, content).await

// Update file content
lsp.service.update_file(path, content).await

// Save file
lsp.service.save_file(path, None).await

// Close file
lsp.service.close_file(path).await
```

When `save_file` is called with text content (`text: Some(...)`), it updates the `last_content_change_at` timestamp for the file, marking diagnostics as potentially stale. A bare save (`text: None`) sends the `didSave` notification without affecting freshness.

### Code Intelligence

```rust
// Goto definition
let locations = lsp.operations.go_to_definition(file_path, line, column).await

// Find references
let refs = lsp.operations.find_references(file_path, line, column).await

// Hover
let hover = lsp.operations.hover(file_path, line, column).await

// Document symbols
let symbols = lsp.operations.document_symbols(file_path).await

// Code actions
let actions = lsp.operations.code_actions(file_path, start_line, start_col, end_line, end_col, Vec::new(), None).await

// Completion
let completions = lsp.operations.completion(file_path, line, column, None, None).await

// Signature help
let sig = lsp.operations.signature_help(file_path, line, column).await

// Preview-only rename (returns WorkspaceEditPreview with unified diff patches; does not write)
let preview = lsp.operations.rename_preview(file_path, line, column, "new_name", Some(allowed_root)).await

// Preview-only format
let preview = lsp.operations.format_preview(file_path, Some(allowed_root)).await

// Preview-only source action (organize imports)
use egglsp::operations::SourceActionPreviewKind;
let kind = SourceActionPreviewKind::parse("source.organizeImports")?;
let preview = lsp.operations.source_action_preview(file_path, kind, Some(allowed_root)).await
```

## Tool Integration

LSP is exposed via `LspTool` in `src/tool/lsp.rs`:

```rust
pub struct LspTool {
    service: Arc<crate::lsp::service::LspService>,
    allowed_root: PathBuf,
}
```

Operations available via tool:
- `goToDefinition`
- `findReferences`
- `hover`
- `documentSymbol`
- `workspaceSymbol` (returns `WorkspaceSymbolSummary` with name, kind, file, start_line, start_column, container_name)
- `diagnostics` (returns `diagnostics_may_still_be_warming: bool` to indicate if the server may not have responded yet after a recent `didOpen`/`didChange`)
- `renamePreview` (preview-only; returns `WorkspaceEditPreview` {title, files:[{file, original_hash, edits, patch}], total_*, truncated}; never mutates)
- `formatPreview` (preview-only; same `WorkspaceEditPreview` shape)
- `sourceActionPreview` (preview-only; same `WorkspaceEditPreview` shape; accepts `action` parameter — currently only `source.organizeImports` with aliases `organizeImports`/`organize_imports`; command-only actions are rejected because command execution is disabled)
- `semanticCheckPreview` (accepts either `content` or a single-file unified diff `patch`; patch input is applied in memory against `file_path` via `OverlaySession` (`apply_overlay`/`restore`), collects diagnostics + symbols, restores disk content, never writes disk; multi-file patches are unsupported in this pass; operation-level root enforcement via `allowed_root`; returns `SemanticCheckPreview` with `diagnostics_may_still_be_warming`, `diagnostics`, `diagnostics_error`, `symbols`, `symbols_error`, `restored_disk_view`, `restore_error`; `execute_structured` sets `success=false` when `restore_error` is present)
- `semanticContext` (combines multiple LSP requests; returns `SemanticContextPacket` with bounded source excerpt + diagnostics + symbols + optional definitions/references/overlay + optional source-action hints + optional call/type hierarchy; read-only, bounded; per-section errors via `definitions_error`, `references_error`; overlay limits tracked by `overlay_diagnostics_truncated`; `result_count` includes overlay items and available source-action hints; source excerpt truncation is UTF-8-safe via char-boundary cutting; `include_source_actions` boolean input, default false, populates `source_actions` array of `SemanticSourceActionHint` objects; `include_call_hierarchy` boolean input, default false, populates `call_hierarchy` section with incoming/outgoing callers; `include_type_hierarchy` boolean input, default false, populates `type_hierarchy` section with supertypes/subtypes; overlay translation stays handler-local because patch/content handling is tool-specific)
- `callHierarchy` (requires file_path, line, column; optional `direction` parameter — `incoming`, `outgoing`, or `both` (default `both`); returns `CallHierarchySummary` with items, incoming, outgoing, errors, truncated)
- `typeHierarchy` (requires file_path, line, column; optional `direction` parameter; returns `TypeHierarchySummary` with items, supertypes, subtypes, errors, truncated)
- `securityContext` (security-review context packet; returns risk markers, security-relevant diagnostics/symbols, optional definitions/references/call hierarchy, optional overlay; read-only, bounded; accepts `security_categories` filter and `max_risk_markers` cap; `include_call_hierarchy` defaults true when position provided; reuses shared diagnostic freshness evidence and capability snapshot from the common LSP path)
- `hunkSourceContext` (hunk-aware source navigation; consumes unified diff, maps changed hunks to enclosing symbols, diagnostics, definitions, references, hierarchy data; read-only, bounded; pure navigator via `HunkSourceNavigator`; DTOs in `crates/egglsp/src/hunk_context.rs`, parser in `src/lsp/hunk_nav_parser.rs`, range primitives in `src/lsp/hunk_nav_ranges.rs`, navigator in `src/lsp/hunk_nav.rs`, collector in `src/lsp/hunk_nav_collector.rs`)

**Preview-only contract**: `renamePreview` / `formatPreview` / `sourceActionPreview` (and future edit previews) produce bounded unified-diff patches for review via `WorkspaceEditPreview`. `sourceActionPreview` currently supports only `source.organizeImports`; arbitrary code actions and command execution are intentionally rejected. `CodeAction` values with `command: Some(_)` but `edit: None` are classified as command-only and rejected. `format_preview` enforces `allowed_root` at the crate layer. Large patches are structurally flagged via `FileEditPreview.patch_omitted` (not string matching). They are `ToolCategory::ReadOnly`. Actual file changes require the separate mutating `apply_patch` tool (or equivalent). `codeLens` is not exposed in the model-facing schema. Source-action hints returned via `semanticContext` with `include_source_actions: true` follow the same preview-only contract — each hint's `preview` field carries a `WorkspaceEditPreview` when the action is available and has edits, or `None` when unavailable or command-only.

### Semantic context packets

`semanticContext` is the preferred agent-facing pre-edit/pre-review context operation. It combines a bounded source excerpt with current diagnostics, document symbols, optional definition/reference information, and optional overlay diagnostics for proposed content or a single-file patch. It is read-only and never applies changes. The shared semantic read model is assembled by `SemanticContextCollector`; overlay translation stays in the tool layer.

Input parameters:
- `file_path` (required)
- `line`, `column` (optional, both-or-neither): 1-indexed target position
- `radius` (optional, default 40, max 120): lines above/below for excerpt
- `include_references` / `include_definitions` / `include_overlay` / `include_source_actions` (optional booleans)
- `include_call_hierarchy` (optional, default false): include call hierarchy information (requires line+column); requests without a target position are rejected rather than silently omitted
- `include_type_hierarchy` (optional, default false): include type hierarchy information (requires line+column); requests without a target position are rejected rather than silently omitted
- `content` / `patch` (optional, mutually exclusive): for overlay diagnostics

Source-action hints: when `include_source_actions` is true, `semanticContext` includes a `source_actions` array of `SemanticSourceActionHint` objects. Each hint has `action` (string identifier), `available` (bool), `preview` (Option\<WorkspaceEditPreview\>), and `error` (Option\<String\>). Currently only `source.organizeImports` is supported. Hints reuse the existing `sourceActionPreview` behavior (preview-only, no command execution, no mutation). Source-action failures are non-fatal; they set `error` on the individual hint but do not fail the whole packet. Available hints affect `result_count`. A pure helper `source_action_hint_from_result` converts results to hints, and `collect_source_action_hints` iterates the hardcoded allowlist.

All sections bounded: diagnostics (100), symbols (120), references (80), overlay diagnostics (100), excerpt (32KB). Per-section errors (`definitions_error`, `references_error`) are non-None when the corresponding LSP request fails. `overlay_diagnostics_truncated` in limits tracks overlay diagnostics overflow. `result_count` includes overlay diagnostics and overlay symbols. Source excerpt truncation uses `truncate_to_byte_limit_on_char_boundary` (UTF-8-safe, no replacement characters). All sections are best-effort; individual failures do not prevent the packet from being returned.

> **Architecture note:** `SemanticContextPacket` is a presentation adapter type. `SemanticContextCollector` assembles the shared semantic read model, and `SemanticContextPacket::from_semantic_response()` adapts that response into the tool-local packet. Overlay translation stays handler-local.

### securityContext operation

`securityContext` is a read-only context-gathering operation for security review. It is not a vulnerability scanner and does not produce vulnerability verdicts.

**Usage guidance:** Use `securityContext` before a security review of a target symbol or proposed patch. Treat risk markers as review prompts, not findings. Use explicit mutating tools only after reviewing returned patches or context.

It provides:

- Bounded source excerpt with configurable radius (default 80, max 200)
- Deterministic risk markers via pattern matching (11 categories: auth, crypto, filesystem, network, process, unsafe, serialization, sql, secrets, path_traversal, concurrency)
- Security-relevant symbols and diagnostics (filtered by keyword matching and proximity to risk markers; filtered before capping so relevant items are not dropped)
- Optional definitions, references, call hierarchy, and overlay diagnostics
- Risk marker category filtering and configurable caps (default 80, max 200)
- Nonfatal error notes when LSP subrequests fail (diagnostics, symbols, definitions, references)

**Key properties:**
- Read-only: never writes files; patch/content input is applied only in memory through the overlay path
- Deterministic: same input produces same output
- Bounded: all sections have configurable caps
- Context, not verdict: provides risk markers with rationale, not vulnerability assessments
- Precise truncation: flags reflect filtered counts, not raw counts

**Limits:** risk markers (default 80, max 200), excerpt radius (default 80, max 200 lines), security diagnostics (80), security symbols (80), references (80).

**Input parameters:** `file_path` (required), `line`/`column` (optional, both required together), `radius` (default 80, max 200), `security_categories` (optional filter), `max_risk_markers` (default 80, max 200), `content`/`patch` (optional overlay, mutually exclusive), `include_call_hierarchy` (default true when position provided).

**Implementation:** Risk marker scanning, pattern tables, and security-relevant filtering helpers live in `src/tool/lsp_security.rs`.

**Security context presets:** `securityContext` supports optional presets via `security_preset`. Presets tune default risk categories, excerpt radius, marker count, and call-hierarchy inclusion. Supported presets: `rust_server`, `rust_cli`, `web_backend`, `dependency_review`, `unsafe_review`. Explicit input fields (`security_categories`, `radius`, `max_risk_markers`, `include_call_hierarchy`) override preset defaults. See `architecture/lsp.md` for the full preset table.

### Hunk/source navigation

`hunkSourceContext` is a read-only context-gathering operation that provides hunk-aware evidence for code review, edit planning, and navigation. It consumes a unified diff (patch) and maps changed hunks to enclosing symbols, nearby diagnostics, definitions, references, and hierarchy data.

**Input parameters:** `file_path` (required), `patch` (optional unified diff), `include_definitions` (default true), `include_references` (default true), `include_call_hierarchy` (default false), `include_type_hierarchy` (default false), `radius` (default 40), `max_hunks` (default 20).

**Output:** Per-hunk evidence (enclosing symbol, related symbols, diagnostics, definitions, references, call/type hierarchy, source excerpt, diagnostic freshness) plus truncation flags, notes, and a `truncated` flag.

**Key properties:**
- Read-only: never writes files; patch is parsed in memory
- Pure navigator: `HunkSourceNavigator` consumes `SemanticContextResponse` and does not call LSP directly
- Bounded: per-hunk caps on symbols, diagnostics, references; global cap on hunk count
- Diagnostic freshness is preserved per hunk from the semantic response
- Fail-open: policy skips and LSP errors produce notes, never block the caller
- Recommendation-based: the tool is invoked by the model when reviewing diffs; no automatic invocation

**Known limitations:**
- Single-file only: accepts `file_path` + `patch`, not a multi-file patch. Multi-file diffs require separate calls per file.
- First-hunk-centered: semantic context (definitions, references, hierarchy) is collected centered on the first hunk and shared across all hunks via range matching. A note is appended when multiple hunks are present.
- No cross-file references: definitions and references are limited to the single file; cross-file analysis requires `securityContext` or `semanticContext`.

**Implementation:** Diff parsing (`parse_unified_diff`) produces `HunkDescriptor` values. Range primitives (`hunk_nav_ranges`) handle overlap, containment, and symbol/diagnostic matching. `HunkSourceNavigator` assembles per-hunk evidence. `HunkSourceNavigationCollector` coordinates parsing + semantic collection.

### HunkSourceContextPolicy

`HunkSourceContextPolicy` (`src/lsp/hunk_nav_policy.rs`) controls when `hunkSourceContext` should be invoked. It is used by the security review workflow to decide whether to collect hunk navigation evidence for a given file.

```rust
pub struct HunkSourceContextPolicy {
    pub enabled: bool,                // master switch (default: true)
    pub max_patch_bytes: usize,       // skip patches larger than this (default: 64KB)
    pub max_hunks: usize,             // skip files with more hunks than this (default: 20)
    pub include_definitions: bool,    // (default: true)
    pub include_references: bool,     // (default: true)
    pub include_call_hierarchy: bool, // (default: false)
    pub include_type_hierarchy: bool, // (default: false)
}
```

`decide_hunk_source_context(policy, patch, file_path)` returns `HunkSourceContextDecision::Use { file_path, patch }` or `HunkSourceContextDecision::Skip { reason }`. Skip reasons include: disabled policy, no file path, unsupported file extension, oversized patch, no hunk headers, too many hunks. Supported extensions are LSP-covered languages (`.rs`, `.py`, `.ts`, `.js`, `.go`, `.java`, `.c`, `.cpp`, `.rb`, `.kt`, etc.).

### Compact summary formatter

`format_hunk_source_context_summary` (`src/lsp/hunk_nav_prompt.rs`) formats a `HunkSourceNavigationResponse` into a compact, bounded agent-facing text summary. The summary format is deterministic but the underlying evidence is best-effort and server-dependent. The output is bounded (max 5 symbols, 5 diagnostics, 5 references per hunk) and preserves freshness/truncation metadata. Used for prompt injection and security review notes.

### Security review workflow integration

The security review workflow (`src/security/workflow/report.rs`) optionally executes `hunkSourceContext` when `--hunk-context` is enabled via `enable_hunk_source_context: bool` (default: false) on `SecurityReviewWorkflowOptions`.

When enabled and an executor is available:
1. Hunks are grouped by file path; files are processed in deterministic sorted order
2. `decide_hunk_source_context` is called per file with actual per-file patch data
3. The `HunkSourceContextExecutor` trait (`src/security/workflow/context.rs`) provides the boundary; `LspHunkSourceContextExecutor` (`src/security/lsp_executor.rs`) is the real adapter that calls `LspTool::execute_hunk_source_context_typed()` directly with a typed `HunkSourceNavigationRequest` — no JSON round-trip. The model-facing tool schema remains patch-only; internal pre-parsed hunk descriptors are used via the typed API.
4. Per-file evidence (enclosing symbols, diagnostics, definitions, references) is collected via `collect_hunk_source_context_all_files` which returns a `HunkSourceContextCollectionResult` with evidence, summaries, notes, and `HunkSourceContextExecutionStats` (tracking files_considered, files_policy_skipped, requests_attempted/succeeded/failed/timed_out, evidence_items_emitted). Policy evaluation (Option B) happens before request-cap check. `files_considered` counts files whose policy was evaluated; `evidence_items_emitted` is assigned post-loop from `all_evidence.len()`. Request caps count actual executor calls, not loop position. The LSP evidence is best-effort and server-dependent.
5. Evidence is injected into the evidence-based synthesis as `HunkNavigation` and `Diagnostic` evidence items
6. `evidence_from_hunk_source_context` converts real `HunkSourceNavigationResponse` into `StructuredSecurityEvidence` — policy skip decisions are routing metadata, never evidence

The tightened eligibility gate requires `HunkNavigation` to appear alongside `RiskMarker` or `Preflight` (or other supporting dimensions) — `ChangedHunk + HunkNavigation` alone is not finding-eligible. Multi-file diffs are processed one file at a time (capped at 8 files), in deterministic sorted order.

Fail-open: per-file errors produce notes, never block the workflow. The policy skips unsupported file extensions, oversized patches, and files with too many hunks.

### Security call expansion

`securityContext` supports optional bounded recursive call expansion via `call_depth`. This is separate from the shared compact call hierarchy collected by `SemanticContextCollector`: the shared hierarchy provides only immediate incoming/outgoing relationships, while call expansion performs its own recursive BFS expansion handler-locally via `build_call_expansion_summary`.

- `call_depth`: 0 (default/off), 1, or 2. Higher values rejected.
- `max_call_nodes`: 32 (default), max 64. Caps total nodes.
- `call_direction`: `"incoming"`, `"outgoing"`, or `"both"` (default).

Expansion uses BFS with cycle detection (HashSet dedup). Edges to already-seen nodes are preserved. When caps are reached, expansion prefers returning a partial graph with `truncated=true` rather than failing the entire packet. `call_expansion.truncated` is true when nodes, edges, or per-edge ranges are dropped due to configured or internal caps (`capped_call_ranges`, `push_call_expansion_edge`, `push_call_expansion_node`). Errors are nonfatal and collected in `call_expansion.errors`.

Presets do NOT enable expansion. Only explicit `call_depth > 0` activates it.

Read-only: only LSP hierarchy requests, never writes files.

### Security review workflow

The `security-review` agent uses `securityContext` in a structured workflow (`src/security/workflow.rs`):

- **Target discovery**: Changed hunks from git diff, filtered for binary/vendor paths
- **Preset selection**: Per-file heuristics map to the 5 `securityContext` presets
- **Context strategy**: `call_depth=0` by default; escalated to 1 only for high-risk targets (unsafe, network, auth, process)
- **Synthesis rule**: Risk markers are review prompts, not findings. Findings require risk marker + changed code + evidence of flow, or preflight failure.

The workflow is invoked via the `/security-review` slash command or by spawning the `security-review` subagent.

The vertical slice entry point is `plan_security_review_from_diff(diff, repo_root)`. It parses unified diff hunks, applies path exclusions (`vendor/`, `third_party/`, `target/`, `dist/`, `build/`, `node_modules/`, `*.min.js`; notably does NOT exclude `Cargo.toml`, `Cargo.lock`, `build.rs`), selects `securityContext` presets, builds request payloads, converts risk markers to review prompts, and assembles reports with an explicit "not confirmed findings" note. In this pass, `call_depth` is always 0 and findings are always empty — risk markers become review prompts only.

### Hierarchy Output Shapes

Hierarchy operations (`callHierarchy`, `typeHierarchy`) follow a consistent shape. Both require `file_path`, `line`, and `column` (1-indexed). An optional `direction` parameter controls which callsites/type sites to retrieve. `semanticContext` can request them via `include_call_hierarchy` / `include_type_hierarchy`, and `securityContext` requests shared call hierarchy from `SemanticContextCollector` when a target position is provided.

**`HierarchyDirection`** accepts:
- `"incoming"` — callers / supertypes only
- `"outgoing"` — callees / subtypes only
- `"both"` (default) — both directions

Invalid values return an error.

Hierarchy operations are shallow and non-recursive — they prepare the target item and request only immediate relationships. Unsupported language servers may return empty sections or error fields. Prepare operations open/sync the file from disk before requesting.

#### CallHierarchySummary

Returned by `callHierarchy` and optionally embedded in `semanticContext` when `include_call_hierarchy` is true.

```json
{
  "items": ["CallHierarchyItemSummary", "..."],
  "incoming": ["CallHierarchyIncomingCallSummary", "..."],
  "outgoing": ["CallHierarchyOutgoingCallSummary", "..."],
  "errors": ["error string", "..."],
  "truncated": false
}
```

Items are the prepared call hierarchy symbols at the given position. Incoming/outgoing calls reference those items by ID. Each item summary includes `name`, `kind`, `file_path`, `start_line`, `start_column`, `end_line`, `end_column`. Each incoming/outgoing summary includes `from`/`to` (item summary) and `from_ranges`/`to_ranges` (list of `LocationSummary`).

#### TypeHierarchySummary

Returned by `typeHierarchy` and optionally embedded in `semanticContext` when `include_type_hierarchy` is true.

```json
{
  "items": ["TypeHierarchyItemSummary", "..."],
  "supertypes": ["TypeHierarchyItemSummary", "..."],
  "subtypes": ["TypeHierarchyItemSummary", "..."],
  "errors": ["error string", "..."],
  "truncated": false
}
```

Items are the prepared type hierarchy symbols at the given position. Supertypes/subtypes are flattened lists of all ancestors/descendants. Each item summary includes `name`, `kind`, `file_path`, `start_line`, `start_column`, `end_line`, `end_column`, `parents` (list of parent item summaries).

### Hierarchy behavior

`callHierarchy` and `typeHierarchy` are shallow, non-recursive operations. They prepare the target item and request immediate relationships only. `from_ranges` are capped at 32 per call; the `truncated` flag accounts for item, edge, and range truncation.

Unsupported language servers return empty sections or per-section error fields.

## Project Root Detection

The module detects project roots by looking for marker files:
- `.git`, `Cargo.toml`, `package.json`, `go.mod`, `pyproject.toml`
- `build.gradle`, `CMakeLists.txt`, `Makefile`, `Gemfile`, `mix.exs`
- `tsconfig.json`, `vite.config.ts`, `next.config.js`, etc.

## Binary Download/Caching

1. Checks PATH first for server binaries
2. Falls back to cached download in `$HOME/.cache/codegg/lsp/`
3. Only rust-analyzer has download specification currently
4. Supports zip, tar.gz, tar.xz extraction
5. Sets executable permissions on Unix (0o755)

## Bug Fixes Applied (2026-05-22)

### PATH Parsing Fixed (`download.rs`)

```rust
// ❌ Before - broken on Unix (split by wrong separator)
for dir in paths.split(std::path::MAIN_SEPARATOR) { ... }

// ✅ After - uses std::env::split_paths correctly
let path_var = std::env::var("PATH").ok()?;
let paths = std::env::split_paths(&path_var);
for dir in paths { ... }
```

### PHP Server Mapping Fixed (`language.rs`)

```rust
// ❌ Before - intelephense doesn't exist in server definitions
"php" => Some("intelephense"),

// ✅ After - correct server ID
"php" => Some("php-language-server"),
```

### Request Timeout Added (`client.rs`)

```rust
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub async fn send_request(...) -> Result<...> {
    // ... request setup ...
    let result = tokio::time::timeout(Self::REQUEST_TIMEOUT, async {
        // ... read loop ...
    }).await;

    match result {
        Ok(inner) => inner,
        Err(_) => Err(LspError::RequestTimeout(...)),
    }
}
```

### Hardcoded PATH Fixed (`launch.rs`)

```rust
// ❌ Before - hardcoded PATH ignored user's environment
.env_clear()
.env("PATH", "/usr/local/bin:/usr/bin:/bin")

// ✅ After - preserves user's PATH if available
.env_clear()
if let Some(user_path) = std::env::var_os("PATH") {
    cmd.env("PATH", user_path);
} else {
    cmd.env("PATH", "/usr/local/bin:/usr/bin:/bin");
}
```

### Stderr Logging (`client.rs`)

Server stderr is now logged during initialization:

```rust
let mut process = launch::spawn_server(...).await?;
let stderr_output = launch::drain_stderr(&mut process).await;
if !stderr_output.is_empty() {
    info!(server = server.id, stderr = %stderr_output, "LSP server stderr");
}
```

## Additional Bug Fixes (2026-05-22 - Session Review)

### Notification Loop Redundancy Fixed (`client.rs`)

The `send_request` method had redundant notification handling:

```rust
// ❌ Before - duplicate branches, silent ignore on send failure
if let Some(resp_id) = resp.get("id") {
    if resp_id.as_i64() == Some(id) { ... }
    let _ = self.notif_tx.send(resp_str);  // Always runs after match
} else {
    let _ = self.notif_tx.send(resp_str);  // Duplicate branch
}

// ✅ After - cleaner logic, logged send failures
if let Some(resp_id) = resp.get("id") {
    if resp_id.as_i64() == Some(id) { ... }
}
if let Err(e) = self.notif_tx.send(resp_str) {
    warn!(error = %e, "failed to send notification to channel");
}
```

### close_file Race Condition Fixed (`service.rs`)

The `close_file` method had lock handling issues that could cause race conditions:

```rust
// ❌ Before - dropped read lock before acquiring write lock (race!)
let clients = self.clients.read().await;
let key = { /* find key */ };
drop(clients);  // Lock dropped here
if let Some(key) = key {
    let mut clients = self.clients.write().await;  // Another task could modify between
    // ...
}

// ✅ After - uses single write lock, removes from opened_files
let client_idx = {
    let clients = self.clients.read().await;
    // find client index
};
// ...
let mut clients = self.clients.write().await;
if let Some(entry) = clients.values_mut().nth(client_idx) {
    let was_open = entry.client.opened_files.lock().await.contains_key(&uri_str);
    if was_open {
        let _ = entry.client.close_file(&uri).await;
        entry.client.opened_files.lock().await.remove(&uri_str);
    }
}
```

### save_file Race Condition Fixed (`service.rs`)

Similar fix for `save_file`:

```rust
// ❌ Before - dropped read lock before acquiring write lock
let clients = self.clients.read().await;
let key = { /* find key */ };
drop(clients);
if let Some(key) = key {
    let mut clients = self.clients.write().await;
    // ...
}

// ✅ After - uses single write lock
let client_idx = {
    let clients = self.clients.read().await;
    // find client index
};
// ...
let mut clients = self.clients.write().await;
if let Some(entry) = clients.values_mut().nth(client_idx) {
    return entry.client.save_file(&uri, text).await;
}
```

## Error Handling

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

**Phase 3 additions:**
- `ServerRestarted` — request targeted a server that has restarted; carries generation numbers for retry decisions
- `ServerUnavailable` — server in non-operational state (`Failed`, `Restarting`, `Stopped`)
- `ServerDegraded` — server in `Degraded` state; some operations may not work

### SharedInitError

A cloneable error type (`SharedInitError` with `SharedInitErrorKind` enum) used for
concurrent initialization waiters. All oneshot channel results carry `SharedInitError`
instead of raw `LspError`, preserving error category and message across threads.
Converts via `From<&LspError> for SharedInitError` and `into_lsp_error()` back to
`LspError`. Kinds: `ServerNotFound`, `DownloadFailed`, `LaunchFailed`,
`InitializeFailed`, `Timeout`, `Cancelled`, `Protocol`, `Other`.

## Capability Discovery

`egglsp::capability` provides a normalized boolean view of `ServerCapabilities` returned by the initialized LSP server.

### LspCapabilitySnapshot

```rust
pub struct LspCapabilitySnapshot {
    // Boolean flags for common operations
    pub goto_definition: bool,
    pub hover: bool,
    pub completion: bool,
    pub references: bool,
    pub document_symbols: bool,
    pub workspace_symbols: bool,
    pub rename: bool,
    pub code_actions: bool,
    pub signature_help: bool,
    pub formatting: bool,
    pub call_hierarchy: bool,
    pub type_hierarchy: bool,
    // ... additional flags
}
```

Built via `LspCapabilitySnapshot::from_capabilities(&ServerCapabilities, server_name, language_id)` which derives the snapshot from live server capabilities reported during `initialize`. The snapshot carries real `server_name` and `language_id` metadata from the initialized server.

### Querying Support

- `snapshot.supports(LspSemanticOperation::GotoDefinition)` → `bool`
- `snapshot.fallback_reason(LspSemanticOperation::Rename)` → `Option<&'static str>` — returns `Some("server does not support rename")` when unsupported, `None` when supported

### LspSemanticOperation

Enum covering all semantic operations the tool supports. Used for querying capability snapshots and for building fallback responses.

### LspUnavailable

Structured fallback response returned when an operation is not supported by the server. Constructed via `LspCapabilitySnapshot::unavailable(op)`.

### capabilities LspTool Operation

The `capabilities` operation on `LspTool` returns a `LspCapabilitySnapshot` for the active server. Callers can use it to decide whether to attempt an operation before investing in a full request.

## Diagnostics Freshness

`egglsp::diagnostics` provides diagnostics with freshness metadata so callers can judge reliability.

### LspDiagnosticSnapshot

```rust
pub struct LspDiagnosticSnapshot {
    pub file_path: String,
    pub freshness: LspDiagnosticFreshness,
    pub source: LspDiagnosticSource,
    pub diagnostics: Vec<lsp_types::Diagnostic>,
}
```

### LspDiagnosticFreshness

| Variant | Meaning |
|---------|---------|
| `Fresh` | Diagnostics arrived after the most recent `didOpen`/`didChange`/`didSave` |
| `PossiblyStale` | No response received yet; server may still be processing |
| `Stale` | File was modified after diagnostics were last received |
| `Unavailable` | No diagnostics are available (server not started, no `publishDiagnostics` received) |

### LspDiagnosticSource

| Variant | Meaning |
|---------|---------|
| `Pushed` | Received via `textDocument/publishDiagnostics` notification |
| `Pulled` | Retrieved via `textDocument/diagnostic` request |
| `Unknown` | Source not tracked |

### age_ms Semantics

`age_ms` is zero for `Unavailable` snapshots and elapsed diagnostic age (milliseconds since `received_at`) for all cached diagnostic snapshots, including `Stale` cached snapshots.

### Usability

- `snapshot.is_usable_evidence()` → `true` for `Fresh` and `PossiblyStale` (callers may choose to treat `PossiblyStale` as usable with a warning)
- `Stale` and `Unavailable` are explicitly flagged so callers can decide whether to re-request or skip

### Warming Detection

`LspDiagnosticSnapshot::diagnostics_may_still_be_warming()` is a derived method that returns `true` when freshness is `PossiblyStale` and diagnostics are empty, indicating the server may still be processing.

### Invalidation Rules

- A `didOpen` or `didChange` resets the freshness to `PossiblyStale` until the next `publishDiagnostics`
- A `didSave` resets freshness; the next `publishDiagnostics` marks it `Fresh`
- File modifications tracked via `last_opened_at` timestamps drive the `Stale` classification
- The `diagnostics_may_still_be_warming` flag on the `diagnostics` tool operation is derived from `PossiblyStale` freshness

### DiagnosticCacheEntry

`DiagnosticCacheEntry` (in `crates/egglsp/src/client.rs`) stores per-file diagnostics with `received_at`, `source`, and `content_version` metadata. `LspClient::diagnostic_snapshot()` classifies freshness based on these fields.

`DiagnosticsCollector::get_diagnostic_snapshot_for_file()` is the primary API for obtaining a snapshot with freshness metadata.

`DiagnosticsCollector::get_all_diagnostic_snapshots()` returns a `HashMap<String, LspDiagnosticSnapshot>` for freshness-aware bulk diagnostics. `get_all_diagnostics()` is a legacy freshness-blind view that returns raw diagnostics without freshness metadata.

### capabilities operation

The `capabilities` LspTool operation uses the shared `capability_snapshot_for_file()` helper, the same code path used by `semanticContext` and `securityContext`.

## Capability-Gated Operations

The `semanticContext` and `securityContext` handlers check `LspCapabilitySnapshot` before making optional expensive LSP calls. When a capability is unsupported, the operation is skipped and an error/note is appended instead of failing:

| Operation | Gated On | Unsupported Behavior |
|-----------|----------|---------------------|
| definitions | `LspSemanticOperation::Definition` | `definitions_error` set; no LSP request |
| references | `LspSemanticOperation::References` | `references_error` set; no LSP request |
| call hierarchy | `LspSemanticOperation::CallHierarchy` | semanticContext: `call_hierarchy` = None; securityContext: note appended |
| type hierarchy | `LspSemanticOperation::TypeHierarchy` | `type_hierarchy` = None |
| call expansion | `LspSemanticOperation::CallHierarchy` | securityContext: note appended; `call_expansion` = None |

When no capability snapshot is available (server not yet initialized), operations default to attempting the call (fail-open).

## Diagnostic Evidence in Context Packets

Both `SemanticContextPacket` and `SecurityContextPacket` include an optional `diagnostic_evidence` field:

```rust
struct DiagnosticEvidenceMeta {
    freshness: LspDiagnosticFreshness,
    source: LspDiagnosticSource,
    age_ms: i64,
    usable_evidence: bool,
}
```

The `age_ms` field is the age in milliseconds since diagnostics were received from the language server, not an absolute generation timestamp. The `usable_evidence` field is `true` when freshness is `Fresh` or `PossiblyStale`. The `securityContext` handler appends notes for stale/unavailable diagnostics:
- `"diagnostics stale: treating diagnostics as low-confidence evidence"` (Stale)
- `"diagnostics unavailable: no LSP diagnostic evidence available"` (Unavailable)

## Shared Semantic Context API

`egglsp::semantic_context` provides the domain-agnostic request/response DTOs for gathering semantic context. `SemanticContextResponse` is the internal semantic read model — tool adapters convert it into presentation-specific JSON shapes (e.g. `SemanticContextPacket` for `semanticContext`, or security-filtered subsets for `securityContext`).

The conversion flow is:

```
SemanticContextRequest → SemanticContextCollector::collect() → SemanticContextResponse → SemanticContextPacket::from_semantic_response()
```

### SemanticContextRequest

```rust
pub struct SemanticContextRequest {
    pub file_path: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
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

Builder methods: `with_position(line, column)`, `with_call_depth(depth)`, `with_overlay(bool)`, `with_source_actions(bool)`, `with_excerpt_radius(radius)`.

### SemanticContextResponse

The assembled semantic context response. This is the internal semantic read model that `SemanticContextCollector` produces. Tool adapters convert it into presentation-specific shapes.

```rust
pub struct SemanticContextResponse {
    pub file_path: String,
    pub symbol: Option<SemanticSymbolSummary>,
    pub all_symbols: Vec<SemanticSymbolSummary>,
    pub diagnostics: Vec<FileDiagnostic>,
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

### SemanticContextIntent

| Variant | Usage |
|---------|-------|
| `Explain` | General code explanation; fetch hover, definitions, references |
| `EditPlanning` | Pre-edit context; diagnostics, symbols, definitions, references |
| `Review` | Code review context; diagnostics, symbols, call/type hierarchy |
| `SecurityReview` | Security review; risk markers, security diagnostics, call hierarchy |
| `TestPlanning` | Test generation context; symbols, definitions, references |
| `Navigation` | Code navigation; definitions, references, symbols |

The intent drives which optional sections are populated and which caps are applied.

### SemanticContextCaps

```rust
pub struct SemanticContextCaps {
    pub max_symbols: usize,
    pub max_references: usize,
    pub max_diagnostics: usize,
    pub max_call_depth: u8,
}
```

Enforces bounded output. Defaults are conservative and aligned with the existing `semanticContext` operation limits. `enforce()` clamps the request fields to the configured caps.

### Supporting Types

- `SemanticSymbolSummary` — compact symbol (name, kind, file, start/end line/column)
- `SemanticLocation` — compact location (file, start/end line/column)
- `SemanticSourceExcerpt` — source text with start/end lines and truncation flag
- `SemanticDiagnosticEvidence` — freshness, source, age_ms, usable_evidence
- `SemanticOverlay` — overlay diagnostics/symbols with restore metadata
- `SemanticSourceActionHint` — action id, available flag, optional error
- `SemanticSectionTruncation` — per-section truncation metadata (section, original/emitted counts, limit)
- `SemanticContextLimits` — boolean flags for each section's truncation state

### Unavailable Responses

`LspCapabilitySnapshot::unavailable(op)` builds a structured fallback for unsupported operations. Used internally when a requested semantic context operation cannot be served because the server lacks the required capability.

## Protocol Peer Hardening

Codegg acts as a bidirectional JSON-RPC peer. The background reader classifies incoming messages into `Response`, `ErrorResponse`, `ServerRequest`, `Notification`, and `Unknown` variants. Server requests are dispatched via `dispatch_server_request` in `server_request.rs`. `is_structural_error()` validates JSON-RPC error codes as integers via `as_i64().is_some()` (rejecting fractional codes).

### Supported server requests
- `workspace/configuration` — scoped configuration lookup
- `workspace/workspaceFolders` — returns current root
- `client/registerCapability` / `client/unregisterCapability` — bounded dynamic registration tracking (256 max); processes full arrays with validation and deduplication; `register_batch()` pre-checks capacity before any mutation (atomic batch registration)
- `window/workDoneProgress/create` — acknowledged with null
- `workspace/applyEdit` — **always rejected** as an application-level result with `applied: false` and `failureReason` (not a JSON-RPC error; Codegg never applies implicit edits)

### Cancellation
Client request timeout triggers: (1) pending entry removal, (2) best-effort `$/cancelRequest` notification, (3) if that cancel write fails, `fail_transport()` marks the transport failed and drains pending, (4) `RequestTimeout` error. Server-request dispatch has a 5-second timeout that returns `-32603` (Internal error) on expiry.

### Initialization
Single-flight via explicit `InitRole` election: the first caller becomes `Leader` and
spawns an owned initialization task; concurrent callers become `Waiters` on the same
completion fan-out. The `InitSlot` stores one leader sender plus a waiter list so the
same result is broadcast to all callers. On failure, the slot is cleaned up by attempt ID
and waiters receive the actual `SharedInitError` (preserving error category and message),
allowing retries. Before publication, the init task rechecks lifecycle phase/generation;
if publication is invalidated or an existing client already won the key, the unpublished
client is shut down via `dispose_unpublished_client(...)` with a bounded timeout. An
`ATTEMPT_COUNTER: AtomicU64` generates monotonic attempt IDs; compare-and-remove prevents
stale cleanup from deleting newer slots.

Each init task is tracked in `active_init_tasks` with a `CancellationToken` and
`AbortHandle`. Cooperative cancellation checks occur at key stages: before download,
process spawn, `initialize` request, and `initialized` notification. This allows
`shutdown_all()` to cancel in-flight initialization cooperatively rather than only
relying on abort.

### Writer
`LspWriter` serializes all output through `Arc<Mutex<...>>`. Content-Length uses UTF-8 byte count.

### Transport State
`ClientTransportState` tracks whether the writer pipe to the server is still operational
(`Running` or `Failed { reason }`). All terminal transport failures (stdout EOF,
request write failure, notification write failure, and timeout-cancel write failure)
transition to `Failed` exactly once via the centralized `fail_transport()` helper.
Pending requests are drained on transition. Subsequent `send_request` /
`send_notification` calls return `LspError::WriterClosed` immediately.

### Shutdown Coordination
`LspService` tracks a `LifecycleState` containing both `ServiceLifecycle` phase and a
monotonic `generation: u64`. The lifecycle is broadcast on a `tokio::sync::watch` channel
(`lifecycle_tx`) so late subscribers do not lose wakeups at the `ShuttingDown → Stopped`
transition. `shutdown_all()` is quiescent: it cancels cooperative tasks via
`CancellationToken` (concurrent, not sequential), then awaits all completion receivers
concurrently via `await_init_task_completions` (using `FuturesUnordered` with `tokio::select!`
over each receiver and the aggregate deadline) under one 300ms grace period. Stragglers
are forcibly aborted via `AbortHandle` and re-awaited through the same authoritative
completion receiver path. The completion receiver is the authoritative terminal signal
— no forwarding task ever wraps the real `JoinHandle`. Ready clients are drained
concurrently via `futures::future::join_all` with a per-client timeout (2s), and
concurrent callers are notified via `await_stopped()` which subscribes to the watch
channel and waits for `Stopped`. The shutdown is driven by an absolute deadline
(`Instant::now() + SHUTDOWN_GLOBAL_TIMEOUT`), so the total shutdown is bounded by 6s
regardless of client count. A second caller observing `ShuttingDown` awaits the same
completion signal via the watch channel rather than racing independently. New client
acquisition is rejected when the lifecycle is not `Running`.

The quiescence tests now accurately distinguish cooperative cancellation paths (`cooperative_cancellation_drops_factory_future`, `cooperative_shutdown_resolves_waiters`) from forced-abort fallback paths, verifying that the `FutureExitProbe` RAII guard confirms the factory future body was actually dropped before shutdown returns.

Each spawned init task is wrapped in `run_init_task_wrapper`, which awaits a
start-registration barrier before doing any work. The barrier is a one-shot oneshot: the
leader registration code sends on `start_tx` only after the `active_init_tasks` entry has
been installed, which guarantees the task body cannot complete (or even begin) before
its bookkeeping record exists. The wrapper owns the `Sender` end of an authoritative
terminal completion channel; the corresponding `Receiver` lives in `InitTaskControl` and
is the only authoritative source of truth for "the wrapper has terminated". The wrapper
explicitly removes its `active_init_tasks` entry on the normal completion path before
sending the terminal `InitTaskExit`. The `ActiveTaskGuard` drop guard is a fallback for
panic/abort paths: its `Drop` spawns a follow-up cleanup task to remove the entry from
the map (no longer relying on `try_lock`, which silently abandoned cleanup under lock
contention). The shutdown drain is the additional safety net — it empties the map after
observing task termination via the completion receivers, so the active map is guaranteed
to be empty post-shutdown regardless of which path any individual wrapper took.

### Client-Map Lock Discipline

Non-mutating client-map access uses read guards (`clients.read().await`). Write guards
are limited to slot election/publication (init task lifecycle) and shutdown drain. No
client-map guard is held across client I/O — operations acquire the read guard, extract
an `Arc<LspClient>`, then drop the guard before performing LSP requests.

## Architecture Notes

### Client-Per-Root Pattern

`LspService` maintains a `HashMap<String, ClientEntry>` where the key is `"{project_root}:{server_id}"`. This means one LSP client per project root per language.

### Content-Length Framing

LSP messages use Content-Length headers for framing:
```
Content-Length: <bytes>\r\n\r\n<json payload>
```

### Notification Handling

Server→client notifications (like `textDocument/publishDiagnostics`) are:

1. Read by the background `_reader_task` from stdout
2. Classified via `classify_json_rpc_message` into `JsonRpcMessage::Notification`
3. Dispatched via `dispatch_notification` which updates the shared `diagnostics` map
4. Diagnostics are now updated independently of pending requests (no more "diagnostics only consumed while request is pending")

### Background Dispatcher Architecture

The background reader task is spawned during `LspClient::new()`. It:

- Continuously reads Content-Length framed JSON-RPC messages from stdout
- Classifies each message via `classify_json_rpc_message` (Response, ErrorResponse, Notification, Unknown)
- Routes responses to pending oneshot senders via the `pending` map
- Dispatches notifications via `dispatch_notification` (currently handles `textDocument/publishDiagnostics`)
- Diagnostics freshness is tracked via `last_opened_at` timestamps; the `diagnostics` operation reports `diagnostics_may_still_be_warming` when a file was recently opened or changed

Key helper functions (exported from `client.rs`):
- `classify_json_rpc_message(value) -> JsonRpcMessage`
- `dispatch_notification(diagnostics, method, params)`
- `url_to_uri(url) -> Uri`

## Quiescence Tests

The following tests in `crates/egglsp/src/service.rs` verify the quiescent shutdown behavior:

- `read_lock_concurrency` — non-mutating operations use read locks and do not contend with each other
- `second_caller_becomes_waiter_before_leader_spawn` — concurrent callers for the same key are sequenced
- `publish_before_shutdown_drains_published_client` — a published client is drained with bounded timeout even if shutdown begins after publication
- `retry_after_failure_invokes_factory_again` — a failed init allows a fresh attempt
- `shutdown_during_init_cancels_waiters_and_disposes_client` — waiters receive `Cancelled`; unpublished client is disposed
- `factory_panic_resolves_all_callers` — a panicking factory is converted to a `SharedInitError` for all waiters
- `same_key_concurrent_cold_start_invokes_factory_once` — single-flight election works under contention
- `shared_failure_is_identical_for_all_callers` — every waiter sees the same `SharedInitError`
- `concurrent_shutdown_callers` — two `shutdown_all()` calls both observe the final `Stopped` state
- `publication_race_remains_safe` — an init task that finishes after `ShuttingDown` does not publish a stale client
- `cooperative_cancellation_drops_factory_future` — cooperative cancellation works via `CancellationToken`; the factory future body is dropped before shutdown returns
- `shutdown_cancels_blocked_factory` — cooperative cancellation works via `CancellationToken`
- `normal_completion_removes_active_task_entry` — explicit cleanup path: the wrapper removes the `active_init_tasks` entry without requiring shutdown
- `ordinary_failure_removes_active_task_entry` — same, for ordinary initialization failures
- `cooperative_shutdown_resolves_waiters` — the aborted task's completion receiver is awaited; the task body actually exits before shutdown returns; the `FutureExitProbe` proves the factory future was dropped
- `concurrent_shutdown_lost_wakeup_boundary` — late subscribers to the watch channel do not miss the `ShuttingDown → Stopped` transition
- `global_deadline_finalizes_state` — a task that does not complete within the global deadline is still drained; lifecycle reaches `Stopped` and all maps are empty
- `fast_completion_cannot_beat_registration` — the start-registration barrier prevents a fast-completing task from racing past the `active_init_tasks` insertion; run repeatedly in a bounded loop
- `cooperative_cancellation_is_observed` — the factory future body is dropped (RAII probe increments) before shutdown returns; the `InitTaskExit` resolution is observed via the authoritative receiver
- `many_tasks_share_one_grace_period` — the aggregate grace wait in `await_init_task_completions` is applied across all in-flight tasks; total shutdown time is bounded by one grace period
- `no_stale_active_entries_under_contention` — concurrent fast success attempts leave `active_init_tasks` empty without requiring shutdown
- `lock_order_no_deadlock_under_overlap` — concurrent registration and shutdown overlap via test gates; neither path deadlocks
- `global_deadline_fallback_asserts_all_signals` — a stuck factory is forcibly aborted, all maps are drained, and the lifecycle is `Stopped` — all within the global deadline
- `aggregate_grace_across_independent_tasks` — the aggregate grace wait in `await_init_task_completions` is applied across independent in-flight tasks; total shutdown time is bounded by one grace period regardless of task count
- `deadline_fallback_with_unresolvable_completion` — when a completion receiver never resolves, the global deadline forces finalization; lifecycle reaches `Stopped` and all maps are empty
- `forced_abort_after_grace_period` — genuinely reaches the abort-after-grace path: a factory that blocks indefinitely triggers the forced-abort fallback after the 300ms grace period expires; verifies the `AbortHandle` path works end-to-end

## Phase 2: Scripted Stdio Integration Tests

The `egglsp` package now owns the phase 2 stdio integration-test surface under `crates/egglsp/tests/`. The fake LSP server binary is built as a `[[bin]]` target from the `egglsp` package; root tests use `codegg-lsp-test-server` (via `CARGO_BIN_EXE_codegg-lsp-test-server`), while `egglsp`-only tests use `egglsp-test-server` (via `CARGO_BIN_EXE_egglsp-test-server`), with `EGGLSP_TEST_SERVER` as an override for CI or manual runs. The scenario engine lives in `egglsp::test_support` module (feature-gated behind `lsp-test-support` and `#[doc(hidden)]`); both binary wrappers are thin `main()` functions.

Phase 2 is complete. The production-harness integration tests cover 11 protocol tests, 3 semantic tests, and 5 service tests through real stdio transport, plus 24 root-crate composite tests in `tests/lsp_composite_stdio.rs` that bridge the gap between `egglsp`-only tests and the real root-crate collectors (`SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`). The crate unit tests (including `forced_abort_after_grace_period` which genuinely reaches the abort-after-grace path) also contribute coverage. Tests live in `tests/production_protocol_stdio.rs`, `tests/production_semantic_stdio.rs`, `tests/production_service_stdio.rs`, and `tests/scenario_engine.rs` includes the fake-server self-tests for strict allow-listing, raw bytes, and grouped-frame fixtures. The previously flaky transport test has been fixed.

The fake server supports **captured-ID mode** for genuinely out-of-order concurrent responses, enabling deterministic testing of concurrent request handling. All integration tests use **bounded condition waits** (polling loops) instead of fixed sleeps. `LspClient` has **typed hierarchy methods** (`prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`, `prepare_type_hierarchy`, `supertypes`, `subtypes`) that replace manual JSON-RPC dispatch.

### Test Infrastructure

- **Fake server binary**: `crates/egglsp-test-server/src/main.rs` (thin wrapper calling `egglsp::test_support::run_or_exit()`; built as `egglsp-test-server` for `egglsp` tests and `codegg-lsp-test-server` for root tests) — reads Content-Length framed JSON-RPC, executes scripted scenarios
- **Production harness**: `tests/common/production_harness.rs` — launches the same binary against a minimal real-project root for launcher-path coverage
- **Scenario format**: JSON files with step types (ExpectRequest, ExpectNotification, AllowRequest, AllowNotification, SendNotification, Delay, ExitNow)
- **Transcript**: Machine-readable JSONL output for failure diagnostics
- **Harness**: `tests/common/harness.rs` — temp directories, scenario management, `CARGO_BIN_EXE_codegg-lsp-test-server` discovery with `EGGLSP_TEST_SERVER` override
- **Fake-server self-tests**: `tests/scenario_engine.rs` — inlined fake-server self-tests for strict allow-listing, raw bytes, and grouped-frame fixtures (no longer `include!` from outside the package)

### Production Protocol Tests (`tests/production_protocol_stdio.rs`)

| Test | Coverage |
|------|----------|
| `initialization_handshake` | Full init/initialized/shutdown/exit lifecycle |
| `server_requests_during_init_and_dynamic_registration` | workspace/configuration during initialization + registration |
| `apply_edit_refusal_keeps_client_usable` | workspace/applyEdit rejection |
| `concurrent_out_of_order_responses_and_notifications` | Multiple requests, reversed responses |
| `request_timeout_and_late_response_are_dropped` | Production $/cancelRequest emission |
| `malformed_frames_fail_transport` | 8 malformed framing cases → transport failure |
| `unknown_json_rpc_frames_are_ignored` | Unknown frames don't break transport |
| `grouped_frames_and_split_writes_are_processed` | Multiple frames in one write + split body |
| `diagnostics_lifecycle_tracks_file_changes` | publishDiagnostics around didOpen/didChange/didSave/didClose |
| `server_exit_before_response_and_error_response` | Server exit + error response handling |
| `error_response_is_reported` | JSON-RPC error response handling |

### Production Semantic Tests (`tests/production_semantic_stdio.rs`)

| Test | Coverage |
|------|----------|
| `typed_semantic_requests_collect_context_and_freshness` | Hover, definition, references, symbols, completion, code actions, semantic context, security context |
| `edit_round_trips_do_not_mutate_disk` | Rename, formatting, code action previews |
| `hierarchy_context_requests_round_trip_through_real_client` | Call hierarchy, type hierarchy |

### Production Service Tests (`tests/production_service_stdio.rs`)

| Test | Coverage |
|------|----------|
| `single_flight_init_uses_a_real_child` | Same-key concurrent init launches one child |
| `document_lifecycle_ownership_tracks_open_update_save_close` | Document ownership routing |
| `diagnostics_propagate_through_service_apis` | Diagnostics retrieval through service APIs |
| `shutdown_during_delayed_init_cancels_waiters` | Delayed init shutdown cancellation |
| `shutdown_with_inflight_request_completes_bounded` | In-flight request shutdown bounded |

### Root Composite Tests (`tests/lsp_composite_stdio.rs`)

These tests exercise root-crate collectors against the fake LSP server via the production `LspClient`/`LspService` stack. They bridge the gap between `egglsp`-only tests and the real collectors.

Preview tests are classified into two categories:
- **Child-process production-chain**: fake server → LspClient → typed response → preview conversion (rename, format, source-action, out-of-root, overlapping)
- **Local production-function**: directly exercises production selection/conversion functions with locally constructed typed values (command-only, no-edit, ambiguous, resource-operation)

| Test | Coverage |
|------|----------|
| `composite_harness_initialization_smoke` | Composite harness initialization end-to-end |
| `composite_service_layer_construction` | Service layer construction from composite harness |
| `composite_document_symbols_via_direct_client` | Document symbols through direct client path |
| `composite_semantic_context_collector_construction` | `SemanticContextCollector` construction and wiring |
| `rename_preview_converts_through_production_path` | Rename preview — child-process production-chain (fake server → LspClient → typed response → preview conversion) |
| `format_preview_converts_through_production_path` | Format preview — child-process production-chain |
| `code_action_source_action_preview_converts_through_production_path` | Source-action preview — child-process production-chain |
| `preview_safety_out_of_root_rejected` | Out-of-root path rejection — child-process production-chain |
| `preview_safety_overlapping_edits_rejected` | Overlapping edit rejection — child-process production-chain |
| `preview_safety_command_only_code_action_rejected` | Command-only code action rejection — local production-function (directly exercises production selection/conversion with locally constructed typed values) |
| `preview_safety_no_edit_code_action_rejected` | No-edit code action rejection — local production-function |
| `preview_safety_ambiguous_source_actions_rejected` | Ambiguous source action rejection — local production-function |
| `semantic_context_collector_exercises_real_workflow` | Full `SemanticContextCollector` workflow (source excerpt, diagnostics, symbols, definitions, references) |
| `semantic_context_collector_capability_gating` | Capability-gated degradation when server lacks a capability |
| `semantic_context_collector_failure_degradation` | Graceful degradation when optional operations error |
| `semantic_context_security_review_intent_collects_security_source` | Security review intent on security-sensitive source (renamed from `security_context_workflow_uses_semantic_collector`) |
| `security_context_tool_exercises_risk_filtering_and_call_expansion` | Real `LspTool::execute("securityContext")` orchestration with risk markers, call expansion, and cycle suppression |
| `security_context_tool_degrades_on_call_hierarchy_error` | Graceful degradation when outgoingCalls fails during expansion BFS — error recorded, packet returned, nodes/evidence preserved |
| `security_context_tool_enforces_call_node_limit_and_truncation` | `max_call_nodes` enforced, BFS depth limit proven, truncation flags set |
| `security_context_tool_filters_and_preserves_diagnostic_evidence` | Security-relevant diagnostic survives filtering, diagnostic_evidence populated |
| `semantic_context_minimal_service_client` | Minimal service-client construction |
| `preview_safety_resource_operation_rejected` | Resource-operation code action rejection — local production-function |
| `hunk_source_context_collector_exercises_real_workflow` | Hunk source context collector real workflow with unified diff |

### Running

```bash
# Run Phase 2 integration tests (parallel-safe, require lsp-test-support feature)
cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio
cargo test -p egglsp --features lsp-test-support --test production_semantic_stdio
cargo test -p egglsp --features lsp-test-support --test production_service_stdio
cargo test -p egglsp --features lsp-test-support --test scenario_engine

# Run root composite tests (semantic/security/hunk collectors + preview safety)
cargo test --features lsp-test-support --test lsp_composite_stdio

# Run unit tests
cargo test -p egglsp --lib

# Force single-threaded to validate sequential stability
cargo test -p egglsp --features lsp-test-support --tests -- --test-threads=1
```

## Phase 3: Real-Server Compatibility & Resilience

Phase 3 adds real-server compatibility testing, operational health tracking, process supervision, and document replay for crash recovery.

### New Modules (crates/egglsp/src/)

| Module | Purpose |
|--------|---------|
| `compatibility.rs` | Per-server compatibility profiles (`LspCompatibilityProfile`), readiness policies (`LspReadinessPolicy`), restart policies (`LspRestartPolicy`, `LspRestartMode`), version detection (`LspServerVersion`), compatibility reports (`LspCompatibilityReport`, `CompatibilityCheckStatus`), tier-1 profiles, and binary requirement checks |
| `health.rs` | Operational state machine (`LspOperationalState`: Starting → Initializing → Indexing → Ready → Degraded → RestartScheduled → Restarting → Failed → Stopping → Stopped), invalid transition detection (`InvalidTransition`), and health snapshots (`LspOperationalHealthSnapshot`) |
| `supervisor.rs` | Process exit event tracking (`LspProcessExitEvent`) and stderr ring buffering (`StderrRingBuffer`, 100 lines / 64KB cap) |
| `document_sync.rs` | Open document registry (`OpenDocumentRegistry`) and document snapshots (`OpenDocumentSnapshot`) for replaying `didOpen` notifications after server restart |

### New Feature Flag

```toml
[features]
lsp-real-server-tests = []  # separate from lsp-test-support
```

### Real-Server Smoke Tests

```bash
# Run real-server smoke tests (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke

# Run with specific server binaries on PATH
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust-analyzer
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- pyright
```

The smoke tests (`crates/egglsp/tests/real_server_smoke.rs`) exercise rust-analyzer and pyright/basedpyright against the production launcher, frame parser, and request routing. They are slow (200ms-2s startup plus indexing), non-hermetic (require installed binaries), and expensive in CI, so they are gated behind the `lsp-real-server-tests` feature.

### Target Compatibility Matrix

| Server | Language | Key Operations | Known Limitations |
|--------|----------|----------------|-------------------|
| **rust-analyzer** | Rust | hover, definition, references, symbols, call hierarchy, rename, code actions, semanticContext, securityContext, hunkSourceContext | Initial indexing may be slow on large workspaces; diagnostics may need warm-up delay |
| **pyright** | Python | hover, definition, references, symbols, rename | No `prepareCallHierarchy` (Python doesn't have function-level call hierarchy); `codeAction` limited to organize imports |
| **typescript-language-server** | TypeScript / JavaScript | hover, definition, references, symbols, rename, code actions | `prepareCallHierarchy` may be empty; large workspaces slow |
| **gopls** | Go | hover, definition, references, symbols, rename, code actions | Call hierarchy not yet supported by gopls; securityContext will degrade gracefully |
| **clangd** | C / C++ | hover, definition, references, symbols, rename, code actions | No call hierarchy; slow on large TUs |

## Phase 3 Corrective Pass — Status

The Phase 3 corrective pass is **complete and closed** as of the 11-pass final closure. The earlier seven passes established the structural scaffolding (compatibility profiles, health state machine, runtime owner, restart coordinator, document replay). The final closure turns that scaffolding into an operationally trustworthy lifecycle:

- **Final closure Pass 1 — Generation-aware runtime map**: `runtime_map` is now `HashMap<String, RuntimeEntry>` where `RuntimeEntry { generation: u64, runtime: LspProcessRuntime }`. Insertion, lookup, and removal go through `install_runtime`, `runtime_for_generation`, and `remove_runtime_if_generation` — all of which check the stored generation. A delayed old monitor cannot remove a newer generation's runtime. The unit tests `old_monitor_cannot_remove_new_runtime` and `runtime_removal_requires_exact_generation` lock the invariant down.
- **Final closure Pass 2 — Runtime intent + wait + kill + reap in `shutdown_all()`**: `LspClient::shutdown()` is split from the runtime-termination helper. The client method sends only `shutdown` / `exit` (now exposed as `request_protocol_shutdown`) and never waits on the child. The service helper `terminate_runtime(key, generation, client, graceful_deadline, absolute_deadline, reason)` runs the bounded sequence: intent → protocol shutdown → wait → force-kill → reap. `RuntimeTerminationReason` is `ServiceShutdown` / `ManualRestart` / `FailedPublication`. Hung processes are killed and reaped under the global deadline; the runtime map is empty after `shutdown_all()`.
- **Final closure Pass 3 — Single generation owner**: `LspService::next_generation_for_key(key) -> u64` is the single source of truth for replacement generation. The reinit closure receives the generation as an argument (`FnMut(&LspClientDescriptor, u64) -> BoxFuture<...>`) and must not compute generation independently. The coordinator calls `next_generation_for_key` exactly once per restart.
- **Final closure Pass 4 — Manual restart terminates old runtime**: `LspService::manual_restart_client(key)` is the public manual-restart API. It always runs (bypasses `LspRestartMode::Disabled`), terminates the old runtime with `RuntimeTerminationReason::ManualRestart` first, then starts the replacement. A manual restart issued while an automatic restart is in progress supersedes it.
- **Final closure Pass 5 — Shared crash-cycle restart budget**: The `restart_attempts` counter is shared across rapid crash cycles. `LspService::increment_restart_attempts(key)` is called once per actual replacement spawn. `LspService::set_last_healthy_now(key)` records the timestamp when readiness reaches `Ready`. `LspService::reset_restart_attempts_if_healthy_inherent(key, reset_after_healthy)` lazily resets the counter to 0 when the next unexpected exit observes a healthy interval. When `restart_attempts >= max_attempts` no new process is launched and the operational state transitions to `Failed`.
- **Final closure Pass 6 — Retained stale diagnostics + `post_restart` correction**: `LspService::snapshot_diagnostics_for_restart(key)` captures the live diagnostic cache for the old client. `LspClient::install_retained_diagnostics("restart", retained)` installs the snapshot in the new client, preserving the old `server_generation` and `post_restart` flags. The freshness classifier returns `Stale` because `entry.server_generation != current_generation`. A new `publishDiagnostics` from the new generation (including an empty vector) overwrites retained entries. `post_restart = generation > 1` is now enforced uniformly in `LspClient::bind_server_generation` and `DiagnosticCacheEntry::with_generation`; generation 1 is never `post_restart`, generation 2+ always is.
- **Final closure Pass 7 — Observed progress readiness**: `LspClient::wait_for_progress_end(timeout) -> bool` requires `ProgressState.completed_cycle == true`. Empty `active_tokens` alone is not sufficient — `wait_for_progress_end` returns `false` until a full `begin`/`end` cycle is observed. The real-server smoke harness calls `client.wait_for_progress_end(*timeout)` instead of fixed sleeps.
- **Final closure Pass 8 — Validated restart config + descriptor parity**: `LspRestartPolicyConfig::try_to_domain(&self, base: &LspRestartPolicy) -> Result<LspRestartPolicy, LspError>` validates user overrides. It rejects `OnUnexpectedExit` with `max_attempts == 0`, rejects `initial_backoff_ms > max_backoff_ms`, and rejects duration overflow. `LspRestartPolicyConfig::merge_with_profile` copies non-`None` fields from the profile. Cold start and restart consume the same persisted `LspClientDescriptor` — they receive identical `launch_spec`, `initialization_options`, `workspace_configuration`, `readiness_policy`, and `restart_policy`. The test `cold_start_and_restart_receive_identical_configuration` asserts generation 1 and generation 2 match exactly.
- **Final closure Pass 9 — Real-server stderr capture**: `LspProcessRuntime::stderr_tail_capped(max_lines) -> Vec<String>` returns the most recent lines from the bounded `StderrRingBuffer` (100 lines / 64KB cap) in chronological order. The real-server smoke harness attaches an `LspProcessRuntime` to each smoke client, takes the child and stderr at construction, and on protocol shutdown calls `runtime.request_graceful_shutdown()` + `client.request_protocol_shutdown()` + `runtime.wait_for_exit()` with a force-kill fallback. `LspCompatibilityReport.stderr_tail` is populated from this accessor. Zero-length `findReferences` results fail the `RequiredIfAdvertised` check for the rust fixture; the Python cross-file fixture continues requiring at least two distinct URIs.
- **Final closure Pass 10 — Supervised constructor invariant**: `LspService::new(config)` is the bare test-only constructor — it returns `Self` without the cyclic back-reference wired. `LspService::new_arc(config) -> Arc<Self>` is the only public production constructor; it builds the service via `Arc::new_cyclic(|weak| Self { ..., self_ref: OnceLock::from(weak.clone()), ... })`. The test `new_arc_wires_self_ref` proves the production constructor populates `self_ref`. No public production path creates an un-supervised service.
- **Final closure Pass 11 — Test timing fix**: `generation_is_identical_across_health_and_exit_event` previously overwrote the generation-3 scenario before generation 2 started, causing the gen-2 process to read the gen-3 scenario. The test now writes the gen-3 scenario only after `service.generation_for_key(&key) >= 2` is observed, and the gen-2 process is verified `Ready` before the gen-3 file is staged. The supervisor/restart test surface is now **11** deterministic scripted scenarios: graceful shutdown, unexpected exit + restart disabled, automatic restart success, init failure then recovery, exhaustion, shutdown cancels scheduled restart, stale exit event, replay uses latest content, hung process force kill, two consecutive restarts use monotonic generations, and generation is identical across health and exit event.

### Earlier Phase 3 Passes (still applicable)

- **Pass 1 — Real-server harness correctness**: `crates/egglsp/tests/real_server_smoke.rs` does a full `initialize` + `send_initialized` handshake, uses typed `RealServerFixture` metadata with `rust_fixture()` and `python_fixture()` constructors, queries only source files at exact positions, and classifies checks by `CompatibilityRequirement`. The new `assert_required_checks(report)` helper fails the test on `Required` regressions.
- **Pass 2 — Supervisor process ownership**: `LspProcessRuntime` (in `runtime.rs`) is the single authoritative process owner; it owns the child handle, stderr ring buffer, intent receiver, and kill channel. `LspService::new_arc(config)` wires the back-reference via `Arc::new_cyclic`; `ensure_exit_receiver_started` auto-activates on the first client-creating call.
- **Pass 3 — Generation and operational health**: `generation_map: Arc<Mutex<HashMap<String, u64>>>` provides per-key generation. `LspOperationalHealthSnapshot` is constructible without a live client and carries `transport: Option<...>`, `last_error`, `stderr_tail`, real `last_message_age_ms` / `last_diagnostics_age_ms`, and `restart_attempts`. All state transitions go through `transition_operational_state()` which uses the `health::transition()` validator.
- **Pass 4 — Restart descriptor and coordinator**: `LspClientDescriptor` persists the per-client launch spec; `LspClientDescriptor::from_profile` resolves readiness/restart policies with explicit `user > profile > server-definition` priority. `restart_client_coordinator<S, F>` is the single source of truth for retry/backoff/exhaustion/cancellation. `LspServiceClone` was removed and the duplicate `restart_client` paths were merged.
- **Pass 5 — Document replay and diagnostic freshness**: replay preserves the snapshot's per-document version; replay failure transitions to `Degraded` instead of silent `Ready`. `DiagnosticCacheEntry` carries `server_generation: u64` and `post_restart: bool`; on restart `mark_diagnostics_stale_for_key` rewrites retained entries to `current - 1` so they classify as `Stale`. `LspDiagnosticSnapshot` exposes both fields; the root `SemanticContextCollector` propagates them to `SemanticDiagnosticEvidence`.
- **Pass 6 — Readiness and workflow adoption**: `LspClient` tracks `ProgressState` (active `$/progress` tokens + last progress timestamp) and exposes `progress_snapshot`, `wait_for_progress_end`, `wait_for_first_diagnostics`, `operational_summary`. `LspService::wait_for_readiness(key, policy)` honors all four `LspReadinessPolicy` variants and returns `ReadinessResult::Ready { elapsed }` or `Degraded { reason, elapsed }`. `LspOperationalState::context_note()` is appended to `SemanticContextResponse.notes`, `SecurityContextPacket.notes`, and hunk source context summary lines.
- **Pass 7 — CI and docs**: `.github/workflows/lsp-real-server.yml` pins `rust-toolchain@1.81.0` (rust-analyzer job) and `basedpyright@1.13.1`; each matrix job runs only its own server test (`-- rust_analyzer` or `-- basedpyright`); artifact filenames are sanitized.

## See Also

- [tool.md](tool.md) - LSP tool wrapper
- [architecture/lsp.md](../../architecture/lsp.md) - Architecture documentation
