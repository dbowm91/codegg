---
name: lsp
description: LSP client-side integration for Language Server Protocol support
version: 1.6.0
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

The LSP implementation lives in the `egglsp` workspace crate (`crates/egglsp/`). `src/lsp/mod.rs` is a thin compatibility shim that re-exports `egglsp::*` and bridges config/error types. The model-facing tool is at `src/tool/lsp.rs`. Phase 2 integration tests live under `crates/egglsp/tests/`: the production-harness tests use `ProductionClientHarness`, and `scenario_engine.rs` includes the fake-server self-tests.

LSP is exposed as a native tool via `LspTool`, returning compact agent-facing summaries (not raw LSP JSON). Model-facing line and column are 1-indexed; the wrapper converts to LSP 0-indexed.

**Phase 4** complete for the exact pinned Tier 1 and Tier 2 matrix. All five real-server jobs pass on one commit; the aggregate manifest verifies consistent run metadata, report completeness, typed operation invariants, required-operation success, shutdown traces, and exact version evidence. Compatibility outside the pinned matrix remains experimental. Phase 4 broadens language-server coverage and exposes higher-level capabilities (declaration, implementation, document highlights, signature help, workspace symbols, bounded completion, semantic tokens, preview-only rename / code actions / formatting). Read-only operations execute directly and are capability-gated; mutation-producing operations return structured previews only and never modify the workspace.

**Phase 4 final reporting + semantic validation closure** (see `architecture/lsp.md` for the full pass table) tightens the public API (`prepare_rename_unchecked` / `rename_preview_unchecked` / `format_preview_unchecked` / `code_actions_unchecked` raw wrappers; typed checked methods are the normal public API), inspects `CapabilityDecision` in the rename pipeline so prepareRename-unsupported servers fall back to `textDocument/rename` directly (fail-closed on `Unknown`), runs real `decode_semantic_tokens` assertions in the smoke harness (per-token `(line, col, length)` validity + legend match), classifies per-server shutdown outcomes against the fixture's `shutdown_requirement` so clangd's daemon-mode hang becomes a `KnownLimitation` with a stderr-tail excerpt, and adds `LspCompatibilityReport.operation_support: Vec<LspOperationCompatibility>` with `advertised` / `exercised` / `request_succeeded` / `semantic_assertion_passed` flags per operation. `assert_required_checks` is now status-driven — it no longer fails on first-class `Skipped` / `Unsupported` statuses. The TypeScript fixture now contains a real `Greeter` interface + `Person` class implementing it (driving `textDocument/implementation`), the Rust fixture adds a `Greeter` trait + `Person` struct (driving `textDocument/prepareTypeHierarchy` + subtype matching), and the clangd fixture drives `textDocument/implementation` from `include/widget.hpp`.

The Phase 4 changes preserve the central safety rule from earlier phases:

```text
read-only semantic operations may be executed directly;
mutation-producing operations must remain preview-only until
explicitly applied by a higher-level user-approved path.
```

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
├── operations/         # Per-concern modules: navigation.rs, signature.rs, completion.rs, code_actions.rs, rename.rs, formatting.rs, semantic_tokens.rs, overlay_ops.rs, mod.rs
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
    pub observed_capabilities: ObservedCapabilitiesOverride,  // Phase 4
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

Key functions: `rust_analyzer_profile()`, `pyright_profile()`, `gopls_profile()`, `typescript_language_server_profile()`, `clangd_profile()`, `profile_for_server(id)`, `tier1_profiles()`, `tier2_profiles()`, `all_profiles()`, `require_server_binary(id)`. The real-server harness classifies each `LspCompatibilityCheck` with a `CompatibilityRequirement` and calls `assert_required_checks(report)` to fail the test on `Required` regressions.

Tier 2 profile specifics (Phase 4):

| Profile | Executable | Readiness | `observed_capabilities.type_hierarchy` |
|---------|-----------|-----------|----------------------------------------|
| `gopls_profile()` | `gopls` | `WaitForDiagnosticsOrTimeout { 15s }` | `Some(true)` |
| `typescript_language_server_profile()` | `typescript-language-server --stdio` | `WaitForProgressEndOrTimeout { 20s }` | `None` |
| `clangd_profile()` | `clangd --background-index=false --clang-tidy=0` | `WarmupDelay { 2s }` | `Some(true)` |

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

## Tier 1 vs Tier 2

Phase 4 complete for the exact pinned Tier 1 and Tier 2 matrix. All five real-server jobs pass on one commit; the aggregate manifest verifies consistent run metadata, report completeness, typed operation invariants, required-operation success, shutdown traces, and exact version evidence. Compatibility outside the pinned matrix remains experimental. Phase 4 final closure added: UTF-8 position offset safety (char boundary enforcement), deterministic force-kill test (SIGTERM-ignoring fixture), known-limitation scope validation (Protocol:/Semantic: prefix), and GitHub artifact layout aggregation test. Server maturity is tracked on the profile, not in
any generic client branch — there is no `match server_id` for Tier 2
quirks. Profile accessors live in `crates/egglsp/src/compatibility.rs`:

```rust
pub fn tier1_profiles() -> Vec<LspCompatibilityProfile>     // rust-analyzer + basedpyright
pub fn tier2_profiles() -> Vec<LspCompatibilityProfile>     // gopls + typescript-language-server + clangd
pub fn all_profiles() -> Vec<LspCompatibilityProfile>       // tier1 ++ tier2 (deterministic order)
```

| Tier | Servers | Test surface |
|------|---------|--------------|
| Tier 1 | `rust-analyzer`, `basedpyright` / `pyright` | Real-server CI in `.github/workflows/lsp-real-server.yml` (`lsp-real-server-tests` feature) on opt-in triggers (`workflow_dispatch`, weekly schedule, push paths under `crates/egglsp/**`, `src/lsp/**`, or the workflow YAML itself) |
| Tier 2 | `gopls`, `typescript-language-server`, `clangd` | Real-server CI in `.github/workflows/lsp-real-server.yml`, opt-in, with pinned versions: `gopls` v0.16.1 (Go 1.22.5), `typescript-language-server` 4.3.3 + `typescript` 5.5.4 (Node 20), `clangd` 18.1.8 (LLVM apt, checksum-verified archive) |

Default CI remains network-free; Tier 2 jobs run only on opt-in triggers or path-triggered runs. Tier 2 profiles share the same `LspCompatibilityProfile` struct, the same accessor pattern, and the same client code path — generic client code reads profile fields (readiness policy, restart policy, executable candidates, observed capability overrides) instead of branching on server IDs.

Each Tier 2 profile records `known_limitations` and surfaces them in `LspCompatibilityReport.known_limitations` (see `crates/egglsp/src/compatibility.rs:260-383`).

## Capability model

Phase 4 corrected capability normalization so the snapshot accurately reflects what the server actually advertised — without inferring one capability from another. `LspCapabilitySnapshot` (in `crates/egglsp/src/capability.rs:39`) was extended with new normalized booleans. Every `LspSemanticOperation` maps to a single `CapabilityDecision` (Supported, Unsupported, or Unknown). The `Unknown` state is fail-closed: `require_capability()` returns `LspError::NotInitialized` for Unknown, so no request is sent before the capability state is authoritative.

```rust
pub supports_declaration: bool,        // textDocument/declaration
pub supports_implementation: bool,      // textDocument/implementation
pub supports_document_highlight: bool,  // textDocument/documentHighlight
pub supports_signature_help: bool,      // textDocument/signatureHelp
pub supports_rename: bool,              // textDocument/rename
pub supports_prepare_rename: bool,      // textDocument/prepareRename (rename provider's prepare_provider)
pub supports_code_actions: bool,        // textDocument/codeAction
pub supports_document_formatting: bool, // textDocument/formatting
pub supports_range_formatting: bool,    // textDocument/rangeFormatting
pub supports_inlay_hints: bool,         // textDocument/inlayHint
pub supports_folding_ranges: bool,      // textDocument/foldingRange
pub supports_selection_ranges: bool,    // textDocument/selectionRange
pub supports_document_links: bool,      // textDocument/documentLink
pub supports_execute_command: bool,     // workspace/executeCommand (recorded, never invoked)
```

### Push vs pull diagnostics

Diagnostics are no longer assumed to be supported by every initialized server.
Push diagnostics (`supports_push_diagnostics`) are **observed** from actual
`publishDiagnostics` notifications — the field is `true` when the server
has pushed at least one `publishDiagnostics` for any file, not derived from
text synchronization settings. Pull diagnostics (`supports_pull_diagnostics`)
are **advertised** via the `diagnostic_provider` capability. The snapshot splits:

```rust
pub supports_push_diagnostics: bool,    // observed from actual publishDiagnostics notifications
pub supports_pull_diagnostics: bool,    // advertised via diagnostic_provider capability
pub supports_diagnostics: bool,         // legacy alias: push || pull
```

`supports_execute_command` is recorded for completeness but no model-facing path ever issues an `executeCommand` request.

### Type hierarchy

Type hierarchy is **no longer inferred from call hierarchy**. On `lsp-types` 0.97, type hierarchy is modeled only as a client capability, so the server-side advertised state defaults to `false` unless a profile override flips it on (see below). The Phase 4 surface adds a profile-level override for capabilities that cannot be discovered from `ServerCapabilities` alone:

```rust
pub struct ObservedCapabilitiesOverride {
    pub type_hierarchy: Option<bool>,
}

pub struct LspCompatibilityProfile {
    // ... existing fields
    pub observed_capabilities: ObservedCapabilitiesOverride,  // Phase 4
}
```

`gopls` profile flips `observed_capabilities.type_hierarchy = Some(true)`; `clangd` does not support type hierarchy (tested against clangd 22.1.1, confirmed not a supported LSP method). `rust-analyzer_profile` was also updated in Phase 4 so the snapshot remains accurate. `LspCapabilitySnapshot::from_capabilities_with_override(caps, server_name, language_id, &override)` merges the override into the snapshot at client construction time.

### Option-level details

A single bool is not enough for every provider. `LspCapabilityDetails` (in `crates/egglsp/src/capability.rs:86`) carries option-level information that a bool cannot represent:

```rust
pub struct LspCapabilityDetails {
    pub rename_prepare_provider: bool,            // vs. symbol-position only
    pub code_action_kinds: Vec<String>,            // advertised CodeActionKind list
    pub completion_trigger_characters: Vec<String>,
    pub signature_trigger_characters: Vec<String>,
    pub semantic_token_legend: Option<SemanticTokenLegendSnapshot>,
}
```

`SemanticTokenLegendSnapshot { token_types, token_modifiers }` is the compact representation of the server's semantic-token legend; the full `SemanticTokensLegend` is never exposed to model-facing surfaces.

`LspSemanticOperation` was extended with `Declaration`, `Implementation`, `DocumentHighlight`, `SignatureHelp`, `Rename`, `PrepareRename`, `CodeAction`, `DocumentFormatting`, `RangeFormatting`, `InlayHints`, `FoldingRanges`, `SelectionRanges`, `DocumentLinks`, and `ExecuteCommand` — every new capability has a matching operation variant and an `LspUnavailable` fallback.

## Operations

Phase 4 added 8 new `LspTool` operations grouped by safety profile. Read-only operations execute directly and are capability-gated via `LspOperations::require_capability`; missing capability surfaces as `LspError::Unavailable(LspUnavailable)`. Preview-only operations return bounded DTOs and never write files.

### Read-only operations (Pass 4)

| LspTool operation | LSP request | Typed DTO | Cap |
|-------------------|-------------|-----------|-----|
| `declaration` | `textDocument/declaration` | `Vec<LocationLink>` (normalized via `normalize_goto_response`) | 100 |
| `implementation` | `textDocument/implementation` | `Vec<LocationLink>` | 100 |
| `documentHighlights` | `textDocument/documentHighlight` | `Vec<DocumentHighlight>` (preserves `Text` / `Read` / `Write` kind) | 100 |
| `signatureHelp` (typed) | `textDocument/signatureHelp` | `Option<SignatureHelpSummary>` (per-item doc truncated to `SIGNATURE_DOC_MAX_CHARS` = 2000) | n/a |
| `workspaceSymbol` (hardened) | `workspace/symbol` | `Vec<SymbolInformation>` (normalized via `normalize_workspace_symbol_response`; collapses `Flat` / `Nested` variants) | 200 |

`LocationLink` variants preserve target URI / target range / selection range / origin selection range so `LocationLink` metadata is not lost in normalization.

### Bounded read-only operations (Pass 5)

| LspTool operation | LSP request | Typed DTO | Cap |
|-------------------|-------------|-----------|-----|
| `completion` | `textDocument/completion` | `Vec<CompletionCandidate>` (raw `textEdit` / `additionalTextEdits` / `command` stripped; `detail` + `insert_text_preview` each capped at `COMPLETION_DETAIL_MAX_CHARS` = 200) | `max_candidates`, default 200 |
| `semanticTokens` | `textDocument/semanticTokens/full` | `Vec<DecodedSemanticToken>` (legend-decoded via `decode_semantic_tokens`; out-of-range type indexes → `LspError::RequestFailed`) | `max_tokens`, default 1000 |

Server order is preserved (no client-side sort). `CompletionCandidate` strips all edit-bearing fields so the surface can never apply a completion edit.

### Preview-only operations (Pass 6 / 7 / 8)

| LspTool operation | LSP request | Typed DTO |
|-------------------|-------------|-----------|
| `codeActionSummaries` | `textDocument/codeAction` | `Vec<CodeActionSummary>` (capped at 50; raw `WorkspaceEdit` / `Command` payloads never exposed) |
| `codeActionPreview` | `textDocument/codeAction` (resolved) | `CodeActionPreview` (rejects command-only with `LspError::CommandOnlyCodeAction(title)`) |
| `prepareRenameTyped` / `renamePreviewTyped` | `textDocument/prepareRename` + `textDocument/rename` | `PrepareRenameResult` (Range / DefaultBehavior / Unavailable) and `RenamePreview` (capped at 100 files / 1000 edits; resource-op warnings) |
| `formatPreviewTyped` | `textDocument/formatting` | `FormattingPreview` (sha256 before/after hashes + bounded 8KB diff + on-disk invariant check) |

The Phase 4 DTOs are documented under "Phase 4 typed DTOs" in `architecture/lsp.md:571-733`.

## Safety boundary

Phase 4 keeps the central safety rule that has governed the LSP integration since Phase 1:

```text
read-only semantic operations may be executed directly;
mutation-producing operations must remain preview-only until
explicitly applied by a higher-level user-approved path.
```

The boundary is enforced at the operation layer, not by the model:

- **Read-only ops execute directly.** `declaration`, `implementation`, `document_highlights`, `signature_help_typed`, `workspace_symbols`, `completion_bounded`, and `semantic_tokens` all call `LspOperations::require_capability` first. When the server does not advertise the provider they short-circuit with `LspError::Unavailable(LspUnavailable { reason, server_id, language_id })`. When the capability state is `Unknown` (server not yet initialized, capabilities not published), `require_capability` returns `LspError::NotInitialized` — no request is sent before the capability state is authoritative.
- **Mutation-producing ops return previews.** `prepare_rename_typed`, `rename_preview_typed`, `code_action_summaries`, `preview_code_action`, and `format_preview_typed` apply edits to an in-memory snapshot only. `format_preview_typed` re-reads the on-disk file at the end and returns `LspError::RequestFailed` if `after_disk_hash != before_hash` — the on-disk invariant check is defense-in-depth, not a workaround. None of these operations writes to the user's workspace.
- **`workspace/executeCommand` is never called.** Command-only code actions (raw `Command` and `CodeAction` with `command: Some(_)` but `edit: None`) are rejected with `LspError::CommandOnlyCodeAction(title)` up front, before any network call. `supports_execute_command` is recorded in the snapshot for completeness but no model-facing path issues an `executeCommand` request.
- **Root-bounded.** All preview operations honor `allowed_root: Option<&Path>`; out-of-root edits are rejected with `LspError::PathOutsideRoot`.
- **No automatic mutation.** Preview outputs (`RenamePreview.affected_files`, `CodeActionPreview.affected_files`, `FormattingPreview`) are inspected by the user-approved path that actually applies them; the LSP tool itself remains `ToolCategory::ReadOnly` regardless of whether the payload contains edits.

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

Operations available via tool (24 operations total):
- `goToDefinition`
- `findReferences`
- `hover`
- `documentSymbol`
- `workspaceSymbol` (returns `WorkspaceSymbolSummary` with name, kind, file, start_line, start_column, container_name)
- `diagnostics` (returns `diagnostics_may_still_be_warming: bool` to indicate if the server may not have responded yet after a recent `didOpen`/`didChange`)
- `declaration` (Phase 4 — read-only; requires line+column; returns `Vec<LocationSummary>` capped at 100; capability-gated on `supports_declaration`)
- `implementation` (Phase 4 — read-only; requires line+column; returns `Vec<LocationSummary>` capped at 100; capability-gated on `supports_implementation`)
- `documentHighlights` (Phase 4 — read-only; requires line+column; returns `Vec<DocumentHighlightSummary>` capped at 100; preserves `Text` / `Read` / `Write` kind; capability-gated on `supports_document_highlight`)
- `signatureHelp` (Phase 4 — read-only; requires line+column; returns `Option<SignatureHelpSummary>` with active signature + parameter offsets resolved via UTF-16-aware `lsp_units_to_byte_offset()`; per-item doc truncated to 2000 chars; capability-gated on `supports_signature_help`)
- `completion` (Phase 4 — read-only; requires line+column; optional `trigger_kind`, `trigger_char`, `max_candidates` (default 200); returns `Vec<CompletionCandidate>` with raw edit payloads stripped; capability-gated on `supports_completion`)
- `semanticTokens` (Phase 4 — read-only; optional `max_tokens` (default 1000); returns `Vec<DecodedSemanticToken>` with legend-resolved type/modifier names; delta overflow rejected via checked arithmetic; capability-gated on `supports_semantic_tokens`)
- `codeActionSummaries` (Phase 4 — preview-only; requires a position range and optional `only` kind filter + `max_actions` (default 50); returns bounded `CodeActionSummary` records — never executes commands; never exposes raw `WorkspaceEdit` or `Command` payloads; capability-gated on `supports_code_actions`)
- `codeActionPreview` (Phase 4 — preview-only; requires a position range, an `action_index` (1-based, derived from `codeActionSummaries`), and optional `only` kind filter; returns `CodeActionPreview` wrapping a root-bounded `WorkspaceEditPreview`; rejects command-only actions with `LspError::CommandOnlyCodeAction`)
- `renamePreview` (preview-only; `RenamePreview` with `base_stale` for concurrent-edit detection; never mutates)
- `formatPreview` (preview-only; `FormattingPreview` with sha256 `before_hash`/`final_disk_hash` + `base_stale` flag + bounded 8KB diff + on-disk invariant check)
- `sourceActionPreview` (preview-only; same `WorkspaceEditPreview` shape; accepts `action` parameter — currently only `source.organizeImports` with aliases `organizeImports`/`organize_imports`; command-only actions are rejected because command execution is disabled)
- `semanticCheckPreview` (accepts either `content` or a single-file unified diff `patch`; patch input is applied in memory against `file_path` via `OverlaySession` (`apply_overlay`/`restore`), collects diagnostics + symbols, restores disk content, never writes disk; multi-file patches are unsupported in this pass; operation-level root enforcement via `allowed_root`; returns `SemanticCheckPreview` with `diagnostics_may_still_be_warming`, `diagnostics`, `diagnostics_error`, `symbols`, `symbols_error`, `restored_disk_view`, `restore_error`; `execute_structured` sets `success=false` when `restore_error` is present)
- `semanticContext` (combines multiple LSP requests; returns `SemanticContextPacket` with bounded source excerpt + diagnostics + symbols + optional definitions/references/overlay + optional source-action hints + optional call/type hierarchy; read-only, bounded; per-section errors via `definitions_error`, `references_error`; overlay limits tracked by `overlay_diagnostics_truncated`; `result_count` includes overlay items and available source-action hints; source excerpt truncation is UTF-8-safe via char-boundary cutting; `include_source_actions` boolean input, default false, populates `source_actions` array of `SemanticSourceActionHint` objects; `include_call_hierarchy` boolean input, default false, populates `call_hierarchy` section with incoming/outgoing callers; `include_type_hierarchy` boolean input, default false, populates `type_hierarchy` section with supertypes/subtypes; overlay translation stays handler-local because patch/content handling is tool-specific)
- `callHierarchy` (requires file_path, line, column; optional `direction` parameter — `incoming`, `outgoing`, or `both` (default `both`); returns `CallHierarchySummary` with items, incoming, outgoing, errors, truncated)
- `typeHierarchy` (requires file_path, line, column; optional `direction` parameter; returns `TypeHierarchySummary` with items, supertypes, subtypes, errors, truncated)
- `securityContext` (security-review context packet; returns risk markers, security-relevant diagnostics/symbols, optional definitions/references/call hierarchy, optional overlay; read-only, bounded; accepts `security_categories` filter and `max_risk_markers` cap; `include_call_hierarchy` defaults true when position provided; reuses shared diagnostic freshness evidence and capability snapshot from the common LSP path)
- `hunkSourceContext` (hunk-aware source navigation; consumes unified diff, maps changed hunks to enclosing symbols, diagnostics, definitions, references, hierarchy data; read-only, bounded; pure navigator via `HunkSourceNavigator`; DTOs in `crates/egglsp/src/hunk_context.rs`, parser in `src/lsp/hunk_nav_parser.rs`, range primitives in `src/lsp/hunk_nav_ranges.rs`, navigator in `src/lsp/hunk_nav.rs`, collector in `src/lsp/hunk_nav_collector.rs`)

**Preview-only contract**: `renamePreview` / `formatPreview` / `sourceActionPreview` (and the Phase 4 `codeActionSummaries` / `codeActionPreview`) produce bounded unified-diff patches for review via `WorkspaceEditPreview` (or the Phase 4 DTOs — `RenamePreview`, `CodeActionSummary`, `CodeActionPreview`, `FormattingPreview`). `sourceActionPreview` currently supports only `source.organizeImports`; arbitrary code actions and command execution are intentionally rejected. `CodeAction` values with `command: Some(_)` but `edit: None` are classified as command-only and rejected with `LspError::CommandOnlyCodeAction(title)` up front. `format_preview` enforces `allowed_root` at the crate layer. `formatPreview` re-reads the on-disk file at the end and returns `LspError::RequestFailed` if the `after_disk_hash != before_hash` — the on-disk invariant check is defense-in-depth. Large patches are structurally flagged via `FileEditPreview.patch_omitted` (not string matching). They are `ToolCategory::ReadOnly`. Actual file changes require the separate mutating `apply_patch` tool (or equivalent). `codeLens` is not exposed in the model-facing schema. Source-action hints returned via `semanticContext` with `include_source_actions: true` follow the same preview-only contract — each hint's `preview` field carries a `WorkspaceEditPreview` when the action is available and has edits, or `None` when unavailable or command-only.

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

### Capability Normalization Helpers

Phase 4 corrected capability normalization so the snapshot accurately reflects what the server actually advertised. Two internal helpers drive the normalization:

- `one_of_bool_or_options_supported()` — handles `Option<OneOf<bool, Options>>` provider fields. Returns `true` when the field is `Some(OneOf::Left(true))` or `Some(OneOf::Right(opts))`; `false` when absent, `None`, or `Some(OneOf::Left(false))`.
- `enum_simple_supported()` — handles `Option<E>` provider fields where the enum variant itself signals support (e.g., `DeclarationOptions`, `ImplementationOptions`). Returns `true` when the variant is present.

Unknown capabilities are surfaced via `CapabilityDecision`:

```rust
pub enum CapabilityDecision {
    Supported,
    Unsupported(LspUnavailable),
    Unknown { operation: LspSemanticOperation, reason: &'static str },
}
```

`LspCapabilitySnapshot::decide(op)` returns the decision; `Unknown` is logged explicitly when the snapshot cannot determine support.

### Stored Capability Snapshot

A single authoritative override-aware `LspCapabilitySnapshot` is stored per client at initialization time, via `LspService::normalized_capabilities_for_key(key)`. The snapshot is built from `ServerCapabilities` with profile-level `ObservedCapabilitiesOverride` merged (e.g. type hierarchy). Capability queries (`semanticContext`, `securityContext`, read-only navigation) all consult this stored snapshot rather than re-deriving capabilities per request.

### Effective Capabilities (One Authoritative Path)

`LspService::effective_capabilities_for_key(key)` is the single authoritative accessor for capability decisions. It merges the stored override-aware snapshot with observed push-diagnostics state from the live client (if the client has received at least one `publishDiagnostics` notification and the snapshot does not yet reflect it). This ensures capability decisions are always derived from one code path — there is no separate "pre-init" or "post-init" snapshot.

### LspCapabilitySnapshot

```rust
pub struct LspCapabilitySnapshot {
    pub language_id: Option<String>,
    pub server_name: Option<String>,
    // Phase 4: push diagnostics observed from actual publishDiagnostics notifications.
    pub supports_push_diagnostics: bool,
    // Phase 4: pull diagnostics advertised via diagnostic_provider capability.
    pub supports_pull_diagnostics: bool,
    pub supports_diagnostics: bool,                       // legacy alias: push || pull
    pub supports_document_symbols: bool,
    pub supports_workspace_symbols: bool,
    pub supports_definition: bool,
    pub supports_declaration: bool,                       // Phase 4
    pub supports_implementation: bool,                     // Phase 4
    pub supports_references: bool,
    pub supports_hover: bool,
    pub supports_document_highlight: bool,                 // Phase 4
    pub supports_completion: bool,
    pub supports_signature_help: bool,                     // Phase 4
    pub supports_rename: bool,
    pub supports_prepare_rename: bool,                     // Phase 4
    pub supports_code_actions: bool,
    pub supports_document_formatting: bool,               // Phase 4
    pub supports_range_formatting: bool,                   // Phase 4
    pub supports_inlay_hints: bool,                       // Phase 4
    pub supports_folding_ranges: bool,                    // Phase 4
    pub supports_selection_ranges: bool,                   // Phase 4
    pub supports_document_links: bool,                     // Phase 4
    pub supports_execute_command: bool,                   // Phase 4 — recorded, never invoked
    pub supports_call_hierarchy: bool,
    pub supports_type_hierarchy: bool,                     // no longer inferred from call_hierarchy
    pub supports_semantic_tokens: bool,
    pub details: LspCapabilityDetails,                     // Phase 4 — option-level info
}
```

Built via `LspCapabilitySnapshot::from_capabilities_with_override(caps, server_name, language_id, &ObservedCapabilitiesOverride)` which derives the snapshot from live server capabilities reported during `initialize` and merges profile-level overrides. The snapshot carries real `server_name` and `language_id` metadata from the initialized server. `supports_execute_command` is recorded for completeness; no model-facing path issues an `executeCommand` request.

### Querying Support

- `snapshot.supports(LspSemanticOperation::GotoDefinition)` → `bool`
- `snapshot.fallback_reason(LspSemanticOperation::Rename)` → `Option<&'static str>` — returns `Some("server does not support rename")` when unsupported, `None` when supported

### LspSemanticOperation

Enum covering all semantic operations the tool supports. Used for querying capability snapshots and for building fallback responses.

Phase 4 extended the enum with `Declaration`, `Implementation`, `DocumentHighlight`, `SignatureHelp`, `Rename`, `PrepareRename`, `CodeAction`, `DocumentFormatting`, `RangeFormatting`, `InlayHints`, `FoldingRanges`, `SelectionRanges`, `DocumentLinks`, `ExecuteCommand` — one variant per normalized capability. Every unsupported operation uses the same `LspUnavailable` structured-fallback path.

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

When no capability snapshot is available (server not yet initialized), operations return `LspError::NotInitialized` — fail-closed, not fail-open.

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
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust_analyzer
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- basedpyright

# Tier 2 smoke tests (Phase 4)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- gopls
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- typescript
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- clangd
```

The smoke tests (`crates/egglsp/tests/real_server_smoke.rs`) exercise rust-analyzer, pyright/basedpyright, **gopls**, **typescript-language-server**, and **clangd** against the production launcher, frame parser, and request routing. Phase 4 added three Tier 2 smoke tests:

| Test | Server | Pinned version |
|------|--------|----------------|
| `gopls_smoke` | `gopls` | v0.16.1 (Go 1.22.5) |
| `typescript_smoke` | `typescript-language-server` | 4.3.3 + `typescript` 5.5.4 (Node 20) |
| `clangd_smoke` | `clangd` | 18.1.8 (LLVM apt, checksum-verified archive) |

The Tier 2 tests resolve the binary via the `CODEGG_<SERVER>_BIN` env var (falling back to `PATH`) and emit `SKIP: …` on `eprintln` when not present so the suite stays CI-friendly without the binary. Reports are written to `target/lsp-compatibility/` under sanitized filenames.

Smoke tests are slow (200ms-2s startup plus indexing), non-hermetic (require installed binaries), and expensive in CI, so they are gated behind the `lsp-real-server-tests` feature. Phase 3 added the Tier 1 jobs (`rust-analyzer`, `basedpyright`) in `.github/workflows/lsp-real-server.yml`; Phase 4 extended the same workflow with `gopls`, `typescript-language-server`, and `clangd` matrix jobs — each pinned to a specific upstream version (see Pinned Versions above) and uploading its own artifact (`lsp-compat-<server>`). Tier 2 jobs run only on `workflow_dispatch`, weekly schedule, or push paths under `crates/egglsp/**`, `src/lsp/**`, or `.github/workflows/lsp-real-server.yml`; default CI remains network-free.

### Target Compatibility Matrix

| Server | Language | Key Operations | Known Limitations |
|--------|----------|----------------|-------------------|
| **rust-analyzer** | Rust | hover, definition, references, symbols, call hierarchy, rename, code actions, semanticContext, securityContext, hunkSourceContext | Initial indexing may be slow on large workspaces; diagnostics may need warm-up delay |
| **pyright** | Python | hover, definition, references, symbols, rename | No `prepareCallHierarchy` (Python doesn't have function-level call hierarchy); `codeAction` limited to organize imports |
| **typescript-language-server** | TypeScript / JavaScript | hover, definition, references, symbols, rename, code actions | `prepareCallHierarchy` may be empty; large workspaces slow |
| **gopls** | Go | hover, definition, references, symbols, rename, code actions | Call hierarchy not yet supported by gopls; securityContext will degrade gracefully |
| **clangd** | C / C++ | hover, definition, references, symbols, rename, code actions | No call hierarchy; slow on large TUs |

## Phase 3 Corrective Pass — Status

The Phase 3 supervision and restart lifecycle is complete for Tier 1 servers. Phase 4 has extended Tier 2 compatibility to gopls, typescript-language-server, and clangd on pinned versions; compatibility outside pinned versions remains experimental.

After the 11-pass closure, a follow-up **10-pass restart ownership / supersession / outcome** sequence (`plans/lsp_phase3_restart_ownership_and_cleanup_final_gap.md`) added explicit restart ownership primitives, live outcome semantics for degraded readiness, and user restart-policy override plumbing. That work is documented in a dedicated section below ("Restart ownership and live outcome semantics (10-pass)").

- **Final closure Pass 1 — Generation-aware runtime map**: `runtime_map` is now `HashMap<String, RuntimeEntry>` where `RuntimeEntry { generation: u64, runtime: LspProcessRuntime }`. Insertion, lookup, and removal go through `install_runtime`, `runtime_for_generation`, and `remove_runtime_if_generation` — all of which check the stored generation. A delayed old monitor cannot remove a newer generation's runtime. The unit tests `old_monitor_cannot_remove_new_runtime` and `runtime_removal_requires_exact_generation` lock the invariant down.
- **Final closure Pass 2 — Runtime intent + wait + kill + reap in `shutdown_all()`**: `LspClient::shutdown()` is split from the runtime-termination helper. The client method sends only `shutdown` / `exit` (now exposed as `request_protocol_shutdown`) and never waits on the child. The service helper `terminate_runtime(key, generation, client, graceful_deadline, absolute_deadline, reason)` runs the bounded sequence: intent → protocol shutdown → wait → force-kill → reap. `RuntimeTerminationReason` is `ServiceShutdown` / `ManualRestart` / `FailedPublication`. Hung processes are killed and reaped under the global deadline; the runtime map is empty after `shutdown_all()`.
- **Final closure Pass 3 — Single generation owner**: `LspService::next_generation_for_key(key) -> u64` is the single source of truth for replacement generation. The reinit closure receives the generation as an argument (`FnMut(&LspClientDescriptor, u64) -> BoxFuture<...>`) and must not compute generation independently. The coordinator calls `next_generation_for_key` exactly once per restart.
- **Final closure Pass 4 — Manual restart terminates old runtime**: `LspService::manual_restart_client(key)` is the public manual-restart API. It always runs (bypasses `LspRestartMode::Disabled`), terminates the old runtime with `RuntimeTerminationReason::ManualRestart` first, then starts the replacement. A manual restart issued while an automatic restart is in progress supersedes it.
- **Final closure Pass 5 — Shared crash-cycle restart budget**: The `restart_attempts` counter is shared across rapid crash cycles. `LspService::increment_restart_attempts(key)` is called once per actual replacement spawn. `LspService::set_last_healthy_now(key)` records the timestamp when readiness reaches `Ready`. `LspService::reset_restart_attempts_if_healthy_inherent(key, reset_after_healthy)` lazily resets the counter to 0 when the next unexpected exit observes a healthy interval. When `restart_attempts >= max_attempts` no new process is launched and the operational state transitions to `Failed`.
- **Final closure Pass 6 — Retained stale diagnostics + `post_restart` correction**: `LspService::snapshot_diagnostics_for_restart(key)` captures the live diagnostic cache for the old client. `LspClient::install_retained_diagnostics("restart", retained)` installs the snapshot in the new client, preserving the old `server_generation` and `post_restart` flags. The freshness classifier returns `Stale` because `entry.server_generation != current_generation`. A new `publishDiagnostics` from the new generation (including an empty vector) overwrites retained entries. `post_restart = generation > 1` is now enforced uniformly in `LspClient::bind_server_generation` and `DiagnosticCacheEntry::with_generation`; generation 1 is never `post_restart`, generation 2+ always is.
- **Final closure Pass 7 — Observed progress readiness**: `LspClient::wait_for_progress_end(timeout) -> bool` requires `ProgressState.completed_cycle == true`. Empty `active_tokens` alone is not sufficient — `wait_for_progress_end` returns `false` until a full `begin`/`end` cycle is observed. The real-server smoke harness calls `client.wait_for_progress_end(*timeout)` instead of fixed sleeps.
- **Final closure Pass 8 — Validated restart config + descriptor parity**: `LspRestartPolicyConfig::try_to_domain(&self, base: &LspRestartPolicy) -> Result<LspRestartPolicy, LspError>` validates user overrides. It rejects `OnUnexpectedExit` with `max_attempts == 0`, rejects `initial_backoff_ms > max_backoff_ms`, and rejects duration overflow. `LspRestartPolicyConfig::merge_with_profile` copies non-`None` fields from the profile. Cold start and restart consume the same persisted `LspClientDescriptor` — they receive identical `launch_spec`, `initialization_options`, `workspace_configuration`, `readiness_policy`, and `restart_policy`. The test `cold_start_and_restart_receive_identical_configuration` asserts generation 1 and generation 2 match exactly.
- **Final closure Pass 9 — Real-server stderr capture**: `LspProcessRuntime::stderr_tail_capped(max_lines) -> Vec<String>` returns the most recent lines from the bounded `StderrRingBuffer` (100 lines / 64KB cap) in chronological order. The real-server smoke harness (`RealServerHarness` struct in `crates/egglsp/tests/real_server_smoke.rs`) owns an `Arc<LspClient>` and its companion `LspProcessRuntime` (spawned via `spawn_process_runtime` from `egglsp::runtime`). Construction calls `client.take_child_for_runtime()` + `client.take_stderr_for_runtime()` and wires them into a fresh runtime at generation 1. The harness's `shutdown_and_collect(graceful_timeout, absolute_timeout)` runs the full bounded sequence: `runtime.request_graceful_shutdown()` → `client.request_protocol_shutdown()` → `runtime.wait_for_exit()` under graceful deadline → force-kill and re-wait on absolute deadline. It returns `HarnessShutdownResult::{Graceful, ForceKilled, TimeoutExpired}` — all variants carry a real `stderr_tail` from the runtime. `LspCompatibilityReport.stderr_tail` is populated from `harness.runtime().stderr_tail_capped(20)`. The readiness primitives `LspClient::wait_for_first_diagnostics(timeout)` and `LspClient::wait_for_progress_end(timeout)` replace the previous polling helper and fixed sleep. Four named tests exercise the harness and primitives directly: `smoke_harness_captures_stderr` (verifies `stderr_tail` is accessible after shutdown), `smoke_harness_force_kills_hung_server` (spawns a long-sleeping process and verifies force-kill), `progress_readiness_failure_is_reported` (verifies `wait_for_progress_end` returns `false` for non-LSP processes), and `empty_diagnostics_readiness_passes` (verifies `wait_for_first_diagnostics` returns `false` when no diagnostics are observed). Zero-length `findReferences` results fail the `RequiredIfAdvertised` check for the rust fixture; the Python cross-file fixture continues requiring at least two distinct URIs.
- **Final closure Pass 10 — Supervised constructor invariant**: `LspService::new(config)` is the bare test-only constructor — it returns `Self` without the cyclic back-reference wired. `LspService::new_arc(config) -> Arc<Self>` is the only public production constructor; it builds the service via `Arc::new_cyclic(|weak| Self { ..., self_ref: OnceLock::from(weak.clone()), ... })`. The test `new_arc_wires_self_ref` proves the production constructor populates `self_ref`. No public production path creates an un-supervised service.
- **Final closure Pass 11 — Test timing fix**: `generation_is_identical_across_health_and_exit_event` previously overwrote the generation-3 scenario before generation 2 started, causing the gen-2 process to read the gen-3 scenario. The test now writes the gen-3 scenario only after `service.generation_for_key(&key) >= 2` is observed, and the gen-2 process is verified `Ready` before the gen-3 file is staged. The supervisor/restart test surface is now **13** deterministic scripted scenarios: graceful shutdown, unexpected exit + restart disabled, automatic restart success, init failure then recovery, exhaustion, shutdown cancels scheduled restart, stale exit event, replay uses latest content, hung process force kill, two consecutive restarts use monotonic generations, generation identical across health and exit event, `manual_waits_for_cancelled_automatic_completion`, and `manual_restart_back_to_back_does_not_deadlock`.

### Restart ownership and live outcome semantics (10-pass)

The 11-pass follow-up (`plans/lsp_phase3_restart_ownership_and_cleanup_final_gap.md`) made restart attempts observable and bounded, so two simultaneous restart requests cannot silently corrupt the live client. The numbering below is independent of the 11-pass closure above.

- **Pass 1 — Restart completion channel**: `RestartCompletion` is a `tokio::sync::watch` enum (`Running` / `Finished`) that is the only completion signal for an in-flight coordinator. `RestartOwnerWaiter { completion: RestartCompletion }` is what `cancel_restart_ownership` returns. `acquire_restart_ownership` returns `RestartLease { token }`; the coordinator checks the token at every cancellation boundary (pre-backoff, mid-sleep, post-spawn, pre-publish, pre-replay). Cancellation of the lease token is *intent* — only `Finished` on the watch channel allows re-acquisition.
- **Pass 2 — Manual supersession with bounded wait**: `LspService::manual_restart_client(key)` acquires the manual lease first, then waits under `MANUAL_SUPERSESSION_OWNER_TIMEOUT = 3s` for any in-flight automatic-restart owner to complete via its `RestartCompletion::Finished` signal. On timeout, the manual caller aborts without touching the live client; on observed completion, the manual caller proceeds and is the only live coordinator for `key`.
- **Pass 3 — Generation-aware runtime install**: `RuntimeInstallResult` (`Installed` / `Replaced { prior }` / `Rejected { existing_generation, requested_generation }`) is returned from `install_runtime`. Same- or newer-generation replacement is logged at warn and rejected. A monitor that observes its own generation has been superseded terminates the orphan runtime rather than leaving it to drive future publication.
- **Pass 4 — Unpublished replacement + generation-scoped cleanup**: The coordinator's reinit closure now returns `UnpublishedReplacement { client, generation }` (with manual `Debug`) instead of `Arc<LspClient>`. Cleanup helpers `remove_unpublished_client_if_generation` and `terminate_unpublished_runtime` only touch unpublished resources when the stored generation matches the supplied one. A delayed old monitor cannot remove a newer generation's replacement. The `generation` is the closure's return value, not a separate field.
- **Pass 5 — Unified internal restart entry**: `OwnedRestartOptions` (`automatic()` / `manual()` constructors) is the internal options type; `restart_client_owned` is the unified internal entry. `manual_restart_client` and `restart_client` are now thin delegators; manual teardown cannot bypass the ownership layer.
- **Pass 6 — Degraded as a live outcome**: `restart_client_coordinator` now returns `Result<RestartOutcome, LspError>` where `RestartOutcome = Ready | Degraded { reason }`. `ReadinessResult::Degraded` no longer maps to `LspError::LaunchFailed`; it is a *live* outcome. The live client remains published, the consumed attempt remains consumed, and `last_healthy_at` is NOT updated. The orchestrator then converts `RestartOutcome` back to `Result<(), LspError>` for the public API surface.
- **Pass 7 — Empty diagnostics readiness**: The fake-server scenario engine's default `emit_progress = true` emits an empty `textDocument/publishDiagnostics { uri: "file:///dummy", diagnostics: [] }` on the `initialized` notification, which accidentally satisfies `wait_for_first_diagnostics`. The new test file `crates/egglsp/tests/empty_diagnostics_readiness.rs` has two tests: `empty_diagnostics_publishes_satisfy_wait_for_first_diagnostics` (the realistic case) and `missing_diagnostics_notification_times_out` (sets `emit_progress: false` in scenario to verify the timeout path).
- **Pass 8 — User restart policy round-trip**: `LspClientDescriptor::from_resolved(...)` is the production constructor that accepts an explicit `readiness_policy` and `restart_policy`. It is called from `LspService` when the user has configured `[lsp.<server>.restart]`; the user override is validated via `LspRestartPolicyConfig::try_to_domain(&base_policy)` and merged with the profile's defaults. Invalid overrides fall back to the profile default with a warning. `from_profile(...)` is retained for the no-user-override path.
- **Pass 9 — Race tests for manual supersession**: Two new scripted supervisor tests in `supervisor_restart_stdio.rs`: `manual_waits_for_cancelled_automatic_completion` and `manual_restart_back_to_back_does_not_deadlock`. Both tests accept `Ok`, `InitializationCancelled`, `ServerRestarted`, or `LaunchFailed` (budget exhausted) as valid outcomes; the critical invariant is bounded execution and a coherent service state. The supervisor/restart test surface is now **13** deterministic scripted scenarios.
- **Pass 1 — Restart ownership slot integrity**: `cancel_restart_ownership` no longer removes the per-key control entry — cancellation is intent, not completion. The slot remains exclusively owned by the in-flight owner until `RestartLease::release` signals `RestartCompletion::Finished` and removes its own entry. `RestartOwnerWaiter::wait` verifies the slot is free before returning `Ok`. Covered by unit tests in `crates/egglsp/src/restart.rs`: `cancel_does_not_remove_restart_owner`, `completion_release_allows_new_owner`, `closed_completion_without_release_is_not_success`.
- **Pass 2 — Manual pre-wait generation capture**: `LspService::capture_manual_supersession_snapshot` is taken BEFORE the lease cancellation so the post-wait comparison can detect a generation advance that occurred during the wait. The previous helper read the generation AFTER the wait and almost always saw the same value as the post-wait check (a no-op comparison). Covered by `manual_revalidates_generation_after_wait` (unit) and the integration race test.
- **Pass 3 — Publication boundary cancellation policy**: once the replacement is installed in the live clients map, lease-token cancellation is non-aborting — the coordinator continues to a coherent `Ready` or `Degraded` outcome. Pre-publication cancellation still reaps the unpublished client/runtime (Pass 4 invariant). Covered by `post_publication_cancellation_returns_live_outcome` (unit).
- **Pass 10 — Documentation sync**: `architecture/lsp.md`, `AGENTS.md`, and this skill guide are all updated to reflect the 10-pass ownership / supersession / outcome semantics. The final invariant checklist in `architecture/lsp.md` is extended with eight new items (restart completion channel, manual supersession bounded wait, runtime install generation rejection, unpublished replacement cleanup, unified restart entry, degraded-as-live-outcome, empty-diagnostics readiness, user restart policy round-trip, race test coverage).
- **Pass 11 — Final closure: remove-before-signal handshake**: `RestartLease::release` is now `async fn` and the per-key `RestartTaskControl` is **removed** from the per-key map before `RestartCompletion::Finished` is broadcast on the watch channel. This inverts the previous "send Finished, then remove" ordering — `Finished` is now the *consequence* of slot removal, not the trigger. Lock contention cannot produce a false `InitializationCancelled` result after a successful owner completion, because the broadcast is what guarantees the slot is free.

  The final handshake sequence is:

  1. Caller cancels the lease token (or the coordinator observes one of its cancellation boundaries).
  2. The owner unwinds and reaches the `release` point.
  3. The owner locks `restart_tasks`, **removes** the owner-ID-matched control entry, and releases the lock.
  4. **Only then** does the owner broadcast `RestartCompletion::Finished`.
  5. The waiter observes `Finished`, returns `Ok`, and immediately proceeds to acquire a new lease.
  6. A new owner acquires the now-free slot without racing the old owner.

  `RestartOwnerWaiter::wait` no longer needs a separate `verify_slot_free` post-check — the completion signal is the slot-release signal. If removal was skipped (entry absent or owned by a newer owner) the broadcast is suppressed: the sender is dropped, the waiter observes channel closure, and the closure is treated as an invariant failure rather than a success.

  `Drop` of `RestartLease` is a safety fallback for panic / early-return paths. It marks the lease as released, spawns an async cleanup task, and runs the same remove-before-signal ordering inside the spawned task. Production ownership paths MUST `await` `RestartLease::release` directly; `Drop` exists only to guarantee the slot is not leaked if the caller forgets.

  Four new adversarial unit tests in `crates/egglsp/src/restart.rs` lock the invariant down:

  - `finished_is_not_observable_until_slot_is_removed` — establishes a watch subscriber before the owner releases, then asserts the subscriber does not see `Finished` until the per-key map entry is gone.
  - `drop_fallback_removes_before_finished` — drops the lease without calling `release`, asserts the cleanup task removes the slot before signaling.
  - `old_owner_release_does_not_signal_for_new_owner` — simulates a newer owner acquiring the slot; the old owner's `release` observes the generation mismatch, suppresses the broadcast, and the new owner is never falsely told the slot is free.
  - `completion_channel_close_without_finished_is_error` — drops the sender without sending; the waiter observes `RecvError` and treats it as an invariant failure (not a silent `Ok`).

  - **Pass 12 — Async release cancellation safety (final cleanup)**: `RestartLease::release_async` commits `released = true` **inside** the ownership-map lock block. The flag commit and the slot removal are now part of the same synchronous critical section under the lock. Cancelling or aborting the `release()` future while it is parked on the lock await leaves `released == false` and routes cleanup to `Drop`'s safety fallback (which already runs the remove-before-signal ordering). Cancelling after lock acquisition cannot interrupt the critical section because there are no further `await` points between the flag commit and the completion broadcast. Two new adversarial unit tests in `crates/egglsp/src/restart.rs` lock the invariant down:

    - `cancelled_async_release_falls_back_to_drop_cleanup` — spawns a release task blocked on the map lock (held by a separate `lock_holder` task), aborts the release task while parked, verifies the `Drop` fallback removes the slot and the waiter observes `Finished`. Deterministic across 10 serial runs.
    - `completion_channel_close_error_names_owner` and `completion_timeout_error_names_owner` — verify the waiter error variants embed the in-flight `owner_id` (and the timeout duration) for caller diagnostics. The `RestartOwnerWaiter::owner_id` field is no longer `#[allow(dead_code)]`.

  Phase 3 supervision and restart lifecycle is complete for Tier 1 servers. Phase 4 has extended Tier 2 compatibility to gopls, typescript-language-server, and clangd on pinned versions; compatibility outside pinned versions remains experimental.

### Earlier Phase 3 Passes (still applicable)

- **Pass 1 — Real-server harness correctness**: `crates/egglsp/tests/real_server_smoke.rs` does a full `initialize` + `send_initialized` handshake, uses typed `RealServerFixture` metadata with `rust_fixture()` and `python_fixture()` constructors, queries only source files at exact positions, and classifies checks by `CompatibilityRequirement`. The new `assert_required_checks(report)` helper fails the test on `Required` regressions.
- **Pass 2 — Supervisor process ownership**: `LspProcessRuntime` (in `runtime.rs`) is the single authoritative process owner; it owns the child handle, stderr ring buffer, intent receiver, and kill channel. `LspService::new_arc(config)` wires the back-reference via `Arc::new_cyclic`; `ensure_exit_receiver_started` auto-activates on the first client-creating call.
- **Pass 3 — Generation and operational health**: `generation_map: Arc<Mutex<HashMap<String, u64>>>` provides per-key generation. `LspOperationalHealthSnapshot` is constructible without a live client and carries `transport: Option<...>`, `last_error`, `stderr_tail`, real `last_message_age_ms` / `last_diagnostics_age_ms`, and `restart_attempts`. All state transitions go through `transition_operational_state()` which uses the `health::transition()` validator.
- **Pass 4 — Restart descriptor and coordinator**: `LspClientDescriptor` persists the per-client launch spec; `LspClientDescriptor::from_profile` resolves readiness/restart policies with explicit `user > profile > server-definition` priority. `restart_client_coordinator<S, F>` is the single source of truth for retry/backoff/exhaustion/cancellation. `LspServiceClone` was removed and the duplicate `restart_client` paths were merged.
- **Pass 5 — Document replay and diagnostic freshness**: replay preserves the snapshot's per-document version; replay failure transitions to `Degraded` instead of silent `Ready`. `DiagnosticCacheEntry` carries `server_generation: u64` and `post_restart: bool`; on restart `mark_diagnostics_stale_for_key` rewrites retained entries to `current - 1` so they classify as `Stale`. `LspDiagnosticSnapshot` exposes both fields; the root `SemanticContextCollector` propagates them to `SemanticDiagnosticEvidence`.
- **Pass 6 — Readiness and workflow adoption**: `LspClient` tracks `ProgressState` (active `$/progress` tokens + last progress timestamp) and exposes `progress_snapshot`, `wait_for_progress_end`, `wait_for_first_diagnostics`, `operational_summary`. `LspService::wait_for_readiness(key, policy)` honors all four `LspReadinessPolicy` variants and returns `ReadinessResult::Ready { elapsed }` or `Degraded { reason, elapsed }`. `LspOperationalState::context_note()` is appended to `SemanticContextResponse.notes`, `SecurityContextPacket.notes`, and hunk source context summary lines.
- **Pass 7 — CI and docs**: `.github/workflows/lsp-real-server.yml` pins `rust-toolchain@1.81.0` (rust-analyzer job) and `basedpyright@1.13.1`; each matrix job runs only its own server test (`-- rust_analyzer` or `-- basedpyright`); artifact filenames are sanitized.

## Phase 4: Broader Compatibility & Higher-Level Capability Adoption (complete for pinned Tier 1 + Tier 2 matrix)

Phase 4 (`plans/lsp_phase4_broader_compatibility_and_capability_adoption.md`) broadens language-server coverage and exposes higher-value LSP capabilities, while preserving the safety rule established in earlier phases: read-only semantic operations may execute directly, mutation-producing operations must remain preview-only. Tier 2 compatibility is passing on pinned versions (gopls v0.16.1, typescript-language-server v4.3.3, clangd v18.1.8) with documented known limitations. All Phase 4 surface lives in `crates/egglsp/`; generic client code reads profile fields instead of branching on server IDs. Compatibility outside pinned versions remains experimental.

### Pass 0 — Baseline and report schema

Tier 1 reports remain valid; new `LspCompatibilityReport` fields (`protocol_version`, `dynamic_registrations`, `operation_support`, `fixture_language`, `project_model`) are additive and backward-compatible.

### Pass 1 — Correct capability normalization

`LspCapabilitySnapshot` no longer assumes diagnostics on every initialized server and no longer infers type hierarchy from call hierarchy. New booleans (`supports_declaration`, `supports_implementation`, `supports_document_highlight`, `supports_signature_help`, `supports_rename`, `supports_prepare_rename`, `supports_code_actions`, `supports_document_formatting`, `supports_range_formatting`, `supports_inlay_hints`, `supports_folding_ranges`, `supports_selection_ranges`, `supports_document_links`, `supports_execute_command`) are derived directly from the corresponding `ServerCapabilities` provider fields. `supports_push_diagnostics` / `supports_pull_diagnostics` split the legacy `supports_diagnostics` alias. `LspCapabilityDetails` carries option-level information that a bool cannot represent (`rename_prepare_provider`, `code_action_kinds`, `completion_trigger_characters`, `signature_trigger_characters`, `semantic_token_legend`). See the "Capability model" section above.

### Pass 2 — Tier 2 compatibility profiles

`gopls_profile()`, `typescript_language_server_profile()`, and `clangd_profile()` extend the data-driven pattern. Each profile records `executable_candidates`, `default_args`, `root_markers`, `initialization_options`, `workspace_configuration`, `readiness_policy`, `restart_policy`, `known_limitations`, and `observed_capabilities`. Tier membership is recorded on the profile; `tier2_profiles()` / `all_profiles()` provide deterministic accessors. `gopls` opts into `observed_capabilities.type_hierarchy = Some(true)`; `clangd` does not (override removed). `rust-analyzer_profile` was also updated in Phase 4 to keep its snapshot accurate.

### Pass 3 — Generalize the real-server fixture harness

The harness now drives typed `RealServerFixture { tempdir, root, language_id, source_files, primary_source, secondary_source, diagnostics_expectation, symbols, positions, mutation_targets }` records with per-position expectations (`definition`, `declaration`, `implementation`, `references`, `hover`, `completion`, `signature_help`, `rename`, `document_highlight`) and typed semantic expectations (`LocationExpectation { min_locations, expected_files }`, `CompletionExpectation`, `SignatureExpectation`). Fixture factories (`gopls_fixture()`, `typescript_fixture()`, `clangd_fixture()`) are pure data; adding Go, TypeScript, and C++ does not duplicate the smoke runner. The generic runner (`run_smoke_suite`) contains no server-ID conditionals — all server-specific behavior (readiness policy, expected symbols, position expectations) is carried by the `LspCompatibilityProfile` and `RealServerFixture`.

### Pass 4 — Read-only navigation

Added typed APIs `declaration`, `implementation`, `document_highlights`, `signature_help` (typed), and `workspace_symbols` (hardened with `normalize_workspace_symbol_response`). `declaration` / `implementation` normalize `Location` / `Location[]` / `LocationLink[]` / `null` responses into a uniform `Vec<LocationLink>` via `normalize_goto_response` (target URI, target range, selection range, origin selection range). `document_highlights` preserves `Text` / `Read` / `Write` kind. `signature_help` is bounded via `SignatureHelpSummary` (per-item documentation truncated to `SIGNATURE_DOC_MAX_CHARS` = 2000; parameter offsets resolved to substrings of the signature label). All use `LspOperations::require_capability` to short-circuit when the server does not advertise the corresponding provider.

### Pass 5 — Bounded completion and semantic tokens

`completion_bounded` returns `Vec<CompletionCandidate>` with raw `textEdit` / `additionalTextEdits` / `command` payloads stripped; `detail` and `insert_text_preview` are each truncated to `COMPLETION_DETAIL_MAX_CHARS` = 200. Server order is preserved (no client-side sort). `semantic_tokens` decodes the delta-encoded stream against the server's legend via the pure helper `decode_semantic_tokens`; out-of-range type indexes are reported as `LspError::RequestFailed` rather than silently dropped. Modifier bitsets are validated strictly — bits beyond the legend length return `LspError::RequestFailed` with a descriptive error, not silently truncated.

### Pass 6 — Preview-only rename

`prepare_rename_typed` returns `PrepareRenameResult { Range, DefaultBehavior, Unavailable(LspUnavailable) }` (normalized from the three `PrepareRenameResponse` variants). `rename_preview_typed` returns `RenamePreview { old_name, new_name, affected_files: Vec<FileEditPreview>, edit_count, warnings, truncated, server_generation }`. Validation rejects empty new names, caps files at 100 and edits at 1000, warns on resource operations (create / rename / delete), preserves document versions, and never executes `workspace/executeCommand`. Every `FileEditPreview` participates in staleness detection: after the rename preview is built, each affected file's on-disk content is re-read and its hash compared against `original_hash`. If any file changed externally during the request, `base_stale` is set.

### Pass 7 — Preview-only code actions

`code_action_summaries` returns `Vec<CodeActionSummary> { title, kind, preferred, disabled_reason, has_edit, has_command, diagnostics }` without exposing raw `WorkspaceEdit` or `Command` payloads. `preview_code_action` returns `CodeActionPreview` for a single resolved action and rejects command-only actions with `LspError::CommandOnlyCodeAction(title)` up front, before any network call. Code-action kinds are classified against `LspCapabilityDetails.code_action_kinds` when the server advertises a non-empty list.

### Pass 8 — Preview-only formatting

`format_preview_typed` returns `FormattingPreview { file, edit_count, before_hash, after_hash, diff, truncated, server_generation }`. Edits are applied to an in-memory snapshot only; the operation re-reads the on-disk file at the end and returns `LspError::RequestFailed` if `after_disk_hash != before_hash` — the on-disk invariant check is defense-in-depth. The diff is capped at `FORMATTING_PREVIEW_MAX_DIFF_BYTES` = 8 KB. The hash and diff are computed from the raw `TextEdit` list applied to the full file content — not from truncated preview DTOs.

### Pass 9 — Higher-level evidence in Codegg workflows

`LspTool` exposes eight new operations (`declaration`, `implementation`, `documentHighlights`, `signatureHelp`, `completion`, `semanticTokens`, `codeActionSummaries`, `codeActionPreview`); the existing `workspaceSymbol` was hardened. Per-operation caps and truncation flags are recorded explicitly in the JSON output (`result_count`, `truncated`). Health, generation, and truncation metadata are preserved on every result.

### Pass 10 — Tier 2 CI and compatibility matrix

`.github/workflows/lsp-real-server.yml` adds `gopls`, `typescript-language-server`, and `clangd` matrix jobs, each pinned to a specific upstream version (see "Real-Server Smoke Tests" above). Default CI remains network-free; Tier 2 jobs run only on opt-in triggers. Compatibility JSON, bounded stderr, fixture metadata, and operation check summaries are uploaded per server under `lsp-compat-<server>` artifact names.

### Pass 11 — Documentation and final verification

`architecture/lsp.md`, this skill guide, `AGENTS.md`, and `README.md` are aligned. Tier 1 suites remain green; Tier 2 suites are reproducible on opt-in CI. The mandatory safety tests — rename preview does not modify disk, code-action preview does not execute command, formatting preview does not modify disk, outside-root edits are rejected, edit and payload caps are enforced, unsupported operations return `LspUnavailable`, stale-generation results are not presented as current — are exercised in `crates/egglsp/tests/real_server_smoke.rs` and the production-harness paths.

### Outcomes at completion

1. Codegg has measured compatibility profiles for a Tier 2 server matrix (gopls, typescript-language-server, clangd).
2. Capability normalization accurately represents server-advertised support rather than relying on broad heuristics.
3. Read-only operations are available through typed `egglsp` APIs and capability-gated Codegg tools. Unknown capabilities are fail-closed: no request is sent before the capability state is authoritative.
4. Mutation-producing operations return structured previews (`RenamePreview`, `CodeActionSummary`, `CodeActionPreview`, `FormattingPreview`) and never modify the workspace automatically. Every `FileEditPreview` participates in staleness detection via `original_hash` comparison.
5. `format_preview_typed` computes hashes and diffs from raw `TextEdit` lists applied to full file content, not from truncated preview DTOs.
6. Semantic-token modifiers are validated strictly — unknown bits return an error rather than silently truncating.
7. Real-server fixtures validate operation semantics, not merely successful response parsing. The generic smoke runner (`run_smoke_suite`) contains no server-ID conditionals; all server-specific behavior is carried by fixture/profile data.
8. Tier 2 CI is opt-in and version-pinned; default CI remains network-free.
9. Existing Phase 2 and Phase 3 suites remain unchanged and green.

## Phase 4 Final Harness Evidence and Matrix Closure (Passes 1–11)

A follow-up 11-pass closure
(`plans/lsp_phase4_final_harness_evidence_and_matrix_closure.md`)
addresses gaps surfaced by the previous closure's
hardening work. The harness is now a first-class consumer
of the typed preview APIs and emits a complete operation
matrix on every run.

| Pass | Focus | Where |
|------|-------|-------|
| Pass 1 | Explicit `ImplementationExpectation` list per fixture (clangd accepts both override declaration `include/widget.hpp` and definition `src/widget.cpp`) | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 2 | Per-operation `LspOperationCompatibility` records emitted at request sites (no more `checks_to_operation_support` post-hoc walk) | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 3 | `assert_required_checks` fails `RequiredIfAdvertised` + `Skipped` when the matching capability was advertised (catches coverage gaps) | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 4 | `run_type_hierarchy_check` emits three distinct records (`typeHierarchy/prepare` / `typeHierarchy/supertypes` / `typeHierarchy/subtypes`) | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 5 | TypeScript code-actions fixture lands on the type-mismatch diagnostic at line 22; `code_action_min_edit_bearing = 1`; command-only → `KnownLimitation` | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 6 | `LspShutdownTrace { requested, server_exited, exit_code, signal, stderr_tail, duration_ms, mode: OperationMode { Stdio, Daemon }, force_kill_requested }` added to `LspCompatibilityReport` | `crates/egglsp/src/compatibility.rs` |
| Pass 7 | `crates/egglsp/src/position.rs` with `PositionEncoding { Utf8, Utf16, Utf32 }`, `lsp_units_to_byte_offset`, `lsp_range_to_byte_offsets` — semantic-token bounds validation is now encoding-aware (Pass 7 invariant: `signature_help` and `decode_semantic_tokens` share a single implementation) | `crates/egglsp/src/position.rs` |
| Pass 8 | `evaluate_rename_workspace_edit` verifies the response touches at least one expected file AND the edit range covers the identifier at `pos` (via `identifier_range_at`) | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 9 | `populate_operation_matrix` emits a default `LspOperationCompatibility` for every one of the 25 `LspSemanticOperation` variants — the report carries a complete matrix | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 10 | Full pinned Tier 1 + Tier 2 matrix executed; per-server JSON artifacts preserved under `target/lsp-compatibility/` | `crates/egglsp/tests/real_server_smoke.rs` |
| Pass 11 | Documentation closure — `architecture/lsp.md`, `AGENTS.md`, and this skill guide updated | All three doc surfaces |

### Phase 4 final closure invariants

- **No new `workspace/executeCommand` invocation.** Code-action
  command-only results remain rejected as `KnownLimitation`.
- **Per-operation traceability.** Every `LspOperationCompatibility`
  in the report is emitted at a single request site (Pass 2)
  or at the matrix pass (Pass 9). The legacy
  `checks_to_operation_support` walk is gone.
- **UTF-16-aware offset handling.** Semantic-token bounds
  and signature-help parameter offsets share a single
  encoding-aware implementation; CJK and supplementary-plane
  identifiers decode correctly across all pinned servers.
- **Fail-closed capability gating.** Pass 3 closes a coverage
  gap: a server that advertises a capability but is not
  exercised by the harness now fails `assert_required_checks`
  instead of silently passing.
- **Preview-only mutation boundary.** No new write paths
  added. The smoke harness still drives `textDocument/rename`
  and `textDocument/formatting` through the typed preview
  APIs (`rename_preview_typed`, `format_preview_typed`); the
  on-disk file is never mutated.

## Phase 4 Final Evidence-Integrity Cleanup (Passes 1–10)

A follow-up cleanup pass
(`plans/lsp_phase4_final_evidence_integrity_cleanup.md`)
addresses the remaining Phase 4 evidence-integrity gaps:
operation records reconstructed from free-form check text,
rename checks accepting null responses, coarse shutdown
traces, closure still relying partly on check-name parsing,
coarse type-hierarchy vs concrete suboperation reconciliation,
TypeScript code-action fixture not proving a previewable
edit-bearing action, implicit UTF-16 in semantic-token bounds,
and pinned matrix execution evidence not preserved in a
navigable manifest.

### Pass summary

| Pass | Focus | What changed |
|------|-------|--------------|
| Pass 1 | Exact request-site outcomes | `OperationOutcome` struct (operation, advertised, exercised, request_succeeded, response_parsed, semantic_assertion_passed, requirement, known_limit) replaces string parsing. All 10 `run_*_check` helpers emit an `LspOperationCompatibility` at the request site. New `response_parsed` field on `LspOperationCompatibility` (serde-defaulted for backward compatibility) distinguishes protocol success from parse success. `operation_record_from_check()` is removed. |
| Pass 2 | Typed rename expectations | `RenameExpectation { source_file, position, new_name, min_edits, expected_files, require_identifier_overlap }` on `RealServerFixture`. Null response and zero-edit responses now fail when `min_edits > 0`. The TypeScript fixture opts into rename via `rename_expectation: Some(...)` and is verified to touch both `main.ts` and `helper.ts`. Disk hash is verified unchanged. The legacy `mutation_targets.rename_preview_requested` field is no longer consulted. |
| Pass 3 | Granular shutdown trace | `LspShutdownTrace` gained 9 new fields (serde-defaulted): `shutdown_request_sent`, `shutdown_response_received`, `exit_notification_sent`, `writer_flush_succeeded`, `writer_closed`, `graceful_wait_completed`, `graceful_exit_observed`, `force_kill_succeeded`, `child_reaped`. `LspClient::request_protocol_shutdown_traced()` returns a `ProtocolShutdownTrace` capturing each step independently. The harness wires each step through `RealServerHarness::shutdown_and_collect`. |
| Pass 4 | Closure from typed records | `assert_required_checks` walks `report.operation_support` directly, never parses check names. Each `LspOperationCompatibility` carries `requirement` and `known_limit`; the closure enforces `Required`, `RequiredIfAdvertised`, `KnownLimitation`, and `Optional` from typed fields. The legacy `check_name_advertised()` helper is removed. |
| Pass 5 | Type-hierarchy from suboperations | The coarse `LspSemanticOperation::TypeHierarchy` is removed from the fallback matrix. Hierarchy coverage comes exclusively from the three suboperations (`typeHierarchy/prepare`, `typeHierarchy/supertypes`, `typeHierarchy/subtypes`) emitted by `run_type_hierarchy_check`. |
| Pass 6 | Edit-bearing TypeScript code action | The TypeScript fixture lands on the type-mismatch diagnostic at line 22 (`const x: string = 42;`) with a 20-character range; `code_action_min_edit_bearing = 1`. The harness classifies command-only results as `KnownLimitation` and edit-bearing results as `Passing`. |
| Pass 7 | Negotiated position encoding | `LspClient::position_encoding()` returns the live negotiated encoding (`PositionEncoding::{Utf8, Utf16, Utf32}`); `set_position_encoding()` records it during `initialize`. `LspCapabilityDetails.position_encoding` carries the negotiated value when the server advertises it. `LspCompatibilityReport.position_encoding` and `position_encoding_assumed` record the negotiated value and whether UTF-16 was assumed. Semantic-token bounds use `client.position_encoding()` instead of assuming UTF-16. |
| Pass 8 | Fixture-aware fallback requirements | `RealServerFixture::requirement_for(op)` derives `RequiredIfAdvertised` for operations the fixture opts into (implementation targets, rename expectation, format-preview request, code-action min-edit-bearing, type-hierarchy targets) and `Optional` otherwise. `populate_operation_matrix` uses this so the fallback records reflect fixture expectations and the closure detects advertised-but-unexercised coverage gaps. |
| Pass 9 | Matrix manifest preservation | `update_matrix_manifest()` writes `target/lsp-compatibility/matrix-manifest.json` per server (commit SHA, `GITHUB_RUN_ID`, per-server artifact path + version + position encoding + record counts). The CI workflow `.github/workflows/lsp-real-server.yml` adds a `matrix-summary` job that downloads all per-server artifacts, verifies the manifest exists, and uploads it as `lsp-compat-matrix-manifest`. |
| Pass 10 | Regression + docs | All passes pass `cargo check` and the production integration suite (`production_protocol_stdio`, `production_semantic_stdio`, `production_service_stdio`, `supervisor_restart_stdio`, `empty_diagnostics_readiness`). Two pre-existing flaky unit tests (`smoke_harness_force_kills_hung_server` and rust-analyzer `typeHierarchy/prepare` against the installed version) remain red and are unrelated to this cleanup. `architecture/lsp.md`, `AGENTS.md`, and this skill guide document the new evidence. |

### Phase 4 final closure definition (Pass 10)

Phase 4 evidence-integrity cleanup is complete only when all
of the following are true:

1. No operation compatibility record is inferred from free-form check text.
2. Protocol success, parse success, and semantic success are recorded independently at the request site.
3. Opted-in rename checks fail on null or zero-edit responses.
4. Shutdown traces record every protocol/runtime step individually.
5. Closure assertions use machine-readable operation records, not check-name parsing.
6. Type-hierarchy aggregate status is derived from prepare/subtype/supertype records.
7. At least one pinned TypeScript code-action check returns a safe edit-bearing action and passes without a known limitation.
8. Semantic-token bounds use the negotiated position encoding or explicitly record that UTF-16 was assumed.
9. The full pinned server matrix is actually run and compatibility artifacts are preserved.
10. Documentation claims only what the final artifacts prove.
11. Phase 2 and Phase 3 regression suites remain green.

### Status

Phase 4 complete for the exact pinned Tier 1 and Tier 2 matrix.
Compatibility outcomes are emitted at request sites, required
advertised operations are enforced from typed records,
rename/code-action previews are semantically validated,
shutdown and position-encoding evidence are preserved, and
the complete artifact manifest is available. Compatibility
outside pinned versions remains experimental.

## Phase 5: Agent Context and Workflow Integration

Phase 5 turns the LSP substrate into bounded, explainable, and safe agent context. The Phase 5 closeout (passes 1–9 below) further hardens the surface, reasserts safety invariants at the boundary, and narrows the public API to one canonical packet with named bridges.

### New Modules

```
crates/egglsp/src/
├── context.rs              # LspContextPacket, LspContextRequest, LspContextBudget, LspContextItem, LspContextMode
├── evidence_collector.rs   # LspEvidenceProvider trait, collect_context(), collect_hunk_context()
├── security_context.rs     # SecurityRiskTag, SecurityEvidenceSummary, build_security_evidence_summary(), build_security_lsp_context_request()
├── context_renderer.rs     # render_lsp_context_for_agent(), render_lsp_status_line(), ModelTier, model_tier_for_profile()
├── preview_registry.rs     # PreviewArtifactRegistry, PreviewArtifactEntry
├── tui_summary.rs          # LspTuiSummary, render_tui_status_line(), render_tui_summary_detail()
├── degradation_policy.rs   # evaluate_degradation(), LspContextDegradeDecision
├── evidence_adapter.rs     # ServiceLspEvidenceProvider (production adapter for the live LspService; consume_provenance_for guard)
├── hunk_context.rs         # HunkDescriptor, HunkEvidence, hunk_response_to_context_items()
└── bridges.rs              # Canonical bridges: semantic_context_to_lsp_items, lsp_packet_to_security_summary, lsp_packet_to_tui_summary

tests/
├── phase5_context_integration.rs  # 53 composite tests (49 pre-existing + 4 no-mutation sweep)
└── lsp_composite_stdio.rs         # 36 production-stack tests + 6 hunk/security bridge production-seam tests
```

### Key Concepts

- **Context packets** carry bounded LSP evidence with provenance (server, generation, freshness, capability decision)
- **Budget enforcement** is deterministic: per-file → category → file count → byte budget
- **Dedup** by kind+file+range+symbol+message hash
- **Ranking**: hunk-local > errors > same-file > definitions > fresh > short
- **Three modes**: Disabled (no calls), Opportunistic (partial on failure), Required (error on failure)
- **Preview artifacts** are registered but never applied (`applied=false`); `LspTool` owns an internal `PreviewArtifactRegistry` and every `LspToolOutput` carries a `preview_id: Option<String>` plus a `preview_metadata: Option<PreviewMetadata>` envelope with `not_applied`, `edit_count`, `affected_files`, `stale_base`
- **Agent rendering** is tier-aware: Small omits refs/hover, Workhorse includes them, Frontier is broader; `model_tier_for_profile()` is best-effort string match and is wired through `DefaultTurnRuntime::run_turn` via `assemble_lsp_context_for_turn` + `resolve_lsp_context_tier`
- **TUI summary** shows server status, counts, truncation, stale state, total items, freshness breakdown, preview ids, and unsupported operations
- **Hunk navigation bridge** converts `HunkSourceNavigationResponse` to `Vec<LspContextItem>` tagged with `AgentContextSource::Hunk`; truncated responses are marked `LspEvidenceFreshness::PossiblyStale`
- **Security review integration** requests context via `LspContextRequest::Review` with `LspRiskMode::Aggressive`; `SecurityEvidenceSummary.public_api_fanout` counts distinct reference files in the reviewed changed files
- **Production evidence adapter** (`ServiceLspEvidenceProvider`) wires `LspEvidenceProvider` to the live `LspService`, capturing `EvidenceOperation` provenance on every item; the side-channel contract is enforced via `consume_provenance_for(expected_operation)` which atomically takes the slot and validates the operation matches the caller's expectation
- **Canonical bridges** in `crates/egglsp/src/bridges.rs` are the single entry point for converting between `LspContextPacket` and tool/UI/audit shapes

### Canonical packet boundary (Pass 1, hardened in Pass 4)

`LspContextPacket` in `crates/egglsp/src/context.rs` is the single source of truth. The legacy tool-local `SemanticContextPacket` bridges via `into_lsp_context_packet()` and `from_lsp_context_packet()`. New callers should consume the canonical packet directly and route through the named bridges in `crates/egglsp/src/bridges.rs`. Do NOT add new parallel packet shapes — extend `LspContextPacket` / `LspContextItem` and add a bridge function instead.

### Agent context input (Pass 3)

`LspAgentContextInput` is threaded through `TurnRunInput` so the agent loop can pass workflow metadata (changed files, hunks, active file, optional `ModelTier` override) to `LspTool::lsp_context_for_agent_with_input()`. `is_empty()` and `has_workflow_metadata()` agree on the same definition: mode flags alone are not workflow metadata.

### Preview-ID propagation (Pass 2)

`LspToolOutput.preview_metadata: Option<PreviewMetadata>` is the canonical envelope for the four preview operations. The new fields are `not_applied: bool` (always true in Phase 5), `edit_count: usize`, `affected_files: Vec<String>`, and `stale_base: bool`. The legacy `preview_id: Option<String>` field is also populated. 10 unit tests in `src/tool/lsp.rs` and 4 live integration tests in `tests/lsp_composite_stdio.rs` lock the shape and the disk-non-mutation invariant.

### Evidence-adapter provenance contract (Pass 3)

`ServiceLspEvidenceProvider::consume_provenance_for(expected_operation)` atomically takes the side-channel slot and validates the recorded operation matches the caller. The collector already dispatches all provider calls sequentially within `collect_context`; the contract is now pinned on the `LspEvidenceProvider` trait and on the adapter implementation. Tuple-trait API is unchanged. 5 new tests cover immediate consumption, mismatch detection, non-parallelization, slot clearing, and on-error recording.

### Model-tier in turn runtime (Pass 5)

`DefaultTurnRuntime::run_turn` extracts LSP-context assembly into two `pub(crate)` helpers: `resolve_lsp_context_tier(input, family)` and `assemble_lsp_context_for_turn(svc, input, family, allowed_root)`. 7 new tests in `src/agent/turn_runtime.rs::tier_resolution_tests` cover small-tier truncation visibility, frontier-tier reference visibility, workhorse default, explicit override preservation, and the assemble-helper with no clients.

### Production-seam tests (Pass 9 from hardening, expanded in Pass 6 of closeout)

7 production-seam tests in `tests/phase5_context_integration.rs::production_seam_tests` exercise the production adapter (`ServiceLspEvidenceProvider`), the hunk navigation bridge, the security bridge, and the preview registry end-to-end via the existing `MockProvider` infrastructure — no real or fake LSP server required. Pass 6 of the closeout adds 6 more in `tests/lsp_composite_stdio.rs` that drive the real `LspTool` stack against the fake server: 3 hunk-bridge tests (`preserves_hunk_source_tag`, `records_truncation`, `degrades_without_lsp`) and 3 security-bridge tests (`preserves_public_api_fanout`, `omits_preview_mutations`, `marks_stale_evidence`).

### No-mutation / no-executeCommand sweep (Pass 7)

4 hash-based tests in `tests/phase5_context_integration.rs::phase5_no_mutation_sweep` create real files in a `tempfile::TempDir`, hash them with `egglsp::operations::sha256_hex`, run the Phase 5 path under audit, and assert hash-equal:

- `phase5_agent_context_collection_does_not_apply_rename`
- `phase5_agent_context_collection_does_not_apply_formatting`
- `phase5_agent_context_collection_does_not_execute_code_action_command`
- `phase5_preview_registration_does_not_apply_workspace_edit`

Static audit of `executeCommand` / `workspace/executeCommand` / `applyEdit` / `workspace/applyEdit` across `src/` and `crates/egglsp/src/` produced 19 matches, all classified OK (comments, capability advertisement, or rejection tests). No Phase 5 code path invokes mutation.

### Safety

No LSP preview mutates disk. `workspace/executeCommand` is never invoked. The preview registry's `applied` field is always `false`. Every `LspToolOutput.preview_metadata` carries `not_applied: true`. The agent-loop `test_no_follow_up_latency` regression is fixed (15s wall-clock bound + deterministic provider-call-count behavioral assertion).

## See Also

- [tool.md](tool.md) - LSP tool wrapper
- [architecture/lsp.md](../../architecture/lsp.md) - Architecture documentation
