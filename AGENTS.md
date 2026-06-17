# AGENTS.md

## Project Overview

This is a **Rust rewrite of an AI coding agent**, built for performance and efficiency. The codebase uses:

- **Tokio** for async runtime
- **SQLx** for SQLite database
- **Ratatui** for terminal UI
- **Axum** for HTTP server (feature-gated)
- **Wasmtime** for WASM plugins (feature-gated)

## Module Reference (38 Modules)

| Module | Purpose |
|--------|---------|
| `agent/` | AgentLoop, compaction, routing, team, turn_runtime |
| `auth/` | Typed `AuthConfig` (api_key / stored / external_command / oauth_device), `Credential`, `AuthResolver` (env → config → store priority), `CredentialStore` at `~/.config/codegg/credentials.json`, `ExternalCommandProvider` (typed but disabled — both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported` for any non-empty command until async timeout plumbing exists), OAuth device-flow scaffolding, `mask_secret`, `cli::AuthCli` for `codegg auth status | set-key | logout`. CLI validates provider/account ids (`[A-Za-z0-9_-]`, with `*` allowed for `logout` only) and never echoes key material. |
| `bus/` | Event bus system (GlobalEventBus, PermissionRegistry, QuestionRegistry) — now in `crates/codegg-core` (`codegg-core` crate) |
| `client/` | Remote TUI client for WebSocket connections with resume/replay support |
| `command/` | Slash command registry and routing from markdown files |
| `config/` | Configuration loading, validation, and file watcher — now in `crates/codegg-config` (`codegg-config` crate), re-exported as `codegg::config` |
| `context/` | Context artifact storage, tool-output projection, `context_read` tool, cache-aware context packing for stable provider prompt-cache prefixes, `NormalizedProviderUsage` (usage_normalize.rs), `EffectiveCostAnalysis` (effective_cost.rs), gated context policy layer (ContextPolicyConfig + hardened tool-palette reduction: base_request_tools source-of-truth, ContextPolicyRuntimeState, non-cumulative base-derived reductions, backoff+starvation detection, Warn dry-run would_*, review_tool_palette_threshold gate, enhanced diagnostics); volatile-tail compaction (`volatile_tail.rs`): gated late-context-only compaction of old tool-result messages with recovery handles, tombstone format, idempotent, observe/warn/compact rollout; defaults safe (disabled/observe) |
| `crypto/` | AES-256-GCM encryption with Argon2id key derivation |
| `error/` | Centralized `AppError` enum with `ProviderError::is_retryable()`, `ToolError::is_retryable()`, `CircuitError` conversion — error enums now in `crates/codegg-core` (`codegg-core` crate), axum wrappers stay root-side |
| `exec/` | Non-interactive exec mode for CI/CD with JSON I/O |
| `git/` | **Removed** in 2026 native crate extraction. Read-only git facts now in `crates/egggit/` (`repo_status`, `diff_summary`, `changed_files`, `file_diff`, `validate_patch`, `list_worktrees`). Mutating worktree operations removed (worktree is now read-only in codegg-core). The `git` tool wrapper still lives at `src/tool/git.rs` |
| `goal/` | Long-horizon goal runtime: budget enforcement, auto-continuation, GoalStore persistence, system prompt steering — now in `crates/codegg-core` (`codegg-core` crate) |
| `hooks/` | Hooks system for agent loop lifecycle events and plugin interaction |
| `ide/` | IDE integration (VS Code IPC, JetBrains remote mode) |
| `lsp/` | Language Server Protocol support (diagnostics, code operations, preview-only semantic edits, temporary overlays, semantic context packets, securityContext call expansion, hunkSourceContext hunk-aware navigation, capability discovery and normalization, diagnostics cache lifecycle with freshness metadata, capability-gated operations, diagnostic evidence in context packets, shared semantic context API, initialization coordinator with explicit leader/waiter election, shared completion fan-out, lifecycle-validated publication, unpublished-client disposal, SharedInitError for concurrent init, lifecycle generation tracking, timeout-cancel transport-failure propagation, authoritative completion-receiver ownership of init task wrappers (no forwarding tasks wrap real `JoinHandle`s), start-registration barrier (one-shot oneshot that gates the wrapper body until `active_init_tasks` is installed), explicit wrapper cleanup + `ActiveTaskGuard` fallback (spawned follow-up task — no `try_lock`), watch-based concurrent shutdown coordination, deadline-driven quiescent shutdown bounded by 6s, aggregate grace wait via `await_init_task_completions` over completion receivers) — egglsp crate is authoritative implementation, src/lsp/ is thin shim; `WorkspaceEditPreview`/`FileEditPreview`/`TextEditPreview` re-exported from egglsp. **Phase 3 final closure** adds: generation-aware `RuntimeEntry` runtime map with `install_runtime` / `runtime_for_generation` / `remove_runtime_if_generation` helpers; runtime termination sequence (intent → protocol shutdown → wait → force-kill → reap) via `terminate_runtime` and `RuntimeTerminationReason` (`ServiceShutdown` / `ManualRestart` / `FailedPublication`); single generation owner (`LspService::next_generation_for_key`, reinit closure receives generation as argument); manual restart API (`LspService::manual_restart_client`) that terminates the old runtime first; shared crash-cycle restart budget with lazy healthy reset (`LspService::increment_restart_attempts`, `reset_restart_attempts_if_healthy_inherent`, `set_last_healthy_now`); retained stale diagnostics across restart (`LspClient::install_retained_diagnostics`, `LspService::snapshot_diagnostics_for_restart`); `post_restart = generation > 1` enforced uniformly; observed-cycle progress readiness (`ProgressState.completed_cycle` required by `LspClient::wait_for_progress_end`); `LspRestartPolicyConfig::try_to_domain` validation (zero attempts, initial > max backoff, duration overflow); real-server stderr capture via `LspProcessRuntime::stderr_tail_capped`; `LspService::new_arc` is the only public production constructor (`LspService::new` is test-only). 11 deterministic scripted supervisor/restart tests in `crates/egglsp/tests/supervisor_restart_stdio.rs` cover the invariants. Phase 2 stdio integration tests now live in `crates/egglsp/tests/`; the legacy fake-server suites use `FakeLspHarness`, the production-harness protocol subset uses `ProductionClientHarness`, and `scenario_engine.rs` includes the fake-server self-tests (inlined, no external `include!`). `egglsp::test_support` is feature-gated behind `lsp-test-support` and `#[doc(hidden)]`; the root `lsp-test-support` feature forwards to `egglsp/lsp-test-support`. `base64` and `libc` are optional dependencies gated on `lsp-test-support`. The fake LSP server is built as the `codegg-lsp-test-server` bin target from the `egglsp` package. Fixture binaries require the `lsp-test-support` feature and are excluded from normal production builds. Root-crate composite tests in `tests/lsp_composite_stdio.rs` exercise `SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`, and security context tool orchestration against the fake server via the production stack (26 tests covering semantic/security/hunk collectors, workspace-edit-preview safety, and capability-gated degradation). The fake server supports captured-ID mode for genuinely out-of-order concurrent responses. |
| `lsp/` (hunk_nav) | Hunk/source navigation: unified diff parser, range matching, HunkSourceNavigator, HunkSourceNavigationCollector, `HunkSourceContextPolicy` (decides when to invoke hunkSourceContext), `HunkSourceContextDecision` enum (Use/Skip), `decide_hunk_source_context()` pure policy function, `format_hunk_source_context_summary()` for compact agent-facing summaries |
| `lsp/` (compatibility) | `crates/egglsp/src/compatibility.rs` — LspCompatibilityProfile, LspReadinessPolicy (4-variant: InitializedIsReady / WaitForDiagnosticsOrTimeout / WaitForProgressEndOrTimeout / WarmupDelay), LspRestartPolicy (mode, max_attempts, initial_backoff, max_backoff, reset_after_healthy), LspRestartMode (Disabled / OnUnexpectedExit), LspServerVersion, LspCompatibilityReport, LspCompatibilityCheck, CompatibilityCheckStatus, CompatibilityRequirement (Required / RequiredIfAdvertised / Optional / KnownLimitation), rust_analyzer_profile(), pyright_profile(), profile_for_server(), tier1_profiles(), require_server_binary() |
| `lsp/` (health) | `crates/egglsp/src/health.rs` — LspOperationalState (Starting/Initializing/Indexing/Ready/Deaded/RestartScheduled/Restarting/Failed/Stopping/Stopped), transition() state machine, InvalidTransition, LspOperationalHealthSnapshot (with `generation: u64` from `generation_map`, `transport: Option<...>`, `last_error`, `stderr_tail`, real `last_message_age_ms` / `last_diagnostics_age_ms`, `restart_attempts`), `LspOperationalState::context_note()` |
| `lsp/` (runtime) | `crates/egglsp/src/runtime.rs` — LspProcessRuntime (single authoritative process owner), LspProcessIntent (Running / GracefulShutdownRequested / ForceKillRequested), spawn_process_runtime() |
| `lsp/` (restart) | `crates/egglsp/src/restart.rs` — LspClientDescriptor (persisted per-client launch spec, built from user config → profile → server-definition priority), RestartTrigger (Automatic / Manual), restart_client_coordinator<S,F>, backoff_delay, RestartShared trait, ServicePhase. User-configurable restart via `[lsp.<server>.restart]` TOML section overrides profile defaults (mode, max_attempts, initial_backoff, max_backoff, reset_after_healthy). |
| `lsp/` (supervisor) | `crates/egglsp/src/supervisor.rs` — LspProcessExitEvent (carries generation, status, signal, expected flag, stderr_tail), StderrRingBuffer (100 lines / 64KB cap) |
| `lsp/` (document_sync) | `crates/egglsp/src/document_sync.rs` — OpenDocumentRegistry, OpenDocumentSnapshot (preserves version for replay) |
| `mcp/` | Model Context Protocol client (local, remote, auth) with auto-reconnect |
| `core/` | Core facade and transport adapters (inproc, stdio, socket) for request/response separation — `src/core/` is the transport layer; domain modules (bus, error, goal, memory, session, storage, snapshot, worktree, resilience, task_state, model_profile, protocol_conversions) live in `crates/codegg-core`. Also contains `runtime_deps` (`CoreRuntimeDeps`) for bundling runtime dependencies. |
| `memory/` | Persistent memory system for session learning and namespace management — now in `crates/codegg-core` (`codegg-core` crate) |
| `permission/` | Access control, path restrictions, DoomLoop detection, mode system |
| `plugin/` | WASM plugin system with hooks and TUI extensions |
| `provider/` | LLM provider implementations (Anthropic, OpenAI, Google, etc.) — now in `crates/codegg-providers` (`codegg-providers` crate), re-exported as `codegg::provider` |
| `protocol/` | Shared `CoreRequest`/`CoreResponse` and `TuiMessage` protocol envelopes — now in `crates/codegg-protocol` (`codegg-protocol` crate), re-exported as `codegg::protocol` |
| `shell_session/` | Shell session metadata management (in-memory, no actual PTY) |
| `resilience/` | Circuit breaker, retry mechanisms, and rate limiting — now in `crates/codegg-core` (`codegg-core` crate) |
| `research/` | Research pipeline for claims verification, source fetching, synthesis, and handoff |
| `security/` | SSRF protection, internal IP validation, Landlock sandboxing, security review workflow (diff parsing, preset selection, target building, securityContext request construction, review-prompt generation, hunk context display, hunk source context integration) — workflow module split into 9 files under `src/security/workflow/` (`mod.rs`, `types.rs`, `diff.rs`, `preflight.rs`, `evidence.rs`, `context.rs`, `report.rs`, `enrichment.rs`, `receipt.rs`). The `receipt.rs` submodule holds the TUI-facing `SecurityReviewReceipt` DTO, the `SecurityReviewPanelItem` projection (with `hunk: Option<SecurityReviewHunkRef>`), the `SecurityReviewFilter` enum (including `HunkBacked`), the `SecurityReviewTaskState` guard, and the `project_receipt_to_panel_items` / `filter_panel_items` helpers consumed by the `Dialog::SecurityReview` panel. |
| `server/` | HTTP server (Axum) with WebSocket support for remote TUIs and replay buffering |
| `session/` | Session storage, message history, and checkpointing (SQLite) — now in `crates/codegg-core` (`codegg-core` crate) |
| `skills/` | Skill system for specialized capabilities (git, research, etc.) |
| `snapshot/` | Snapshot support for file state capture and restore — now in `crates/codegg-core` (`codegg-core` crate) |
| `storage/` | SQLite database storage layer and initialization — now in `crates/codegg-core` (`codegg-core` crate) |
| `theme/` | Frontend-neutral theme system, registry, Halloy compat, validation, projections |
| `tool/` | Built-in tools (bash, read, edit, task, webfetch, image, etc.) |
| `tts/` | Text-to-speech module with provider support |
| `tui/` | Terminal user interface (widgets, handlers, input processing, diff viewer, notifications, image support, CoreClient-backed flows) |
| `upgrade/` | Self-upgrade functionality via GitHub releases |
| `util/` | Utility functions (clipboard, fuzzy search, pricing, etc.) |
| `worktree/` | Git worktree support for project management — now in `crates/codegg-core` (`codegg-core` crate), read-only. Mutating operations removed. |

## Architecture Index

- `architecture/core.md`: Core crate architecture, ownership boundaries, and extraction status
- `architecture/codegg_core.md`: codegg-core workspace crate (bus, error, goal, memory, session, storage, snapshot, worktree, task_state, model_profile, resilience, protocol_conversions)
- `architecture/tui.md`: TUI state, dialog/component maintenance, and CoreClient-backed flows
- `architecture/client.md`: Remote TUI client, resume handshake, and replay-aware event handling
- `architecture/server.md`: WebSocket TUI server, replay buffer, and REST/SSE routes
- `architecture/skills.md`: Runtime skill loader plus the repo-maintained `.skills/` copy
- `architecture/native_crates.md`: Workspace crates (egglsp, egggit, eggsentry, eggcontext, codegg-config, codegg-protocol, codegg-providers), backend contract, raw MCP exposure policy, diagnostics
- `architecture/lsp.md`: LSP client, diagnostics, code operations, preview-only semantic edits, temporary overlays (semanticCheckPreview), capability discovery and normalization, diagnostics cache lifecycle with freshness metadata, capability-gated operations, diagnostic evidence in context packets, shared semantic context API, remote/core ownership model
- `architecture/git.md`: Git session management, git info injection, worktree per session (now in `crates/egggit`; worktree is read-only in codegg-core, mutating operations removed)
- `architecture/goal.md`: Goal runtime, budget enforcement, auto-continuation, TUI status bar
- `architecture/auth.md`: Typed AuthConfig, Credential, AuthResolver, user-level credential store, ExternalCommand safety, OAuth scaffolding, and CLI surface (`codegg auth ...`) — auth types now live in `codegg-providers`
- `architecture/context-ledger.md`: Context artifact storage, tool-output projection, ContextLedgerState, context_read tool, and config options
- `architecture/cache-aware-context.md`: Cache-aware context packing, tier-based block ordering, ContextPacker algorithm, ContextBlockBuilder, cache stats, and config (hardened: observe-only, stable hashes via stable_hash_hex, source_handle on ContextBlock, multi-phase observation via observe_context_pack, cache stats wired from provider finish events via `record_context_cache_stats_from_processor` with normalization; active mutation disabled); gated context policy layer (ContextPolicyConfig + tool-palette reduction prototype) added in policy.rs, wired per-request in AgentLoop (strictly gated, defaults disabled/observe); volatile-tail compaction (`volatile_tail.rs`) for late-context-only compaction of old tool-result messages

## Critical Implementation Notes

These items are important for future agents to know when working with the codebase:

### Implementation Patterns

- **PermissionRegistry/QuestionRegistry are synchronous**: `register()`, `respond()`, `answer_question()` are `fn`, not `async fn`. Do NOT use `await` when calling these.

- **Registry Limitations**: `PermissionRegistry` and `QuestionRegistry` do NOT store `session_id` in their keys. Permission IDs are in format `{tool_call_id}-{tool_name}`, not `{session_id}-...`. This means `get_pending_permissions_for_session()` and `get_pending_questions_for_session()` cannot properly filter by session_id without code changes.

- **Error module split**: Pure error enums live in `crates/codegg-core/src/error.rs`. Root `src/error.rs` re-exports from codegg-core plus `AxumAppError`/`AxumServerRuntimeError` wrappers behind `#[cfg(feature = "server")]`.

- **protocol_conversions split**: Core conversions (session, message, provider, config) live in `crates/codegg-core/src/protocol_conversions.rs`. Agent-specific conversions remain in root `src/protocol_conversions.rs`. Root re-exports core conversions via `pub use codegg_core::protocol_conversions::*;`.

- **PermissionDecision vs PermissionChoice**: `PermissionDecision` is the bus-owned DTO in `crates/codegg-core/src/bus/mod.rs`. `PermissionChoice` is the domain type in `src/permission/mod.rs`. Bidirectional `From` impls allow conversion. The `PermissionRegistry` API uses `PermissionDecision`; use it when calling `respond()` or registering permissions through the bus.

- **Tool factory**: `src/tool/factory.rs` provides `build_session_tool_registry()` which consolidates tool construction (base registry + task tool + goal tools). Used by `core/daemon.rs` to reduce direct tool module coupling.

- **Agent runtime factory**: `src/agent/runtime_factory.rs` provides `build_agent_loop()` which consolidates agent loop construction (permission checker + AgentLoop::new + session/subagent configuration). Used by `core/daemon.rs` to reduce direct agent/permission module coupling.

- **CoreRuntimeDeps**: `CoreDaemon` stores a single `deps: CoreRuntimeDeps` field instead of separate `subagent_pool`, `memory_store`, `bg_scheduler` fields. `subagent_pool` and `bg_scheduler` are grouped under `legacy_agent: LegacyAgentRuntimeDeps`. Legacy `new()` constructor kept for backward compat; new code should use `with_deps()`.

- **Hunk source navigation**: `hunkSourceContext` is a read-only LSP tool operation that maps unified diff hunks to enclosing symbols, diagnostics, definitions, references, and hierarchy data. It consumes `SemanticContextResponse` via the pure `HunkSourceNavigator` (no direct LSP calls). Diff parsing is in `src/lsp/hunk_nav_parser.rs`, range primitives in `src/lsp/hunk_nav_ranges.rs`, navigator in `src/lsp/hunk_nav.rs`, collector in `src/lsp/hunk_nav_collector.rs`. Policy in `src/lsp/hunk_nav_policy.rs` (`HunkSourceContextPolicy` + `decide_hunk_source_context()`); agent-facing summary in `src/lsp/hunk_nav_prompt.rs` (`format_hunk_source_context_summary()`). DTOs are in `crates/egglsp/src/hunk_context.rs`. Diagnostic line indexing is 0-based internally (`FileDiagnostic.line`) and converted to 1-based for hunk ranges via `diagnostic_to_range()`. Malformed hunk headers (`@@ bad header`) return `InvalidHunkHeader` errors. Multi-file patches are rejected with a clear error listing mismatched file paths. `normalize_diff_relative_path()` strips `a/`/`b/` diff prefixes and rejects path-traversal, `RootDir`, and `Prefix` components. `normalize_request_relative_path()` canonicalizes paths against the allowed root via `Path::canonicalize()` and rejects paths outside the root or resolving to the root itself. Errors are propagated from the collector's `collect()` method via `.map_err()`. Tests use real `TempDir` fixtures for canonical containment verification. Truncation flags use raw pre-cap counts (`> max`, not `>= max`). Semantic context is collected centered on the first hunk and shared across all hunks (documented in response notes when multiple hunks are present). The `hunks` field on `HunkSourceNavigationRequest` is internal DTO support; the model-facing tool schema exposes only `patch` for unified diff input. The security review workflow executes `hunkSourceContext` via the `HunkSourceContextExecutor` trait (`src/security/workflow/context.rs`) with `LspHunkSourceContextExecutor` (`src/security/lsp_executor.rs`) as the real adapter calling `LspTool::execute_hunk_source_context_typed()` directly with a typed `HunkSourceNavigationRequest` — no JSON round-trip. Internal pre-parsed hunk descriptors are used via the typed API; the model-facing tool schema remains patch-only. Policy decisions are routing metadata, never security evidence — only real `HunkSourceNavigationResponse` produces `HunkNavigation` evidence. The collection phase provides deterministic routing, ordering, and bounded invocation; best-effort, server-dependent LSP evidence; fail-open execution. `HunkSourceContextExecutionStats` tracks request outcomes (attempted/succeeded/failed/timed_out) per `collect_hunk_source_context_all_files` call. Request caps count actual executor calls, not loop position.

- **TurnRuntime**: Daemon calls `DefaultTurnRuntime.run_turn(TurnRunInput)` instead of building tool registries, permission checkers, and agent loops inline. `TurnRuntime` owns the full turn lifecycle: provider resolution, tool registry construction, system prompt assembly, agent loop construction, and background spawning. `AgentLoopFactory` (build-only) is kept as a transitional internal detail.
- **Daemon TurnSubmit ownership**: Daemon still owns request validation, session_id/turn_id management, active-turn bookkeeping, and TurnStarted event publishing. Runtime owns everything else.

- **TaskToolRuntime**: `tool::factory::build_session_tool_registry` takes `Option<&TaskToolRuntime>` instead of `Option<&Arc<SubAgentPool>>`. This breaks the tool factory's direct dependency on `SubAgentPool`.

- **MCP reconnect wired up**: Heartbeat failures now trigger reconnect via `reconnect_needed` Notify mechanism

- **MCP DNS re-validation**: `RemoteClient::initialize()` re-validates DNS on each call (connect/reconnect), keeping `validated_ips` current

- **MCP ensure_connected()**: Clones all fields before `tokio::spawn` to avoid borrow-after-spawn issues

- **`/security-review` async dispatch + result panel**: The TUI handler no longer calls `tokio::task::block_in_place` + `Handle::current().block_on(...)`; it spawns a tokio task and the result surfaces via the message timeline (a new `UIMessage` with `MessageRole::Assistant` and a `[Security Review]` label) plus a brief success/error toast. The TUI render thread is never blocked. The `App.security_review_running: Option<SecurityReviewTaskState>` reentrancy guard holds `{ id, abort_handle }`; it is set when the run is dispatched and cleared in both success and failure paths. A second `/security-review` while the guard is set is rejected with a warning toast: "Security review already running. Wait for it to finish or cancel it." On success the structured `SecurityReviewReceipt` is stored on `App.latest_security_review` (via `App::set_latest_security_review` at `src/tui/app/mod.rs:914`) so it can be reopened via `/security-review-show`. The `--panel` flag on `/security-review` auto-opens the result panel on completion. Default completion behavior sends the report to the timeline; the panel is opened on demand via `/security-review-show` or with `--panel`. Receipt persistence is in-memory only (no database persistence; cleared on app restart) — the rationale is that receipts are large rendered artifacts tied to a specific run and the cost of SQLite serialization/deserialization outweighs the benefit of cross-restart persistence. The only thing that crosses the dispatch boundary from the App is a cloned `Arc<LspTool>` — no borrowed `&self` survives the await, so the dispatcher can own the inputs and call `run_security_review_background()` in a spawned task. See `src/tui/app/mod.rs:4243-4247` for the spawn-with-AbortHandle wiring.

- **`/security-review` cancellation and stale completion**: `/security-review-cancel` aborts the running task via `App::cancel_security_review` (`src/tui/app/mod.rs:936`), which calls `AbortHandle::abort()` and clears the guard immediately; a "Security review cancelled." toast confirms. Cancellation is best-effort: if the spawned task is in a non-cancellable section (e.g. inside a blocking syscall), its completion may still arrive. The completion handler in `src/tui/mod.rs:2205` (`handle_security_review_finished`) guards against stale completions by comparing the incoming `id` against `app.security_review_running.id` via `App::security_review_run_id`; mismatches are silently dropped so a cancelled run cannot reinstate its guard or push a stale receipt.

- **`/security-review-show` reopens the latest receipt**: A read-only command that opens `Dialog::SecurityReview` with `App.latest_security_review`. It does NOT rerun the review. If no receipt exists yet, a "No security review result available yet." warning toast is shown. Wired in `src/tui/command.rs:215` with `Some(Dialog::SecurityReview)` so command-mode completion opens the dialog.

- **Protocol is now a re-export**: `src/protocol/` has been deleted. `src/lib.rs` uses `pub use codegg_protocol as protocol;`. The `codegg-protocol` crate is the single source of truth for `CoreRequest`, `CoreResponse`, `CoreEvent`, `TuiMessage`, and frame types. Root code uses DTO types from `codegg_protocol::dto` when constructing protocol messages; conversions between domain types and DTOs are in `src/protocol_conversions.rs`.

- **TUI render.rs doesn't exist**: Only `mod.rs`, `types.rs`, and `commands.rs` exist in `src/tui/app/`

- **Component trait**: All dialogs implement `Component` trait with `handle_key`, `update`, `render` methods

- **Registration-before-publish pattern**: When publishing `PermissionPending` or `QuestionPending`, register the responder BEFORE publishing the event

- **ResyncRequired serialization**: Server uses `TuiMessage::ResyncRequired` variant directly (not raw JSON)

- **Client timeouts**: Health check has 10s timeout, WebSocket connection has 30s timeout

- **TTS is macOS-only**: Currently uses hardcoded `say` command in `src/tts/mod.rs`. TTS auto-stops when `AgentFinished` is received. Toggle with `/tts` or `/voice` command.

- **Subprocess PATH**: All tools use `std::env::var_os("PATH")` instead of hardcoded paths for proper Homebrew/cargo/pyenv tool discovery

- **Plugin fuel tracking**: `fuel_reserved` set at `loader.rs:270` is returned via `module_cache::CACHE.return_fuel()` on normal exits. Early error returns at `loader.rs:255-285` also correctly return fuel.

- **handle_remote_event location**: `src/tui/app/mod.rs:794` - not in client module

- **IdeServer async I/O**: `run_stdio()` uses `tokio::io::stdin()/stdout()` with async I/O, not blocking `std::io`

- **ToolCatalog::register() takes `&dyn Tool`**: Not `Box<dyn Tool>`. Document in architecture but often overlooked.

- **patch_util.rs shared utilities**: `src/tool/patch_util.rs` contains shared patch utility functions used by both `apply_patch` tool and LSP preview operations. Extracted to avoid duplication between the mutating `apply_patch` tool and the read-only `WorkspaceEditPreview` path.

- **Capability-gated LSP operations**: `semanticContext` and `securityContext` check `LspCapabilitySnapshot` before optional expensive LSP calls (definitions, references, call hierarchy, type hierarchy). When a capability is unsupported, the operation is skipped and an error/note is appended instead of failing. When no snapshot is available (server not initialized), operations default to attempting the call (fail-open). `DiagnosticEvidenceMeta` carries shared freshness metadata for semantic/security context packets, with `usable_evidence` indicating whether diagnostics are reliable. `securityContext` reuses the same diagnostic evidence and capability snapshot, but still applies its own security filtering and notes.
- **Semantic context consolidation**: `SemanticContextResponse` (in `egglsp`) is the internal semantic read model. `SemanticContextCollector` (`src/lsp/semantic_context.rs`) assembles the shared semantic evidence for source excerpt, diagnostics, symbols, definitions, references, source-action hints, and hierarchy summaries. `SemanticContextPacket::from_semantic_response()` adapts the shared response into the tool-local presentation packet for `semanticContext`. Overlay translation remains handler-local by design: the handler resolves overlays via `semanticCheckPreview` and attaches the summary, because patch/content expansion is tool-specific. The `semanticContext` handler forwards `include_call_hierarchy` and `include_type_hierarchy` flags into the `SemanticContextRequest` so the collector populates hierarchy data when requested. Source-action hints (e.g. `source.organizeImports`) are collected handler-locally by `LspTool::collect_source_action_hints`, not by the shared collector, because they produce `WorkspaceEditPreview` payloads that are preview-rich and tool-specific. `securityContext` requests shared call hierarchy from the collector when `include_call_hierarchy` is enabled and a target position is supplied; it does not consume the full packet, but reuses the shared diagnostic freshness evidence and capability snapshot from the common LSP path. Security call expansion (`call_depth > 0`) remains separate from the shared compact call hierarchy — it performs its own recursive BFS expansion handler-locally via `build_call_expansion_summary`, while the shared call hierarchy provides only immediate incoming/outgoing relationships.

- **LSP Phase 2 integration tests**: `crates/egglsp/tests/` now owns the stdio integration surface. `production_protocol_stdio.rs`, `production_semantic_stdio.rs`, and `production_service_stdio.rs` use `ProductionClientHarness`, while `scenario_engine.rs` includes the fake-server self-tests (inlined, no external `include!`). `egglsp::test_support` is feature-gated behind `lsp-test-support` and `#[doc(hidden)]`; the root `lsp-test-support` feature forwards to `egglsp/lsp-test-support`. `base64` and `libc` are optional dependencies gated on `lsp-test-support`. The fake LSP server binary is built as the `codegg-lsp-test-server` bin target from the `egglsp` package. Scenario JSON is delivered via `CODEGG_FAKE_LSP_SCENARIO` env var, transcripts via `CODEGG_FAKE_LSP_TRANSCRIPT`. Cargo exposes the binary to tests via `CARGO_BIN_EXE_codegg-lsp-test-server`, with `EGGLSP_TEST_SERVER` as an override for manual or CI use. Tests exercise real stdio transport through `egglsp::launch::spawn_server` and `LspWriter`. Root-crate composite tests in `tests/lsp_composite_stdio.rs` (24 tests) exercise `SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`, and security context tool orchestration against the fake server via the production `LspClient`/`LspService` stack — these bridge the gap between `egglsp`-only tests and the real root-crate collectors. The fake server supports captured-ID mode for genuinely out-of-order concurrent responses. All integration tests use bounded condition waits (polling loops) instead of fixed sleeps. Typed hierarchy methods on `LspClient` (`prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`, `prepare_type_hierarchy`, `supertypes`, `subtypes`) are used in hierarchy tests.

- **LSP Phase 3 operational health**: `LspOperationalState` enum with `transition()` state machine. `context_note()` returns bounded notes for semantic/security/hunk context responses. `LspProcessExitEvent` is the authoritative exit signal. `StderrRingBuffer` caps at 100 lines / 64KB. `OpenDocumentRegistry` tracks documents for restart replay. New `LspError` variants: `ServerRestarted`, `ServerUnavailable`, `ServerDegraded`. Per-client generation is tracked in `generation_map: Arc<Mutex<HashMap<String, u64>>>`; all state transitions go through `transition_operational_state()` (validated by `health::transition()`); stale exit events whose generation doesn't match the current authoritative value are silently dropped. Health snapshots remain available during `RestartScheduled` / `Restarting` / `Failed` / `Stopped` states and carry `transport: Option<...>`, `last_error`, `stderr_tail`, real `last_message_age_ms` / `last_diagnostics_age_ms`, and `restart_attempts`.

- **LSP process runtime owner**: `LspProcessRuntime` is the single authoritative process owner; the runtime task owns the child handle, stderr ring buffer, intent receiver, and kill channel. The monitor does NOT retain an `Arc<LspClient>` while waiting on the child. `LspProcessIntent` (Running / GracefulShutdownRequested / ForceKillRequested) is the source of truth for expected-vs-unexpected exit classification — transport state never determines expectedness. A zero exit with no shutdown intent is still unexpected. Hung processes are force-killed and reaped under a bounded deadline. `LspClient::shutdown()` sets graceful intent, sends `shutdown`/`exit`, awaits runtime exit under a deadline, then force-kills on timeout.

- **LSP restart coordinator**: `restart_client_coordinator<S: RestartShared, F>` is the single source of truth for restart retry/backoff/exhaustion/cancellation. It owns generation increment, restart-state transition, current-client removal, old runtime shutdown, retry/backoff loop, client reinitialization from `LspClientDescriptor`, readiness wait, document replay, ownership restoration, diagnostics stale marking, and final ready/failed transition. `LspServiceClone` was removed; the duplicate `restart_client` paths were merged. `LspClientDescriptor` persists the per-client launch spec (server_id, root, launch_spec, initialization_options, workspace_configuration, readiness_policy, restart_policy, seed_file) so restart reconstructs from real data, not a hard-coded `src/lib.rs` path.

- **LSP per-client generation**: `generation_map: Arc<Mutex<HashMap<String, u64>>>` provides a monotonically increasing generation per client key (starts at 1 on first publish, bumped by the restart coordinator after successful reinit + replay). `LspService::generation_for_key(key)` and `LspService::set_generation(key, gen)` are the public accessors. Exit events whose `event.generation != current_generation` are rejected as stale — old exit events cannot fail a newer client. Restart publication rechecks the expected generation before publishing and aborts with `LspError::ServerRestarted` if a newer generation is observed.

- **LSP diagnostic generation / post-restart**: `DiagnosticCacheEntry` carries `server_generation: u64` (0 is the "never assigned" sentinel) and `post_restart: bool` (true when the entry arrived from a post-restart client; monotonically sticky). `LspDiagnosticSnapshot` exposes both fields. On restart, `LspService::mark_diagnostics_stale_for_key(key)` re-keys retained diagnostics to `current - 1` so the freshness classifier returns `LspDiagnosticFreshness::Stale` until the new server emits its first push. The root-side `SemanticContextCollector` propagates these fields to `SemanticDiagnosticEvidence`. `post_restart` semantics: true if evidence was received from the current generation after at least one restart.

- **LSP readiness policy**: `LspReadinessPolicy` is a 4-variant enum: `InitializedIsReady`, `WaitForDiagnosticsOrTimeout { timeout }`, `WaitForProgressEndOrTimeout { timeout }`, `WarmupDelay { duration }`. `LspService::wait_for_readiness(key, policy)` honors all four and returns `ReadinessResult::Ready { elapsed }` or `ReadinessResult::Degraded { reason, elapsed }`. The four variants drive the production `Indexing` → `Ready` and timeout → `Degraded` transitions. `LspClient` tracks `ProgressState` (active progress tokens + last progress timestamp) and exposes `progress_snapshot()`, `wait_for_progress_end(timeout)`, `wait_for_first_diagnostics(timeout)`, and `operational_summary()`.

- **LSP operational notes propagation**: `LspOperationalState::context_note()` returns `None` for `Ready` and a bounded `Some("LSP state: ...")` for every other state. The note is appended to `SemanticContextResponse.notes`, `SecurityContextPacket.notes`, and hunk source context summary lines so root workflows expose the operational state explicitly. Restarting/failed/degraded states are not silently treated as ready.

- **LSP Phase 3 final closure (11-pass)**: `runtime_map` is now `HashMap<String, RuntimeEntry>` with an explicit `generation: u64` field. Helpers: `install_runtime` (rejects same- or newer-generation replacement), `runtime_for_generation` (lookup), `remove_runtime_if_generation` (deletion). A delayed old monitor cannot remove a newer generation's runtime. The runtime termination sequence is `LspProcessIntent::GracefulShutdownRequested` (set BEFORE the protocol shutdown) → `request_protocol_shutdown` → `wait_for_exit` under graceful deadline → `LspProcessIntent::ForceKillRequested` + `start_kill` on timeout → `wait_for_exit` under absolute deadline. The sequence is recorded in `LspProcessExitEvent` and the runtime is removed only if the stored generation still matches. `RuntimeTerminationReason` is `ServiceShutdown` / `ManualRestart` / `FailedPublication`. `LspService::next_generation_for_key` is the single source of truth for replacement generation; the reinit closure receives the generation as an argument (`FnMut(&LspClientDescriptor, u64) -> BoxFuture<...>`) and never derives it independently. `LspService::manual_restart_client(key)` bypasses `LspRestartMode::Disabled`, terminates the old runtime with `RuntimeTerminationReason::ManualRestart` first, then proceeds; a manual restart supersedes an in-flight automatic restart. The `restart_attempts` counter is shared across rapid crash cycles and resets only after `reset_after_healthy` of continuous health; `LspService::increment_restart_attempts` is called once per actual replacement spawn, `LspService::set_last_healthy_now` updates the timestamp, and `LspService::reset_restart_attempts_if_healthy_inherent` lazily resets the counter when the next unexpected exit observes the interval. Old diagnostics survive as stale evidence: `LspService::snapshot_diagnostics_for_restart(key)` captures the old cache, `LspClient::install_retained_diagnostics(_source, entries)` preserves `server_generation` and `post_restart` flags, and a new `publishDiagnostics` from the new generation overwrites retained entries. `post_restart = generation > 1` is enforced uniformly in `LspClient::bind_server_generation` and `DiagnosticCacheEntry::with_generation`. `LspClient::wait_for_progress_end(timeout) -> bool` requires `ProgressState.completed_cycle == true` — empty `active_tokens` alone is not sufficient. `LspRestartPolicyConfig::try_to_domain` validates user overrides (rejects zero attempts when mode is `OnUnexpectedExit`, rejects initial > max backoff, rejects duration overflow). `LspProcessRuntime::stderr_tail_capped(max_lines)` returns recent stderr for the real-server harness; zero-location references fail `RequiredIfAdvertised` checks. `LspService::new` is the bare test-only constructor; `LspService::new_arc` is the only public production constructor and wires `self_ref` via `Arc::new_cyclic`. The `generation_is_identical_across_health_and_exit_event` supervisor test now writes the gen-3 scenario only AFTER gen-2 is observed Ready (Pass 11 test-timing fix).

- **Dialog::Info doesn't exist**: Despite `src/tui/components/dialogs/info.rs` existing, `Dialog::Info` is NOT in the Dialog enum at `types.rs:2-25`.

- **DialogType is in component.rs**: Not in `types.rs`. FocusManager is in `component/focus.rs`.

- **AgentLoop has 49 fields**: The struct at `src/agent/loop.rs:1380` has 49 fields (added base_request_tools + context_policy_runtime for hardened tool-palette policy); many docs list only 15.

- **Exec mode question behavior**: `setup_question_channel_for_exec()` at `src/exec.rs:121` DOES set `question_rx`, meaning exec mode waits up to 300s before timing out. The "[question not supported]" string is in the `else` branch (non-exec path when `question_rx` is None).

- **Goal and todo are two separate surfaces**: Goals are long-horizon, multi-session, durable, autonomous. Todos are in-flight, per-turn, ephemeral. They form a hierarchy: a goal spans many sessions; each session may have todos as steps toward the goal. The system prompt steers models toward todos for in-flight planning and `goal_request_completion` for long-horizon work.

- **Goal budget accounting is atomic**: `GoalStore::increment_usage()` checks all four budget axes in a single transaction. If any axis is exceeded, the goal transitions to `BudgetLimited` atomically. The agent loop then queues a wrap-up prompt on the next turn.

- **Goal wall-clock is durable**: `wallclock_secs` is persisted in SQLite, so time spent on the goal across session restarts is accurately tracked. `GoalWallClock` in the agent loop tracks wall-clock deltas between accounting ticks.

- **Context policy (first active, gated, now hardened Phases 1-8)**: `[context_policy]` section + ContextPolicyMode (Observe|Warn|ToolPaletteReduce) + ContextPolicyConfig (incl. review_tool_palette_threshold); src/context/policy.rs (decide_policy + reduce_tool_palette deterministic, now with would_* for Warn dry-run + `detect_palette_starvation()` pure helper); reductions always derive from unreduced `base_request_tools` (full profile-filtered palette captured once per run after model-profile filter) in AgentLoop; non-cumulative/stateless per provider call (noop or backoff can restore full base); `request.tools=None` respected and never re-enabled; ContextPolicyRuntimeState (backoff `reduction_disabled_until_turn`, consecutive_reductions, last_* counters/names); starvation detection via `detect_palette_starvation()` (pure, testable) + `AgentLoop::observe_tool_palette_starvation()` (wired at both parse sites): if name in base but not last_selected (only base-present tools), set backoff + warn; starvation never blocks the tool call — only disables reduction for next provider call; backoff triggers (empty selected fallback, starvation) logged with `policy_backoff_active`/`reduction_disabled_until_turn`; Warn performs dry-run reduce when base passed and populates would_selected/omitted counts (logs include would_select/would_omit); `review_tool_palette_threshold=false` gates ReviewToolPalette trigger in decide_policy; diagnostics (info when log_policy_decisions): base_tool_count/selected/omitted/cap_exceeded_by_required/policy_backoff_active/reduction_disabled_until_turn + debug names/overflow; wired only to per-request tools before provider observes; defaults safe (disabled/observe). Active mutation of packer itself remains disabled. See architecture/cache-aware-context.md.

- **Volatile-tail compaction**: `[context_policy]` section with `volatile_tail_compaction` (bool, default false), `volatile_tail_mode` (observe|warn|compact, default observe), `min_volatile_tokens_for_compaction` (12000), `preserve_recent_messages` (12), `max_compacted_tail_tokens` (8000), `require_effective_cost_signal` (true), `compact_tool_results_only_first` (true). Lives in `src/context/volatile_tail.rs`. Only compacts old volatile tool-result messages with recovery handles (`ctx://` source_handle). Tombstone format: `[compacted volatile tool result]` with `original_estimated_tokens`, `reason`, `recovery_handle`. Idempotent — already-compacted messages are skipped. Preserves stable prefix, system prompts, user messages, assistant messages with tool calls, and recent messages. Rollout: observe → warn → compact (all disabled by default).

### Known Issues (Lower Priority)

| Issue | Location | Status |
|-------|----------|--------|
| **TTS init ignores providers** | `src/tts/mod.rs:45-49` | Known issue - macOS say adequate |
| **Static CANONICAL_PATHS_CACHE** | `src/security/sandbox.rs:262` | Has 300s TTL + 100-entry cap now |
| **OAuth replay protection TOCTOU** | `src/mcp/auth.rs:318-332` | Known issue |
| Real-server tests are opt-in only | `crates/egglsp/tests/real_server_smoke.rs` | Phase 3 - Tier 1 only |

### Key Lessons from Review Sessions

1. **Always verify documentation claims against actual code** - Many "bugs" in review files turned out to be correctly implemented after direct inspection.

2. **Documentation can become stale** - Struct fields get added/removed; always compare architecture docs against actual source code.

3. **Counts should be verified** - Component/dialog counts (TUI), server counts (LSP), command counts can drift from reality. When fixing documentation, count from actual source files, not from other documentation. **UiState has 27 fields** (not 25 as some docs claim). `timeline_visible` and `timeline_selected` are in `UiState` struct (lines 74-76), NOT `App` struct.

4. **Line numbers in docs are fragile** - References like `watcher.rs:157-158` should be verified; they can be off by several lines. Use code search to find exact locations.

5. **Pre-verification before editing** - When a plan or review file claims "X is wrong in architecture doc", first check if it's been fixed since the review was written. Many "corrections" in old plans were already addressed.

6. **Use subagents for batch review work** - Process 4-5 plan files per subagent (2000 line context limit), consolidate results, then consolidate into final plan.

7. **multiedit tool exists but not in default registry** - `src/tool/multiedit.rs` exists and `multiedit` module is registered via `pub mod multiedit`, but it's NOT included in `ToolRegistry::with_defaults()`. Don't assume every tool in `/tool` is in the default registry.

8. **LSP server count is 39** (verified 2026-05-27) - count entries in `server_definitions()` array at `crates/egglsp/src/server.rs` (moved from `src/lsp/server.rs:27-383`). cmake-language-server is NOT in the list despite some review claims. clangd, rust-analyzer, gopls, etc. are included.

9. **Permission mode documentation** - `architecture/permission.md:202` (docs mode) shows restricted tools as `bash, task` (without `write`). Code at `modes.rs:220-227` shows docs mode restricted_tools as `bash, terminal, git, commit, task, image` (6 tools). Verify against actual code.

### Verified Codebase Facts

These items were verified during review sessions:

| Item | Value | Location |
|------|-------|----------|
| Tool count | 27 | `src/tool/mod.rs:90-122` (27 registrations in with_defaults()) |
| LSP server count | 39 | `crates/egglsp/src/server.rs` (moved from `src/lsp/server.rs:27-383`) |
| LSP capability snapshot | `LspCapabilitySnapshot` | `crates/egglsp/src/capability.rs` — normalized boolean view from `ServerCapabilities` |
| LSP semantic operation enum | `LspSemanticOperation` | `crates/egglsp/src/capability.rs` — Diagnostics, DocumentSymbols, WorkspaceSymbols, Definition, References, Hover, Completion, CallHierarchy, TypeHierarchy, SemanticTokens, SecurityContext |
| LSP unavailable response | `LspUnavailable` | `crates/egglsp/src/capability.rs` — structured fallback for unsupported operations |
| LSP diagnostics snapshot | `LspDiagnosticSnapshot` | `crates/egglsp/src/diagnostics.rs` — freshness metadata (Fresh, PossiblyStale, Stale, Unavailable) |
| Semantic context API | `SemanticContextRequest`/`SemanticContextResponse` | `crates/egglsp/src/semantic_context.rs` — domain-agnostic semantic queries with intent enum; `SemanticContextResponse` is the internal semantic read model |
| Semantic context collector | `SemanticContextCollector` | `src/lsp/semantic_context.rs` — assembles the shared semantic read model, produces `SemanticContextResponse` from LSP services |
| Semantic context adapter | `SemanticContextPacket::from_semantic_response()` | `src/tool/lsp.rs` — adapts shared response into tool-local presentation packet |
| `capabilities` LspTool operation | `capabilities` | `src/tool/lsp.rs` — returns `LspCapabilitySnapshot` for the file's server |
| LSP bidirectional peer | Yes | `crates/egglsp/src/server_request.rs`, `crates/egglsp/src/client.rs` |
| Server request dispatcher | `dispatch_server_request` | `crates/egglsp/src/server_request.rs` |
| Dynamic registration state | `DynamicRegistrationState` (256 cap) — processes full arrays with validation and deduplication | `crates/egglsp/src/server_request.rs` |
| Shared serialized writer | `LspWriter<W>` | `crates/egglsp/src/writer.rs` |
| Single-flight initialization | `InitSlot` pattern with explicit leader/waiter election and shared completion fan-out, oneshot channels, attempt IDs; `SharedInitError` for cloneable concurrent error propagation; `#[cfg(test)]` injectable test factories; failure cleaned up by attempt ID allowing retries | `crates/egglsp/src/service.rs` |
| egglsp-test-server bin target | `crates/egglsp-test-server/src/main.rs` (thin wrapper calling `egglsp::test_support::run_or_exit()`) | Fake LSP server binary for deterministic integration testing and scenario-engine self-tests. The scenario engine lives in `egglsp::test_support` module (feature-gated behind `lsp-test-support`, `#[doc(hidden)]`). Reads Content-Length framed JSON-RPC, executes scripted scenarios, writes machine-readable transcripts. Root tests use `codegg-lsp-test-server` (via `CARGO_BIN_EXE_codegg-lsp-test-server`). Scenario engine tests are inlined in `crates/egglsp/tests/scenario_engine.rs`. |
| Fake server captured-ID mode | Supports captured-ID for genuinely out-of-order concurrent responses | `crates/egglsp-test-server/src/main.rs` — enables deterministic testing of concurrent request handling without fixed sleeps |
| Root composite test count | 24 | `tests/lsp_composite_stdio.rs` — exercises `SemanticContextCollector`, `DiagnosticsCollector`, `LspOperations`, and security context tool orchestration (including call hierarchy error degradation, node-limit truncation, depth-limit enforcement, and diagnostic evidence filtering) against fake server via production stack |
| Typed hierarchy methods on LspClient | `prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`, `prepare_type_hierarchy`, `supertypes`, `subtypes` | `crates/egglsp/src/client.rs` — dedicated typed methods replacing manual JSON-RPC dispatch |
| Bounded wait pattern in integration tests | Polling loops instead of fixed sleeps | All integration tests in `crates/egglsp/tests/` and `tests/lsp_composite_stdio.rs` |
| Client-map read/write lock discipline | Non-mutating service methods use `clients.read().await`; write guards reserved for slot election/publication and shutdown drain; no guard held across client I/O | `crates/egglsp/src/service.rs` |
| Tracked initialization tasks | `active_init_tasks: HashMap<u64, InitTaskControl>` with `CancellationToken`, `AbortHandle`, and authoritative `oneshot::Receiver<InitTaskExit>` completion primitive; wrapper task owns the `Sender` and explicitly removes the entry before sending the terminal exit (primary path); `ActiveTaskGuard` fallback spawns a follow-up cleanup task on Drop (no `try_lock`); start-registration barrier (one-shot oneshot) gates the wrapper body until the `active_init_tasks` entry is installed; cooperative cancellation at download, spawn, initialize, initialized stages; shutdown awaits completion receivers via `await_init_task_completions` (no forwarding task ever wraps the real `JoinHandle`) | `crates/egglsp/src/service.rs` |
| Quiescent shutdown_all() | Concurrent cooperative cancel (signal all tokens simultaneously) → aggregate grace wait via `await_init_task_completions` over completion receivers (using `FuturesUnordered` with `tokio::select!` per control) → concurrent abort-and-await for stragglers through the same completion receiver path → concurrent ready-client drain via `futures::future::join_all` → notify `Cancelled` to slot waiters → transition to `Stopped`; driven by absolute deadline (`Instant::now() + 6s`) so total duration is independent of client count; defensive forced finalization (pathological deadline fallback) if deadline expires — service state is finalized with unresolved task completion logged as a severe invariant failure | `crates/egglsp/src/service.rs` |
| Concurrent shutdown callers | `await_stopped()` subscribes to `lifecycle_tx: tokio::sync::watch::Sender<LifecycleState>`; re-checks current state; awaits state changes until `Stopped`; race-free with no lost-wakeup window at the `ShuttingDown → Stopped` transition | `crates/egglsp/src/service.rs` |
| Unified terminal state | `InitTerminal` enum (Published/Existing/Invalidated/Failed/Cancelled/Panicked) with `finish_attempt()` cleanup helper | `crates/egglsp/src/service.rs` |
| Cancellation on timeout | best-effort `$/cancelRequest`; cancel write failure marks transport failed and drains pending | `crates/egglsp/src/client.rs` |
| Global map lock release | Arc clone before await | `crates/egglsp/src/service.rs` |
| workspace/applyEdit rejection | Always `applied: false` (application-level result, not JSON-RPC error) | `crates/egglsp/src/server_request.rs` |
| New LspError variants | `Protocol`, `WriterClosed`, `InitializationCancelled` | `crates/egglsp/src/error.rs` |
| Client transport state | `ClientTransportState` (`Running` or `Failed { reason }`) — writer failure propagation drains pending requests | `crates/egglsp/src/client.rs` |
| Client health snapshot | `LspClientHealthSnapshot` with typed `transport: ClientTransportSnapshot` and `pending_requests: usize`; `health_snapshot()` accessor | `crates/egglsp/src/client.rs:370-380` |
| Service lifecycle | `ServiceLifecycle` (`Running` → `ShuttingDown` → `Stopped`) — prevents new client acquisition after shutdown | `crates/egglsp/src/service.rs` |
| Lifecycle generation tracking | `LifecycleState { phase: ServiceLifecycle, generation: u64 }` — `shutdown_all()` increments generation; spawned init task rechecks phase/generation before publication and disposes unpublished clients on invalidation or lost publication | `crates/egglsp/src/service.rs` |
| SharedInitError | Cloneable error for concurrent initialization waiters; `SharedInitErrorKind` enum (ServerNotFound, DownloadFailed, LaunchFailed, InitializeFailed, Timeout, Cancelled, Protocol, Other); `From<&LspError>` and `into_lsp_error()` conversions | `crates/egglsp/src/error.rs` |
| InitSlot/InitSlotState/InitRole | InitSlot tracks attempt_id plus leader sender and waiter list; InitRole enum (Leader/Waiter); ATTEMPT_COUNTER for monotonic attempt IDs; compare-and-remove prevents stale cleanup; leader and waiters share the same completion result | `crates/egglsp/src/service.rs` |
| fail_transport() helper | Centralized transport failure: atomically transitions to `Failed` (idempotent), releases lock, then drains pending. Used for stdout EOF, request/notification write failures, and timeout-cancel write failures | `crates/egglsp/src/client.rs` |
| register_batch() | Atomic batch registration: pre-checks capacity before any mutation in DynamicRegistrationState | `crates/egglsp/src/server_request.rs` |
| is_structural_error() validation | Uses `as_i64().is_some()` to reject fractional JSON-RPC error codes | `crates/egglsp/src/client.rs` |
| Lock ordering documentation | Documented on `LspService` struct: clients map lock before client-level lock | `crates/egglsp/src/service.rs` |
| Document ownership | `document_owners` map (URI → client key) for O(1) deterministic ownership lookup in `close_file`/`save_file` | `crates/egglsp/src/service.rs` |
| LspProcessRuntime | Single authoritative process owner; owns child handle, stderr ring buffer, intent receiver, kill channel | `crates/egglsp/src/runtime.rs` |
| LspProcessIntent | `Running` / `GracefulShutdownRequested` / `ForceKillRequested` — source of truth for expected-vs-unexpected exit; `is_expected()` helper | `crates/egglsp/src/runtime.rs` |
| LspClientDescriptor | Persisted per-client launch spec (key, server_id, root, launch_spec, init opts, workspace config, readiness/restart policy, seed file); built by `LspClientDescriptor::from_profile()` | `crates/egglsp/src/restart.rs` |
| RestartTrigger | `Automatic` (honors `LspRestartMode::Disabled`) / `Manual` (always runs) | `crates/egglsp/src/restart.rs` |
| `restart_client_coordinator` | Single source of truth for retry/backoff/exhaustion/cancellation; honors `max_attempts`, `initial_backoff`, `max_backoff`, `reset_after_healthy`; emits `LspError::ServerRestarted` on stale generation | `crates/egglsp/src/restart.rs` |
| `LspService::new_arc` | Production constructor that wires the back-reference via `Arc::new_cyclic`; required for auto-activation of the exit receiver | `crates/egglsp/src/service.rs` |
| `LspService::ensure_exit_receiver_started` | Idempotent self-activation of the exit receiver; called from the first client-creating path | `crates/egglsp/src/service.rs` |
| `transition_operational_state` | Centralized operational state mutation; calls `health::transition()` validator | `crates/egglsp/src/service.rs` |
| `generation_map` | `Arc<Mutex<HashMap<String, u64>>>`; per-key generation; `generation_for_key` / `set_generation` are public accessors | `crates/egglsp/src/service.rs` |
| `ReadinessResult` | `Ready { elapsed }` / `Degraded { reason, elapsed }` returned by `LspService::wait_for_readiness` | `crates/egglsp/src/service.rs` |
| `LspOperationalHealthSnapshot` | `generation: u64` from `generation_map`, `transport: Option<...>`, `last_message_age_ms`, `last_diagnostics_age_ms`, `restart_attempts`, `last_error: Option<String>`, `stderr_tail: Vec<String>`; available without a live client | `crates/egglsp/src/health.rs` |
| `LspReadinessPolicy` | 4-variant enum: `InitializedIsReady` / `WaitForDiagnosticsOrTimeout` / `WaitForProgressEndOrTimeout` / `WarmupDelay` | `crates/egglsp/src/compatibility.rs` |
| `LspRestartPolicyConfig` | User-configurable restart via `[lsp.<server>.restart]` TOML section; overrides profile defaults; fields: `mode`, `max_attempts`, `initial_backoff`, `max_backoff`, `reset_after_healthy` | `crates/egglsp/src/config.rs`, `crates/codegg-config/src/schema.rs` |
| `CompatibilityRequirement` | 4-variant enum: `Required` / `RequiredIfAdvertised` / `Optional` / `KnownLimitation`; used by `assert_required_checks()` to fail tests on required regressions | `crates/egglsp/src/compatibility.rs` |
| `LspCompatibilityCheck.requirement` | New field binding the check to a `CompatibilityRequirement` for test-time assertion | `crates/egglsp/src/compatibility.rs` |
| `LspClient::progress_snapshot` / `wait_for_progress_end` / `wait_for_first_diagnostics` / `operational_summary` | Progress and diagnostics readiness observability; backs the 4 readiness policies | `crates/egglsp/src/client.rs` |
| `DiagnosticCacheEntry.server_generation` / `post_restart` | New fields; `0` sentinel for "never assigned"; `post_restart` is monotonically sticky | `crates/egglsp/src/client.rs` |
| `LspDiagnosticSnapshot.server_generation` / `post_restart` | Exposed to consumers; root `SemanticContextCollector` propagates to `SemanticDiagnosticEvidence` | `crates/egglsp/src/diagnostics.rs` |
| `mark_diagnostics_stale_for_key` | Service method invoked by the restart coordinator to re-key retained diagnostics to `current - 1` | `crates/egglsp/src/service.rs` |
| Supervisor/restart test count | 11 deterministic scripted scenarios in `supervisor_restart_stdio.rs`: graceful shutdown, unexpected exit + restart disabled, automatic restart success, init failure then recovery, exhaustion, shutdown cancels scheduled restart, stale exit event, replay uses latest content, hung process force kill, two consecutive restarts monotonic generations, generation identical across health and exit event | `crates/egglsp/tests/supervisor_restart_stdio.rs` |
| Real-server CI | Pins `rust-toolchain@1.81.0` (rust-analyzer job) and `basedpyright@1.13.1`; runs one server per matrix job; sanitizes artifact filenames | `.github/workflows/lsp-real-server.yml` |
| `LspOperationalState::context_note()` | Returns `None` for `Ready`; bounded `Some("LSP state: ...")` for every other state; appended to semantic/security/hunk context notes | `crates/egglsp/src/health.rs` |
| `RuntimeEntry` | Authoritative process runtime paired with the generation that installed it; `runtime_map` is now `HashMap<String, RuntimeEntry>` instead of `HashMap<String, Arc<LspProcessRuntime>>` | `crates/egglsp/src/service.rs:38-43` |
| `install_runtime` / `runtime_for_generation` / `remove_runtime_if_generation` | Generation-aware runtime-map helpers; replacement of same- or newer-generation entry is rejected at warn; delayed old monitors cannot remove newer runtimes | `crates/egglsp/src/service.rs:52-110` |
| `RuntimeTerminationReason` | `ServiceShutdown` / `ManualRestart` / `FailedPublication`; recorded in logs; identical termination path | `crates/egglsp/src/service.rs:115-123` |
| `terminate_runtime` | Service helper implementing the documented sequence: intent → protocol shutdown → wait → force-kill → reap; returns `RuntimeTerminationOutcome` | `crates/egglsp/src/service.rs:144-...` |
| `LspClient::request_protocol_shutdown` | Protocol-only shutdown (sends `shutdown`/`exit`); never waits on the child; `LspClient::shutdown` is the higher-level helper that combines this with the runtime termination sequence | `crates/egglsp/src/client.rs` |
| `LspService::next_generation_for_key` | Single source of truth for replacement generation; called exactly once per restart; reinit closure receives the generation as an argument | `crates/egglsp/src/service.rs:2162`, `crates/egglsp/src/restart.rs:252` |
| `LspService::manual_restart_client` | Public API bypassing `LspRestartMode::Disabled`; terminates the old runtime with `RuntimeTerminationReason::ManualRestart` before starting the replacement; supersedes in-flight automatic restarts | `crates/egglsp/src/service.rs:2440` |
| `LspService::increment_restart_attempts` | Increments the shared `restart_attempts` counter once per actual replacement spawn; called before invoking the restart coordinator | `crates/egglsp/src/service.rs:2856` |
| `LspService::set_last_healthy_now` | Sets `last_healthy_at` on `OperationalServerState` when readiness reaches `Ready`; the next lazy reset evaluates `last_healthy_at.elapsed() >= reset_after_healthy` | `crates/egglsp/src/service.rs:2871` |
| `LspService::reset_restart_attempts_if_healthy_inherent` | Lazily resets `restart_attempts` to 0 when the next unexpected exit observes the configured healthy interval; returns `Some(prev)` when the reset applies, `None` otherwise | `crates/egglsp/src/service.rs:2804` |
| `LspService::snapshot_diagnostics_for_restart` | Captures the live diagnostic cache for the old client (empty map when no client exists); passed to `LspClient::install_retained_diagnostics` for the new client | `crates/egglsp/src/service.rs:2885` |
| `LspClient::install_retained_diagnostics` | Installs retained diagnostic entries in the new client; preserves `server_generation` and `post_restart` flags; updates existing entries only when the incoming generation is newer | `crates/egglsp/src/client.rs:1905` |
| `LspClient::wait_for_progress_end` | Requires `ProgressState.completed_cycle == true`; empty `active_tokens` alone is insufficient | `crates/egglsp/src/client.rs:1770` |
| `ProgressState.completed_cycle` | New `bool` field on `ProgressState`; only flips to `true` when a full `begin`/`end` cycle is observed | `crates/egglsp/src/client.rs:404,414` |
| `LspRestartPolicyConfig::try_to_domain` | Validates user overrides: rejects `OnUnexpectedExit` with `max_attempts == 0`, rejects `initial_backoff_ms > max_backoff_ms`, rejects duration overflow; returns `LspError::InvalidConfig` | `crates/egglsp/src/config.rs:95` |
| `LspProcessRuntime::stderr_tail_capped` | Returns the most recent `max_lines` lines from the bounded `StderrRingBuffer` (100 lines / 64KB cap) in chronological order; used by the real-server smoke harness to populate `LspCompatibilityReport.stderr_tail` | `crates/egglsp/src/runtime.rs:140` |
| `LspService::new` (test-only, **Pass 7 crate-private**) | Bare constructor restricted to `pub(crate)` so production callers cannot create an un-supervised service; retained for tests that explicitly assert on the un-supervised path | `crates/egglsp/src/service.rs:787` |
| `LspService::new_arc` (production) | Only public production constructor; builds the service via `Arc::new_cyclic` and wires `self_ref` via `OnceLock::from(weak.clone())`; exit receiver auto-activates on first client-creating call | `crates/egglsp/src/service.rs:824` |
| Supervisor/restart test count | 11 deterministic scripted scenarios in `supervisor_restart_stdio.rs` + 5 Pass-1 ownership scenarios (`ownership_serializes_per_key`, `owner_safe_cleanup_does_not_drop_other_keys`, `lease_token_cancellation_aborts_coordinator`, `max_three_exact_spawn_count`, `pre_exhausted_budget_rejects_before_spawn`, `failed_init_consumes_attempt`) covering lease cancellation, exact spawn counts, and provenance preservation; harness enforces Pass 9 contract (coordinator must NOT call `mark_diagnostics_stale_for_key`) | `crates/egglsp/tests/supervisor_restart_stdio.rs` |
| Restart ownership primitives (Pass 1) | `acquire_restart_ownership` / `RestartLease` / `RestartTaskControl` / `RestartTaskMap`; `restart_client_coordinator` checks the lease token at every cancellation boundary (pre-backoff, mid-sleep, post-spawn, pre-publish, pre-replay) | `crates/egglsp/src/restart.rs` |
| `evaluate_references_check` (Pass 6) | Pure helper that turns a `Vec<Location>` into an `LspCompatibilityCheck`; rule: zero locations → `RequiredIfAdvertised` failure; unadvertised → `Unsupported`. `evaluate_references_check_with_min` adds a distinct-URI floor (used by the Python cross-file fixture) | `crates/egglsp/src/compatibility.rs` |
| Restart force-kill on deadline exhaustion (Pass 8) | `terminate_runtime` re-issues `runtime.request_force_kill()` on absolute deadline expiration and on early sender close; idempotent with the existing graceful-deadline force-kill. Guarantees the runtime's `LspProcessIntent` is `ForceKillRequested` whenever shutdown completes | `crates/egglsp/src/service.rs:259-298` |
| Native tool crates | 4 | `crates/egglsp`, `crates/egggit`, `crates/eggsentry`, `crates/eggcontext` — see `architecture/native_crates.md` |
| Extracted workspace crates | 4 (+1 new) | `crates/codegg-config`, `crates/codegg-protocol`, `crates/codegg-providers`, `crates/codegg-core` |
| Tool backend contract | `src/tool/backend.rs` | `ToolBackendKind`, `ToolProvenance`, `StructuredToolResult`, `build_report()` for `/tool-backends` |
| `/tool-backends` slash command | `src/tui/command.rs`, handler in `src/tui/app/mod.rs` | aliases: `/tools`, `/backends` |
| InprocCoreClient fields | All wrapped in `Option<Arc<...>>` except pool which is `Option<SqlitePool>` | `src/core/mod.rs:22-28` |
| CoreRuntimeDeps | Bundles pool, memory_store, legacy_agent (LegacyAgentRuntimeDeps grouping subagent_pool + bg_scheduler), turn_runtime (non-optional Arc<dyn TurnRuntime>) | `src/core/runtime_deps.rs` |
| AgentLoopFactory | Trait for agent loop construction seam | `src/agent/agent_loop_factory.rs` |
| TurnRuntime | Execution-oriented trait for turn lifecycle | `src/agent/turn_runtime.rs` |
| DefaultTurnRuntime | Default implementation building tools, permissions, prompt, agent loop | `src/agent/turn_runtime.rs` |
| Daemon direct agent refs | **0** (zero) | `src/core/daemon.rs` — acceptance target met |
| Daemon turn runtime injection | `deps.turn_runtime.run_turn()` | `src/core/daemon.rs:560` — no direct DefaultTurnRuntime construction |
| TaskToolRuntime | Narrow DTO for task tool construction | `src/agent/task_tool_runtime.rs` |
| Plugin fuel logic | Fixed - all early returns correctly return fuel | `src/plugin/loader.rs` |
| CoreEvent mapping | Complete - all events including Subagent* properly mapped | `src/core/mod.rs` |
| CommandRegistry location | Line 77 | `src/tui/command.rs:77` |
| UiState fields | 27 fields | `src/tui/app/state/ui.rs:40-92` |
| Subagent event types | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `crates/codegg-core/src/bus/events.rs:120-141` |
| CoreEvent has subagent variants | SubagentStarted, SubagentProgress, SubagentCompleted, SubagentFailed | `crates/codegg-protocol/src/core.rs:244-268` |
| map_app_event_to_core_event | All Subagent events mapped | `src/core/mod.rs` |
| SessionCompacting hook | IS dispatched in AgentLoop::compact_if_needed() | `src/agent/loop.rs:1216-1220` |
| hook_timeout vs WASM_HOOK_TIMEOUT | Outer 5s, inner 30s | `src/plugin/service.rs:18`, `src/plugin/loader.rs:14` |
| Backoff formula | `2^i` (no jitter) | `crates/codegg-providers/src/fallback.rs:107` |
| Client backoff formula | 1s, 2s (attempt 1, 2) | `src/client/attach.rs:39` — formula is `2^(attempt-1)`, only attempts 1 and 2 apply (attempt 0 is the first connection, no backoff) |
| Protocol version | 1 | `crates/codegg-protocol/src/core.rs:3` |
| AppEvent count | 41 | `crates/codegg-core/src/bus/events.rs:5-147` |
| Built-in command count | 63 (includes /tts, /pr, /issue, /checkpoint, /revert, /research, /research-runs, /research-open, /research-show, /doctor, /tool-backends, /security-review, /security-review-show, /security-review-cancel) | `src/tui/command.rs:84-219` |
| ToolDefCache | `(Option<String>, bool, bool, usize, u64, bool, Vec<ToolDefinition>, Vec<ToolDefinition>)` - model, plan_mode, lsp_enabled, mcp_count, perm_ver, exec_policy_enabled, definitions, deferred_definitions | `src/agent/loop.rs:64-73` |
| Timeline fields location | `timeline_visible` and `timeline_selected` are in `UiState` struct (lines 62-63), NOT `App` struct | `src/tui/app/state/ui.rs:62-63` |
| Snapshot hash | Uses SHA256 consistently | `crates/codegg-core/src/snapshot/mod.rs` |
| Git module | removed in 2026 native crate extraction; read-only facts now in `crates/egggit/` (`repo_status`, `diff_summary`, `changed_files`, `file_diff`, `validate_patch`, `list_worktrees`). Mutating worktree operations removed (worktree is now read-only in codegg-core). The git tool wrapper still lives at `src/tool/git.rs` | `crates/egggit/src/{lib,status,diff,worktree}.rs`, `crates/codegg-core/src/worktree.rs` |
| Pricing service | `src/util/pricing.rs` - ModelPricing, calculate_cost | `src/util/pricing.rs` |
| Auto-compact wrapper | Both `auto_compact()` and `auto_compact_sync()` exist | `src/agent/compaction.rs:550,594` |
| ImageTool | IS registered in ToolRegistry::with_defaults() | `src/tool/mod.rs:102` |
| Tool session constructor | `with_session_config_defaults(&Config, ...)` is the production session constructor; `with_session_defaults(...)` is the legacy all-native fallback (documented footgun for config-aware paths) | `src/tool/mod.rs:477,503` |
| `Tool::expose_in_definitions()` | Default `true`; `DisabledTool` overrides to `false`. Both `ToolRegistry::definitions()` and `AgentLoop::build_tool_definitions()` filter through it, so disabled/MCP-stub tools are hidden from the model but remain callable for diagnostics and `/tool-backends` | `src/tool/mod.rs:146`, `src/tool/disabled.rs:78` |
| Central native tool execution | `AgentLoop::execute_tool_calls` calls `ToolRegistry::execute_capture(name, input, ctx)` for native tool calls; provenance is recorded via `tracing::debug!` without changing the model-facing string output | `src/agent/loop.rs:3249` |
| Native tool execution context helper | `AgentLoop::build_tool_execution_context(tc, timeout_ms)` builds the `ToolExecutionContext`; `AgentLoop::resolve_native_backend(name)` resolves the `ToolBackendKind` (native by default, `Mcp` for `websearch`/`webfetch` when `[search].backend = eggsearch`, `BuiltinLegacy` otherwise) | `src/agent/loop.rs` |
| Structured execution smoke test | `tests/tool_structured_execution.rs` locks down `ToolProvenance` shape, disabled/MCP-fallback semantics, and definition visibility | `tests/tool_structured_execution.rs` |
| Live dispatcher wiring tests | `test_live_dispatcher_uses_execute_capture`, `test_live_dispatcher_passes_native_backend_in_context`, `test_live_dispatcher_model_output_shape_is_plain_string` prove the agent loop routes native calls through `execute_capture` and the model-facing tool content is a plain string (no provenance JSON) | `tests/agent_loop_harness.rs` |
| Dialog::Stats | EXISTS in Dialog enum | `src/tui/app/types.rs:21` |
| Dialog::SecurityReview | EXISTS in Dialog enum; opens the `SecurityReviewDialog` master/detail panel over `App.latest_security_review`. Constructed on demand in `App::open_dialog` (`src/tui/app/mod.rs:5379`). | `src/tui/app/types.rs:28` |
| `TuiMsg::SecurityReviewJump { path, line }` | Fallback path: copies `path[:line]` to the clipboard and surfaces a toast. Emitted by `SecurityReviewDialog` when `Enter` is pressed but `resolve_security_review_item_path` rejects the path (e.g. outside root, missing parent). Read-only — the file is never mutated. | `src/tui/app/types.rs:214`, `src/tui/app/mod.rs:2491` |
| `TuiMsg::OpenSourcePreview { path, line, origin_label }` | Primary Enter path: opens a read-only `SourcePreviewDialog` at the resolved absolute path, root-scoped via `resolve_security_review_item_path` in `receipt.rs`. Emitted by `SecurityReviewDialog` when `Enter` is pressed on a file-backed item and path resolution succeeds. The `origin_label` carries "Security Review Finding" or "Security Review Prompt" when opened from the security review dialog; `None` for other callers. The `SourcePreviewDialog` reads the file, highlights the target line, and shows ±10 lines of context. Read-only — the file is never mutated. | `src/tui/app/types.rs:220`, `src/tui/app/mod.rs:2481` |
| `Dialog::SourcePreview` | EXISTS in Dialog enum; opens a read-only source preview dialog. Constructed on demand in `App::open_dialog` via `TuiMsg::OpenSourcePreview`. | `src/tui/app/types.rs:29` |
| `SecurityReviewTaskState` | `pub struct { id: String, abort_handle: tokio::task::AbortHandle }` in `src/security/workflow/receipt.rs`. Held by `App.security_review_running` for the duration of a background review so the TUI can abort it via `/security-review-cancel` and so the completion handler can correlate results by id. `App::security_review_run_id()` exposes the id without leaking the handle. | `src/security/workflow/receipt.rs:301` |
| `App.latest_security_review` | `Option<SecurityReviewReceipt>` stored on `App`. Set by `App::set_latest_security_review` (`src/tui/app/mod.rs:914`) on successful review completion. Consumed by `/security-review-show` to reopen the result panel without rerunning the review. No database persistence; cleared on app restart. Receipts are large rendered artifacts — in-memory-only is intentional. | `src/tui/app/mod.rs:372` |
| `/security-review-show` command | Reopens the latest `SecurityReviewReceipt` via `Dialog::SecurityReview`. Registered with `Some(Dialog::SecurityReview)` in `CommandRegistry::new` so command-mode completion opens the dialog. If no receipt exists yet, surfaces "No security review result available yet." warning toast. Does NOT rerun the review. | `src/tui/command.rs:215`, `src/tui/app/mod.rs:4255` |
| `/security-review-cancel` command | Aborts an in-flight `/security-review` via `App::cancel_security_review` (`src/tui/app/mod.rs:936`) which calls `AbortHandle::abort()` and clears the guard. Idempotent: if no review is running, shows a "No security review is running." warning toast. Stale completions (id mismatch) are silently dropped by the completion handler in `src/tui/mod.rs:2205`. | `src/tui/command.rs:217`, `src/tui/app/mod.rs:936`, `src/tui/mod.rs:2205` |
| `open_panel_on_complete` field | `SecurityReviewCommandArgs.open_panel_on_complete: bool` — when true, the result panel auto-opens on completion (set by `--panel` flag). Default false. | `src/security/workflow/report.rs` |
| `Dialog::SourcePreview` / `DialogType::SourcePreview` | EXISTS in Dialog/DialogType enums; opens a read-only source preview dialog. Used by `TuiMsg::SecurityReviewJump` to show finding files. | `src/tui/app/types.rs` |
| `SourcePreviewDialog` | Read-only source preview dialog at `src/tui/components/dialogs/source_preview.rs`. Root-scoped; used for security review finding navigation. Has `origin_label: Option<String>` field for showing where the preview was opened from (e.g. "Security Review Finding"). | `src/tui/components/dialogs/source_preview.rs` |
| Provider auth resolver | `AuthResolver::resolve` is sync; `register_builtin_with_config` threads a shared `Arc<CredentialStore>` through `register_credential_provider` / `register_api_key_provider` / `register_config_provider`. All three helpers go through the centralized `resolve_provider_credential` helper. | `crates/codegg-providers/src/provider_core.rs`, `crates/codegg-providers/src/auth_types.rs` |
| OpenAI-compatible factories | `create_xai`, `create_mistral`, `create_groq`, `create_deepinfra`, `create_cerebras`, `create_cohere`, `create_together`, `create_perplexity`, `create_venice`, `create_generalcompute`, `create_opencode_go` all take `Credential`; `create_minimax` takes `String` (Anthropic-compatible). Backed by `OpenAiCompatibleProvider::simple_with_credential`; the legacy `simple(id, name, api_key, base_url)` is a backwards-compatible shim. | `crates/codegg-providers/src/provider/additional.rs`, `crates/codegg-providers/src/provider/openai_compatible.rs` |
| ExternalCommand in resolver | Disabled in the synchronous resolver. Returns `AuthError::Unsupported("ExternalCommand")` because the existing `ExternalCommandProvider::fetch` does not enforce its timeout. Async timeout plumbing is a follow-up. | `crates/codegg-providers/src/auth_types.rs`, `src/auth/external.rs` |
| ExternalCommandProvider::fetch | Returns `AuthError::Unsupported("ExternalCommand requires async timeout plumbing")` for any non-empty command; an empty command yields `AuthError::Invalid`. The previous `std::process::Command` shell-out path has been removed; no safe code path can accidentally execute a configured command. | `src/auth/external.rs:48` |
| Provider credential resolution path | Single resolution path: every helper in `crates/codegg-providers/src/provider_core.rs` (`register_credential_provider`, `register_api_key_provider`, `register_config_provider`) goes through `resolve_provider_credential()` → `AuthResolver::resolve()`. Legacy `cfg.api_key` is honored by the resolver via `ctx.legacy_api_key`; no helper reads `cfg.api_key` directly. | `crates/codegg-providers/src/provider_core.rs`, `crates/codegg-providers/src/auth_types.rs` |
| `codegg providers` and `codegg models` | Both commands now use `register_builtin_with_config`, so they see the same set of providers (including those backed by the user credential store). | `src/main.rs` |
| Stored bearer-token policy | `AuthConfig::Stored` and the no-auth fallback's store lookup both filter to `CredentialKind::ApiKey`. A future OAuth/bearer-token refresh flow will need a separate `kind` selector or policy module. `codegg auth set-key` only writes `ApiKey` records. | `crates/codegg-providers/src/auth_types.rs` |
| `codegg auth` CLI validation | `AuthCli::set_key` and `AuthCli::logout` validate provider and account ids up-front to contain only `[A-Za-z0-9_-]`. The wildcard `*` is accepted **only** by `logout`. `set_key` never echoes the key, key length, or any prefix/suffix. `status` never prints ciphertext, plaintext, or secret-derived fingerprints. | `src/auth/cli.rs` |
| `codegg auth` CLI | `codegg auth status`, `codegg auth set-key <provider>` (stdin), `codegg auth logout <provider>` are wired in `src/main.rs` and backed by `auth::cli::AuthCli`. | `src/main.rs`, `src/auth/cli.rs` |
| Goal module | Goal, GoalStatus, GoalBudget, GoalUsage, GoalStore, runtime | `crates/codegg-core/src/goal/` |
| Goal budget axes | 4 axes: max_turns, max_model_tokens, max_tool_calls, max_wallclock_secs | `crates/codegg-core/src/goal/model.rs` |
| Goal wall-clock durability | wallclock_secs persisted in SQLite, survives session restarts | `crates/codegg-core/src/goal/model.rs`, `crates/codegg-core/src/goal/store.rs` |
| AgentLoop goal fields | goal_store, goal_wall_clock | `src/agent/loop.rs` |
| Per-turn token tracking | last_turn_input_tokens, last_turn_output_tokens | `src/agent/loop.rs` |
| TUI goal status bar | format_goal_status_line, set_goal on StatusBarWidget | `src/tui/app/mod.rs`, `src/tui/components/status_bar.rs` |
| Goal budget slash cmd | /goal budget [show\|raise <axis> <n>] | `src/tui/app/mod.rs` |
| Goal+todo prompt contract | goal_and_todos_contract() in assemble_system_prompt_with_profile | `src/agent/prompt.rs` |
| Theme module | SemanticTheme, ThemeRegistry, native/Halloy parsers, ratatui projection | `src/theme/` |
| Built-in theme count | **50** Halloy-format themes from `themes.halloy.chat`, bundled as TOML via `include_str!`. Default = `cyber-red` (`DEFAULT_THEME_ID`) | `src/theme/registry.rs:BUILTIN_THEME_FILES` |
| Halloy field coverage | `general.{background,border,horizontal_rule,highlight_indicator,unread_indicator}`; `text.{primary,secondary,tertiary,success,warning,error,info,debug,trace}`; `buffer.{action,topic,nickname,highlight,code,url,timestamp,selection,border,border_selected,background*,server_messages.default}`; 6/8-digit hex accepted (alpha stripped) | `src/theme/halloy.rs` |
| `Theme.code_theme` type | `String` (was `&'static str`) | `src/tui/theme.rs` |
| Theme persistence | **SQLite** `user_preferences.theme.active` (authoritative) + config mirror. Writes via `tokio::spawn` so the UI doesn't block. Read on startup via `App::apply_persisted_preferences`. | `src/storage/preferences.rs`, `src/tui/app/mod.rs` |
| Theme live preview | `ThemePickerDialog::PreviewState` machine: Up/Down → `TuiMsg::ThemePreviewChanged` (live apply, no persist); Enter → `TuiMsg::ThemeCommit` (persist + close); Esc / close → `TuiMsg::ThemeRevert` (restore original + close) | `src/tui/components/dialogs/theme.rs`, `src/tui/app/mod.rs` |
| Last-used model persistence | `KEY_MODEL_LAST_USED` in `user_preferences`. Updated on every `SelectModel` / `cycle_model_forward` / `cycle_model_backward`. Read on startup and applied to `agent_state.current_model` if present in the model list. | `src/tui/app/mod.rs` |
| `/theme` slash command | list, use, reload, diagnostics subcommands | `src/tui/app/mod.rs:handle_theme_command` |
| Boundary script | `scripts/check-core-boundary.sh` | Verifies no forbidden imports/dependencies in codegg-core |
| ckcore alias | `.cargo/config.toml` | `cargo ckcore` = `check -p codegg-core` |
| Context module | artifact storage + projection + context_read tool + cache-aware packer (hardened observe-only layer) | `src/context/` |
| Context new modules | `NormalizedProviderUsage` (usage_normalize.rs), `EffectiveCostAnalysis` (effective_cost.rs) — diagnostic-only, no mutation; plus policy.rs (decide_policy + reduce_tool_palette + `detect_palette_starvation`) | `src/context/usage_normalize.rs`, `src/context/effective_cost.rs`, `src/context/policy.rs` |
| Volatile-tail compaction | `volatile_tail.rs` — gated late-context-only compaction of old tool-result messages with recovery handles, tombstone format, idempotent, observe/warn/compact rollout | `src/context/volatile_tail.rs` |
| ProjectionConfig defaults | max_success=800, max_failure=2000, enabled=true, artifact_store=true | `src/context/projection.rs` |
| ContextLedgerState limits | 20 files, 10 commands, 10 test results, 10 errors; empty handles rejected | `src/agent/context_frame.rs` |
| context_read registration | Registered when `artifact_store = true`, regardless of `project_tool_outputs` | `src/tool/factory.rs` |
| Handle building | Only `ContextHandle::build_tool()` (checked) exists; raw `build_handle()` has been removed entirely | `src/context/handle.rs` |
| `sourceActionPreview` full-document range | Uses `document_end_position_utf16()` to compute real UTF-16 end position from synced file contents (no `u32::MAX`) | `crates/egglsp/src/operations.rs` |
| `select_source_action_edit` command-only | `CodeAction` with `command: Some(_)` but `edit: None` is classified as `CommandOnlySourceAction` (command execution disabled) | `crates/egglsp/src/operations.rs` |
| `document_end_position_utf16` | Pure helper computing LSP Position at end of document using UTF-16 code units; handles empty, single-line, multiline, unicode, and trailing-newline cases | `crates/egglsp/src/operations.rs` |
| Overlay module | `crates/egglsp/src/overlay.rs` | OverlaySession (apply_overlay/restore), OverlayRestoreToken, SemanticCheckPreview (with diagnostics_error, symbols_error, restore_error), SemanticSymbolSummary |
| `semanticCheckPreview` overlay input | Accepts either full proposed content or a single-file unified diff patch; patch input is applied in memory against `file_path`, never written to disk, and the overlay is restored after the check | `crates/egglsp/src/overlay.rs`, `src/tool/lsp.rs` |
| `semanticCheckPreview` error fields | `diagnostics_error`, `symbols_error`, `restore_error` are `Option<String>` — non-None when the corresponding LSP request or restore fails; `restored_disk_view` reflects restore success; previously returned empty vectors or a bare bool | `crates/egglsp/src/overlay.rs` |
| `semanticCheckPreview` root enforcement | Operation-level `allowed_root: Option<&Path>` param; rejects files outside root with `LspError::PathOutsideRoot` | `crates/egglsp/src/operations.rs` |
| LspTool `execute_structured` success | `success=false` when `restore_error` is present in the JSON response; checks `/results/restore_error` pointer | `src/tool/lsp.rs` |
| `semanticContext` source-action hints | `include_source_actions` bool (default false) triggers `collect_source_action_hints` which iterates a hardcoded allowlist (`OrganizeImports`); returns `Vec<SemanticSourceActionHint>` with `action`, `available`, `preview`, `error`; pure helper `source_action_hint_from_result` converts `Result<WorkspaceEditPreview, LspError>` to hint; available hints add to `result_count`; source-action failures are per-hint and non-fatal | `src/tool/lsp.rs` |
| HierarchyDirection enum | Incoming, Outgoing, Both | `crates/egglsp/src/operations.rs` — invalid values return an error |
| `securityContext` operation | Security-review context packet with deterministic risk markers (11 categories), security-relevant diagnostics/symbols, optional call hierarchy, optional overlay; read-only, never writes files; risk marker scanning and filtering helpers in `src/tool/lsp_security.rs`; precise truncation flags; nonfatal LSP failures surfaced in notes; supports 5 presets via `security_preset` input | `src/tool/lsp.rs`, `src/tool/lsp_security.rs` |
| Security presets | 5 (rust_server, rust_cli, web_backend, dependency_review, unsafe_review) | `src/tool/lsp_security.rs`, `src/tool/lsp.rs` |
| `callHierarchy` / `typeHierarchy` operation | Read-only, shallow, bounded hierarchy summaries. Require `file_path`, `line`, and `column`. `callHierarchy` maps incoming (callers) and outgoing (calls made). `typeHierarchy` maps incoming (supertypes) and outgoing (subtypes). Non-recursive; unsupported servers may return empty sections. | `crates/egglsp/src/operations.rs`, `src/tool/lsp.rs` |
| Hierarchy `from_ranges` truncation | Capped at `MAX_HIERARCHY_RANGES = 32` per call; included in summary `truncated` flag alongside item and edge truncation | `src/tool/lsp.rs` |
| SecurityContext call expansion | Optional bounded recursive call expansion for securityContext with precise truncation via cap helpers (`capped_call_ranges`, `push_call_expansion_edge`, `push_call_expansion_node`). When caps are reached, returns partial graph with `truncated=true` rather than failing. | `src/tool/lsp.rs` — `build_call_expansion_summary()`, cap helpers, constants `DEFAULT_CALL_EXPANSION_DEPTH` etc. |
| Security workflow module | `src/security/workflow/` (split into 9 submodules: `mod.rs`, `types.rs`, `diff.rs`, `preflight.rs`, `evidence.rs`, `context.rs`, `report.rs`, `enrichment.rs`, `receipt.rs`) — vertical slice: diff parsing, preset selection, target building, securityContext request construction, review-prompt generation, and hardened evidence-based finding synthesis. Top-level API: `plan_security_review_from_diff(diff, repo_root)`. Key types: `ChangedHunk` (now includes `lines: Vec<DiffLine>` with `DiffLine { kind: DiffLineKind, text }`), `SecurityReviewTarget`, `SecurityTargetReason`, `SecurityReviewPrompt`, `SecurityReviewFindingStub`, `SecurityReviewReport`, `SecurityReviewHunkRef`, `SecurityReviewHunkLine`, `SecurityReviewHunkLineKind`. `SecurityReviewOutput.hunks` carries parsed hunk refs for TUI display. Exclusions: `vendor/`, `third_party/`, `target/`, `dist/`, `build/`, `node_modules/`, `*.min.js`. NOT excluded: `Cargo.toml`, `Cargo.lock`, `build.rs`. Risk markers → prompts, never findings. `call_depth` always 0. Evidence-based synthesis: `synthesize_evidence_based_findings()` groups evidence by file/line bucket, applies `is_finding_eligible()` gate (2+ dimensions), produces `SecurityReviewFinding` with `SecuritySeverity`/`SecurityConfidence` enums. Marker-only evidence never creates findings. Preflight evidence is structured (`SecurityPreflightEvidence`) and file-scoped — different-file evidence never supports a finding. `evidence_matches_group()` enforces same-file + nearby-line (+/-5) grouping for positioned groups. `run_content_preflight_checks_for_targets()` scans only a line window (radius=10) around positioned targets. Legacy string-only preflight evidence is not finding-eligible. `synthesize_review_prompts_only()` is the renamed prompt-only synthesis; `synthesize_findings()` is a deprecated wrapper. The `receipt.rs` submodule holds the TUI-facing `SecurityReviewReceipt` DTO + `project_receipt_to_panel_items`/`filter_panel_items` helpers + `SecurityReviewTaskState` reentrancy guard; consumed by the `Dialog::SecurityReview` result panel. `SecurityReviewPanelItem` has `hunk: Option<SecurityReviewHunkRef>` — findings/prompts are matched to hunks by file_path + line in range. `SecurityReviewFilter` includes `HunkBacked` variant to filter items with hunk context. | `src/security/workflow/` |
| `SecurityReviewWorkflowOptions` | Options struct for `run_security_review_workflow` orchestrator: `include_prompts`, `include_findings`, `run_filename_preflight`, `run_content_preflight`, `hunk_local_content_preflight`, `max_findings`, `max_prompts`, `enable_hunk_source_context` (default false), `enable_lsp_enrichment` (default false), `max_lsp_enriched_targets` (8), `max_lsp_requests` (8), `lsp_request_timeout_ms` (2500), `max_hunk_context_files` (8), `max_hunk_context_requests` (8), `hunk_context_timeout_ms` (2500). Defaults are conservative and bounded. | `src/security/workflow/report.rs` |
| `run_security_review_workflow` | Async orchestrator entry point running full pipeline (discover targets → build prompts → preflight checks → hunk source context → evidence-based synthesis → assemble output). Accepts an optional `hunk_executor: Option<&dyn HunkSourceContextExecutor>` for best-effort `hunkSourceContext` execution. Content preflight uses `root.join(p)` for repo-root-relative reads. | `src/security/workflow/report.rs` |
| Testable command functions | `SecurityReviewCommandArgs` (includes `open_panel_on_complete: bool` for `--panel`, `hunk_context: bool` for `--hunk-context`), `parse_security_review_args()`, `run_security_review_command()` — testable CLI argument parsing and command execution. | `src/security/workflow/report.rs` |
| `SecurityContextEscalationLevel` | Enum (None, Basic, CallDepth1, CallDepth2) for selective LSP call expansion in securityContext. Read-only and bounded (max depth 2). | `src/security/workflow/context.rs` |
| `choose_security_context_escalation` | Pure decision helper mapping risk signals (target reason, finding category, prompt markers) to `SecurityContextEscalationLevel`. Deterministic and testable. | `src/security/workflow/context.rs` |
| `build_escalated_security_context_request` | Builds JSON payload for securityContext with appropriate `call_depth` and node caps. Read-only, never writes files. | `src/security/workflow/context.rs` |
| `plan_security_context_escalations` | Returns `SecurityContextEscalationPlan` DTO — a policy recommendation per target, not an execution. | `src/security/workflow/context.rs` |
| `render_security_review_summary/findings/prompts` | Rendering helpers: `render_security_review_summary` (compact counts), `render_security_review_findings` (severity/confidence labels), `render_security_review_prompts` (no severity). | `src/security/workflow/report.rs` |
| `/security-review` command | TUI slash command with flags: `--changed` (shorthand for `--base HEAD`), `--base <ref>`, `--json`, `--prompts-only`, `--findings-only`, `--no-content`, `--no-filename`, `--max-findings N`, `--max-prompts N`, `--enrich` (opt-in LSP enrichment), `--max-enriched-targets N`, `--lsp-timeout-ms N`, `--hunk-context` (opt-in best-effort hunkSourceContext execution), `--panel` (auto-open result panel on completion). Command handler: `parse_security_review_args()` and `run_security_review_command()` in `src/security/workflow/report.rs`. Wired in `src/tui/app/mod.rs`. Dispatched asynchronously via `tokio::spawn` from the `/security-review` branch in `src/tui/app/mod.rs:4226`; the spawned task calls `run_security_review_background()` and posts a `TuiCommand::SecurityReviewFinished { id, receipt, error }` back through `tui_cmd_tx`. The TUI render thread is never blocked. Reentrancy guard: `App.security_review_running: Option<SecurityReviewTaskState>` holding `{ id, abort_handle }` (defined in `src/security/workflow/receipt.rs:301`); set on dispatch, cleared in both success and failure paths by `handle_security_review_finished` (`src/tui/mod.rs:2205`) which silently drops stale completions whose id no longer matches. On success the full report is pushed to the message timeline as a `UIMessage` with `MessageRole::Assistant` and a `[Security Review]` label, plus a brief success toast, and the structured `SecurityReviewReceipt` is stored on `App.latest_security_review`; on failure an error toast is shown. No `block_in_place` / `block_on` in the command path. | `src/tui/app/mod.rs`, `src/tui/mod.rs`, `src/security/workflow/report.rs` |
| `run_security_review_background` | `pub async fn run_security_review_background(root: PathBuf, args: SecurityReviewCommandArgs, lsp_tool: Option<Arc<LspTool>>) -> Result<SecurityReviewReceipt, String>` in `src/security/workflow/report.rs`. TUI-facing entry point that owns its inputs (no borrowed `&self` across await) and constructs the `LspSecurityContextExecutor` internally when `lsp_tool` is `Some`. Returns the structured `SecurityReviewReceipt` (id, root, args, output, rendered_report, completed_at_ms, enriched, lsp_available, hunk_context_requested, hunk_context_available, hunk_context_executed, hunk_context_succeeded, hunk_context_requests_attempted, hunk_context_requests_succeeded) so the caller can both push the rendered text into the message timeline AND store the receipt for the result panel. In remote/socket mode `lsp_tool = None` and the call falls back to deterministic stage-1 with `note_lsp_enrichment_unavailable`. Dispatched from the spawn task in `src/tui/app/mod.rs:4226` which then sends `TuiCommand::SecurityReviewFinished` back through `tui_cmd_tx`. | `src/security/workflow/report.rs`, `src/tui/app/mod.rs`, `src/tui/mod.rs` |
| `SecurityReviewRunId` | `pub struct SecurityReviewRunId(String)` newtype in `src/security/workflow/report.rs` used as the `id` field of `TuiCommand::SecurityReviewRun`. Owned, `Clone`, and cheaply formattable so the dispatcher can correlate dispatched runs with their eventual timeline results. | `src/security/workflow/report.rs`, `src/tui/app/mod.rs` |
| `SecurityContextExecutor` | Trait for executing bounded, read-only `securityContext` LSP requests. `NoopSecurityContextExecutor` (always errors), `FixtureSecurityContextExecutor` (returns pre-configured responses/failures, tracks requests), `LspSecurityContextExecutor` (real adapter wrapping `LspTool`, validates requests, injects operation field). | `src/security/workflow/context.rs`, `src/security/lsp_executor.rs` |
| `LspSecurityContextExecutor` | Real adapter implementing `SecurityContextExecutor` by wrapping `LspTool`. Validates requests, injects `"operation": "securityContext"`, delegates to `LspTool::execute()`, parses JSON string response. Lives in `src/security/lsp_executor.rs` near the security workflow boundary. | `src/security/lsp_executor.rs` |
| `validate_security_context_request` | Validates securityContext request payloads: checks file_path, security_preset, call_depth (0-2), max_call_nodes (<=64), rejects mutation fields. | `src/security/lsp_executor.rs` |
| `SecurityContextExecutorProvider` | Provider abstraction for obtaining a security context executor at command execution time. When no executor is available, the command runner skips enrichment entirely (deterministic stage-1 only) and appends an unavailable note. | `src/security/workflow/context.rs` |
| `HunkSourceContextExecutor` | Trait for executing bounded, read-only `hunkSourceContext` LSP requests in the security review workflow. `NoopHunkSourceContextExecutor` (always errors), `LspHunkSourceContextExecutor` (real adapter wrapping `LspTool`). Mirrors the `SecurityContextExecutor` pattern. | `src/security/workflow/context.rs`, `src/security/lsp_executor.rs` |
| `LspHunkSourceContextExecutor` | Real adapter implementing `HunkSourceContextExecutor` via internal `TypedHunkSourceContextTarget` trait (production: `LspTool`). Calls `LspTool::execute_hunk_source_context_typed()` directly with a typed `HunkSourceNavigationRequest` — no JSON round-trip. `#[cfg(test)]` `with_target()` constructor allows recording targets for forwarding verification without a live LSP server. Lives in `src/security/lsp_executor.rs` near the security workflow boundary. | `src/security/lsp_executor.rs` |
| `run_security_review_command_with_executor` | Command runner accepting an optional `&dyn SecurityContextExecutor`. When enrich=true + executor provided, uses enriched workflow; when enrich=true + executor None, uses noop with availability note. | `src/security/workflow/report.rs` |
| `SecurityContextEnrichmentResult` | DTO for a single LSP enrichment pass: target, level, request, response, prompts, evidence, notes. | `src/security/workflow/enrichment.rs` |
| `SecurityReviewHunkRef` | Compact hunk context for TUI display, carrying `file_path`, `old_start`, `old_lines`, `new_start`, `new_lines`, `header`, and `lines: Vec<SecurityReviewHunkLine>`. Built from parsed `ChangedHunk`s by `build_hunk_refs_from_changed_hunks()` in `report.rs`. Has `contains_new_line(line: u32) -> bool` method for new-side line matching (actual line match first, range fallback). | `src/security/workflow/types.rs:146` |
| `SecurityReviewHunkLine` | Single line within a hunk with `old_line: Option<u32>`, `new_line: Option<u32>`, `kind: SecurityReviewHunkLineKind`, `text: String`. Focus (highlight) is computed at render time by comparing panel item's `line` against `new_line`, not stored on the struct. | `src/security/workflow/types.rs:158` |
| `SecurityReviewHunkLineKind` | Enum (`Added`, `Removed`, `Context`) for hunk line display styling. Mapped from `DiffLineKind` in `build_hunk_refs_from_changed_hunks()`. | `src/security/workflow/types.rs:168` |
| `ChangedHunk` | Now includes `lines: Vec<DiffLine>` with `DiffLine { kind: DiffLineKind, text }` — the diff parser preserves individual line content for hunk context display. | `src/security/workflow/types.rs:117`, `src/security/workflow/diff.rs` |
| `SecurityReviewOutput.hunks` | `Vec<SecurityReviewHunkRef>` — parsed hunk refs carried on the output for TUI display. Populated by `build_hunk_refs_from_changed_hunks()` after target discovery. | `src/security/workflow/types.rs:276` |
| `SecurityReviewPanelItem.hunk` | `Option<SecurityReviewHunkRef>` — findings/prompts are matched to hunks by `file_path` + new-side line via `hunk.contains_new_line()` in `project_receipt_to_panel_items()`. Findings have evidence-line fallback for hunk attachment. | `src/security/workflow/receipt.rs:85` |
| `SecurityReviewFilter::HunkBacked` | Filter variant that shows only items with a matching hunk context (`item.hunk.is_some()`). Included in `SecurityReviewFilter::ALL` array and cycled via `f`. | `src/security/workflow/receipt.rs:96` |
| `run_security_context_enrichment` | Enrichment runner: filters escalation plans, caps requests, executes via `SecurityContextExecutor` with timeout, converts responses to prompts/evidence, records failures as notes. | `src/security/workflow/enrichment.rs` |
| `merge_enrichment_results` | Merges enrichment prompts with stage-1 prompts (deduplicating on file+line+title), collects extra evidence and enrichment notes. | `src/security/workflow/enrichment.rs` |
| `evidence_from_security_context` | Converts `securityContext` JSON into `StructuredSecurityEvidence`: risk markers (accepts `file`/`file_path`), diagnostics, call graph summaries, truncation notices. Always file-scoped, compact. | `src/security/workflow/evidence.rs` |
| `SecurityEvidenceKind::HunkNavigation` | Evidence from `hunkSourceContext` LSP operation: enclosing symbols, diagnostics in changed ranges, definitions, and references. Recognized by `is_finding_eligible()` as a supporting dimension. | `src/security/workflow/types.rs:60` |
| `ChangedHunk::to_hunk_descriptor()` | Converts a `ChangedHunk` into an egglsp `HunkDescriptor` for use with `hunkSourceContext`. Computes `old_range`/`new_range` from start/count fields; `hunk_index` provides the deterministic hunk id prefix. | `src/security/workflow/types.rs:330` |
| `HunkSourceContextPolicy` | Configuration for when `hunkSourceContext` should be invoked. Fields: `enabled` (default true), `max_patch_bytes` (default 64KB), `max_hunks` (default 20), `include_definitions` (true), `include_references` (true), `include_call_hierarchy` (false), `include_type_hierarchy` (false). | `src/lsp/hunk_nav_policy.rs:8` |
| `HunkSourceContextDecision` | Enum: `Use { file_path, patch }` (call hunkSourceContext) or `Skip { reason }` (skip with documented reason). | `src/lsp/hunk_nav_policy.rs:41` |
| `decide_hunk_source_context()` | Pure policy function evaluating patch, file extension, size, and hunk count against `HunkSourceContextPolicy`. Returns `HunkSourceContextDecision`. | `src/lsp/hunk_nav_policy.rs:61` |
| `format_hunk_source_context_summary()` | Formats `HunkSourceNavigationResponse` into a compact, deterministic, bounded agent-facing summary. Shows file path, diagnostic freshness, hunk evidence (enclosing symbol, diagnostics, definitions, references, call/type hierarchy), truncation flags. | `src/lsp/hunk_nav_prompt.rs:13` |
| `SecurityReviewWorkflowOptions.enable_hunk_source_context` | `bool` field (default false). When true, collects hunk navigation evidence for each changed file and injects it into evidence-based synthesis. Fail-open: errors noted, not fatal. | `src/security/workflow/report.rs:287` |
| `evidence_from_hunk_source_context()` | Converts `HunkSourceNavigationResponse` into `Vec<StructuredSecurityEvidence>`. Extracts enclosing symbols (HunkNavigation), diagnostics (Diagnostic), definitions (HunkNavigation), and reference counts (HunkNavigation). | `src/security/workflow/report.rs:932` |
| `collect_hunk_source_context_for_file()` | Async. Uses `HunkSourceContextPolicy` to decide whether to invoke `hunkSourceContext` for a file's hunks. When policy returns `Use` and an executor is provided, actually executes `hunkSourceContext` via the `HunkSourceContextExecutor` trait. Returns evidence, summary (from `format_hunk_source_context_summary`), and notes. Fail-open. | `src/security/workflow/report.rs:1007` |
| `collect_hunk_source_context_all_files()` | Async. Groups hunks by file, processes files in deterministic sorted order (capped at 8 files), calls `collect_hunk_source_context_for_file` per file with actual per-file patch data, merges evidence/summaries/notes. Returns `HunkSourceContextCollectionResult` (evidence, summaries, notes, `HunkSourceContextExecutionStats`). Policy evaluation (Option B) happens before request-cap check, keeping skip statistics complete. `files_considered` counts files whose policy was evaluated (within file cap, before any request-cap break). `evidence_items_emitted` is assigned post-loop from `all_evidence.len()` (not incrementally accumulated). Request caps count actual executor calls, not loop position. Accepts an optional `HunkSourceContextExecutor`. Patch byte-size policy uses actual per-file patch data. Fail-open per file. | `src/security/workflow/report.rs:1308` |
| `synthesize_evidence_based_findings_with_extra_evidence` | Enriched synthesis: combines base prompts with enriched evidence for a second pass. Injects matching CallPath/Diagnostic/TruncationNotice evidence into findings and re-classifies. | `src/security/workflow/evidence.rs` |
| `run_security_review_workflow_with_lsp_enrichment` | Enriched orchestrator: runs deterministic stage-1, then optional LSP enrichment via executor, reruns synthesis with enriched evidence. Fail-soft: returns stage-1 output on any failure. | `src/security/workflow/report.rs` |
| `DiagnosticCacheEntry` | Stores per-file diagnostics with `received_at`, `source`, `content_version` metadata for freshness classification | `crates/egglsp/src/client.rs` |
| `LspClient::diagnostic_snapshot()` | Classifies diagnostics freshness based on cache entry metadata; stale cached diagnostics preserve their actual `age_ms` (not zero) | `crates/egglsp/src/client.rs` |
| `DiagnosticsCollector::get_diagnostic_snapshot_for_file()` | Primary API for obtaining `LspDiagnosticSnapshot` with freshness metadata | `crates/egglsp/src/diagnostics.rs` |
| `DiagnosticsCollector::get_all_diagnostic_snapshots()` | Freshness-aware bulk diagnostic snapshots API, returns `HashMap<String, LspDiagnosticSnapshot>` | `crates/egglsp/src/diagnostics.rs` |
| `LspDiagnosticSnapshot::diagnostics_may_still_be_warming()` | Derived from snapshot freshness: `PossiblyStale` + empty diagnostics = warming | `crates/egglsp/src/diagnostics.rs` |
| `capabilities` operation | Uses `capability_snapshot_for_file()` shared with `semanticContext` and `securityContext` | `src/tool/lsp.rs` |
| `LspDiagnosticFreshness` enum variants | `Fresh`, `PossiblyStale`, `Stale`, `Unavailable` — freshness classification for diagnostics | `crates/egglsp/src/diagnostics.rs` |
| `DiagnosticEvidenceMeta` struct | Carries `freshness`, `source`, `age_ms`, `usable_evidence` for semantic/security context packets; `age_ms` is age in milliseconds since diagnostics were received | `src/tool/lsp.rs` |
| Capability-gated operations | `semanticContext` and `securityContext` check `LspCapabilitySnapshot` before optional expensive LSP calls (definitions, references, call hierarchy, type hierarchy); unsupported ops append notes instead of failing | `src/tool/lsp.rs` |
| New compatibility module | `compatibility.rs` | `crates/egglsp/src/compatibility.rs` — `LspCompatibilityProfile`, `LspReadinessPolicy`, `LspRestartPolicy`, `LspRestartMode`, `LspServerVersion`, `LspCompatibilityReport`, `LspCompatibilityCheck`, `CompatibilityCheckStatus`, `rust_analyzer_profile()`, `pyright_profile()`, `profile_for_server()`, `tier1_profiles()`, `require_server_binary()` |
| Health module | `health.rs` | `crates/egglsp/src/health.rs` — `LspOperationalState` (Starting/Initializing/Indexing/Ready/Deaded/RestartScheduled/Restarting/Failed/Stopping/Stopped), `transition()` state machine, `InvalidTransition`, `LspOperationalHealthSnapshot` |
| Supervisor module | `supervisor.rs` | `crates/egglsp/src/supervisor.rs` — `LspProcessExitEvent`, `StderrRingBuffer` (100 lines / 64KB cap) |
| Document sync module | `document_sync.rs` | `crates/egglsp/src/document_sync.rs` — `OpenDocumentRegistry`, `OpenDocumentSnapshot` |

### Security Notes

- **Auth middleware allows requests without token when none configured**: At `src/server/middleware/auth.rs:37-39`, when `expected_token` is `None`, requests are allowed through. This may be intentional for development but should be reviewed for production.
- **WebSocket auth is consistent with HTTP**: Both `src/server/ws.rs:103-106` and `middleware/auth.rs:37-39` return Ok when no token is configured.
- **ExternalCommand is disabled in the synchronous resolver**: `AuthConfig::ExternalCommand` is recognized but `AuthResolver::resolve` returns `AuthError::Unsupported("ExternalCommand")` because the underlying `ExternalCommandProvider::fetch` uses `std::process::Command` and does not enforce its timeout. Re-enable only when async timeout plumbing is in place. See `crates/codegg-providers/src/auth_types.rs:227-237`.
- **Provider registration logging policy**: `register_credential_provider` / `register_api_key_provider` / `register_config_provider` log only `ResolvedAuthSource::as_str()` and the env var name. They never log secret prefix / suffix / length. New log lines that touch auth must follow the same rule.
- **Security review workflow**: `src/security/workflow/` (split into 9 submodules: `mod.rs`, `types.rs`, `diff.rs`, `preflight.rs`, `evidence.rs`, `context.rs`, `report.rs`, `enrichment.rs`, `receipt.rs`) implements a security review vertical slice: parses unified diff hunks (`parse_changed_hunks`), applies path exclusions (`is_security_review_excluded_path`), selects deterministic `securityContext` presets (`select_security_preset`), builds review targets and `securityContext` request payloads (`build_security_review_targets`, `build_security_context_request`), converts risk markers to review prompts (`prompts_from_security_context`), and assembles reports with an explicit "not confirmed findings" note (`assemble_security_review_report`). Top-level API: `plan_security_review_from_diff(diff, repo_root)`. Async orchestrator: `run_security_review_workflow(root, base, options, hunk_executor)` runs the full pipeline (discover → preflight → hunk source context → evidence-based synthesis → assemble). Content preflight uses `root.join(p)` for repo-root-relative reads (works from any cwd). `SecurityReviewWorkflowOptions` controls stages and output caps (including `max_hunk_context_files`, `max_hunk_context_requests`, `hunk_context_timeout_ms`). Key types: `ChangedHunk`, `SecurityReviewTarget`, `SecurityTargetReason`, `SecurityReviewPrompt`, `SecurityReviewFindingStub`, `SecurityReviewReport`. Exclusions: `vendor/`, `third_party/`, `target/`, `dist/`, `build/`, `node_modules/`, `*.min.js`. NOT excluded: `Cargo.toml`, `Cargo.lock`, `build.rs`. Risk markers become review prompts, NEVER findings. `call_depth` is always 0. Escalation policy: `choose_security_context_escalation(target, finding, prompt)` maps risk signals to `SecurityContextEscalationLevel` (None, Basic, CallDepth1, CallDepth2); `build_escalated_security_context_request(target, level)` builds the payload. `plan_security_context_escalations(targets, ...)` returns a `SecurityContextEscalationPlan` DTO — a policy recommendation, not execution. Escalation is read-only and bounded. The hunk source context integration executes `hunkSourceContext` via the `HunkSourceContextExecutor` trait (`src/security/workflow/context.rs`) with `LspHunkSourceContextExecutor` (`src/security/lsp_executor.rs`) as the real adapter calling `LspTool::execute_hunk_source_context_typed()` directly with a typed `HunkSourceNavigationRequest` — no JSON round-trip. Internal pre-parsed hunk descriptors are used via the typed API; the model-facing tool schema remains patch-only. Policy skip decisions are routing metadata, never security evidence. Findings are heuristic defensive review outputs, not proof of exploitability. Rendering helpers: `render_security_review_summary`, `render_security_review_findings`, `render_security_review_prompts`. The `receipt.rs` submodule holds the TUI-facing `SecurityReviewReceipt` DTO + `project_receipt_to_panel_items`/`filter_panel_items` helpers + `SecurityReviewTaskState` reentrancy guard; consumed by the `Dialog::SecurityReview` result panel. | `src/security/workflow/` |
- **Security review result panel + show/cancel commands**: The TUI stores a structured `SecurityReviewReceipt` on `App.latest_security_review` after each successful run (`App::set_latest_security_review` at `src/tui/app/mod.rs:914`). The receipt carries the structured `SecurityReviewOutput` plus the rendered report and is the input to `Dialog::SecurityReview` (a master/detail panel at `src/tui/components/dialogs/security_review.rs` with keybindings `j/k`, `PgUp/PgDn`, `f` cycle filter, `n` notes, `p` prompts, `h` jump to hunk section, `H` copy hunk text to clipboard, `]`/`[` next/previous hunk-backed item, `Enter` opens a read-only source preview dialog for the finding's file via `resolve_security_review_item_path` in `receipt.rs` — root-scoped, shows "Security Review Finding/Prompt" origin label, falls back to clipboard if the file cannot be opened), `Esc/q` close). The reentrancy guard `App.security_review_running: Option<SecurityReviewTaskState>` (`src/security/workflow/receipt.rs:301`) holds `{ id, abort_handle }` for the lifetime of a background review. `/security-review-show` reopens the latest receipt without rerunning the review. `/security-review-cancel` calls `AbortHandle::abort()` via `App::cancel_security_review` (`src/tui/app/mod.rs:936`) and clears the guard; the completion handler in `src/tui/mod.rs:2205` ignores stale completions whose id no longer matches the guard. Cancellation is best-effort — if the spawned task is in a non-cancellable section (e.g. inside a blocking syscall), its completion may still arrive and is dropped by the id-mismatch guard. The review is read-only by design; no file mutations. Hunk context is derived from the reviewed diff, not from live files. Focus (highlight) is computed at render time by comparing `item.line` against `hunk_line.new_line`, so two items sharing one hunk can highlight different lines.
- **Evidence-based security finding synthesis**: `synthesize_evidence_based_findings()` groups evidence by file/line bucket, applies `is_finding_eligible()` gate (requires 2+ evidence dimensions), and produces `SecurityReviewFinding` with `SecuritySeverity`/`SecurityConfidence` enums. Marker-only evidence never creates findings. Preflight evidence is structured (`SecurityPreflightEvidence`) with file paths and optional line numbers — different-file evidence never supports a finding. `evidence_matches_group()` enforces same-file + nearby-line grouping. `run_content_preflight_checks_for_targets()` provides locality-aware scanning (radius=10 lines). Legacy string-only preflight evidence cannot globally support findings. `synthesize_review_prompts_only()` is the renamed prompt-only synthesis (marker-only → prompts, findings always empty). Content preflight checks are local/deterministic (secrets, unsafe, process exec, SQL interpolation, weak crypto). `SecurityEvidenceKind::HunkNavigation` evidence from `hunkSourceContext` is recognized by `is_finding_eligible()` as a supporting dimension — but `ChangedHunk + HunkNavigation` alone is not finding-eligible; `HunkNavigation` requires `RiskMarker` or `Preflight` as a supporting dimension (e.g. `marker + hunk_nav` or `hunk_nav + preflight_fail`). Findings are defensive review outputs, not proof of exploitability.
- **Security review LSP enrichment**: Optional second-stage pass (`--enrich` flag) executes bounded, read-only `securityContext` requests for escalated targets via `SecurityContextExecutor` trait. `NoopSecurityContextExecutor` (always errors, used in tests), `FixtureSecurityContextExecutor` (test doubles), `LspSecurityContextExecutor` (real adapter in `src/security/lsp_executor.rs` wrapping `LspTool`). `validate_security_context_request()` validates request payloads before execution. `SecurityContextExecutorProvider` trait provides executor injection. `run_security_review_command_with_executor()` accepts an optional executor for command-level injection. In local mode the TUI creates a shared `LspTool` at startup (`App.lsp_tool`) and passes a `LspSecurityContextExecutor` to the command handler for `--enrich`. In socket/remote mode `lsp_tool` is `None` and `--enrich` falls back to deterministic stage-1 with an unavailable note. `run_security_context_enrichment()` filters escalation plans, caps requests, executes with timeout, converts responses to prompts/evidence via `evidence_from_security_context()`. `run_security_review_workflow_with_lsp_enrichment()` runs deterministic stage-1 then optional enrichment. Fail-soft: returns stage-1 output on any failure. Three `note_lsp_enrichment_*` helpers (`unavailable`, `no_eligible_targets`, `executed`) in `report.rs` append idempotent notes to the output. Enrichment is opt-in, read-only, bounded, and never mutates files. No-executor runtimes fail soft with clear notes.

### CoreRequest Handler Attention Points

- `CoreRequest` enum in `crates/codegg-protocol/src/core.rs:50-175`
- InprocCoreClient handlers at `src/core/mod.rs:52-355` handle: TurnSubmit, SessionMessagesLoad, SessionMessageCounts, SessionCreate, SessionLoad, SessionAttach, etc.
- TurnSubmit now delegates to `self.deps.turn_runtime` (injected `Arc<dyn TurnRuntime>`) instead of constructing `DefaultTurnRuntime` directly.
- Variants falling through to `Ack`: Initialize, TurnCancel, TurnSteer, AgentSelect, ModelSelect - verify if TUI actually sends these before implementing meaningful responses.

## Helpful Patterns for Future Agents

### Provider Auto-Registration
- `register_builtin_with_config()` at `crates/codegg-providers/src/provider_core.rs:610` registers providers via env vars, the typed `auth` block, and the user credential store. It builds a single `Arc<CredentialStore>` and threads it into the per-provider helpers.
- Per-provider helpers:
  - `register_credential_provider` — OpenAI-compatible providers that accept a full `Credential` envelope (mistral, groq, deepinfra, cerebras, cohere, together, perplexity, xai, venice, opencode_go, generalcompute).
  - `register_api_key_provider` — providers that genuinely need a static API-key string (opencode_zen, minimax). Rejects `CredentialKind::BearerToken` with a `tracing::warn!`.
  - `register_config_provider` — base-URL-aware variant (anthropic, openai native, google, openrouter).
- All three go through the centralized `resolve_provider_credential(provider_id, cfg, env_var, store)` helper.
- Adding ANY provider via config disables all env-var auto-registration (intentional design)
- SAP AI Core, Zenmux, Kilo, Vercel AI Gateway are config-only, NOT auto-registered
- Check `crates/codegg-providers/src/provider_core.rs:register_builtin_with_config()` for details

## Documentation Structure

### Directory Structure

```
.opencode/skills/
├── agent-loop/          # AgentLoop, TuiCommand, TuiMsg, compaction, router, team
├── auth/               # AuthConfig, Credential, AuthResolver, CredentialStore, mask_secret, codegg auth CLI
├── caching/            # Provider response caching (not yet wired in)
├── client/             # Remote TUI client, WebSocket
├── command/            # Slash commands, templates, execution
├── compaction/         # Context compaction strategies
├── config/             # Config loading, validation, encryption, watching
├── context/            # Artifact storage, tool-output projection, context_read tool, cache-aware packer, volatile-tail compaction
├── crypto/             # API key encryption
├── diff/               # Inline diff visualization
├── e2e/                # End-to-end testing guide
├── error/              # AppError, ProviderError, ToolError, is_retryable
├── event-bus/          # GlobalEventBus, PermissionRegistry, QuestionRegistry
├── exec/               # Exec mode for CI/CD
├── git/                # Git session, git info in prompts, worktree management
├── goal/               # Long-horizon goal runtime, budget enforcement, auto-continuation
├── hooks/              # Hooks system for agent lifecycle
├── ide/                # IDE integration (VS Code, JetBrains)
├── lsp/                # LSP client, diagnostics, code operations
├── mcp/                # MCP connection manager
├── memory/             # Memory system, consolidation, patterns
├── mode/               # Mode system (Review/Debug/Docs)
├── model-dialog/       # Model selection/config dialog
├── notifications/       # Desktop notifications
├── permission/         # PermissionChecker, DoomLoop, PermissionRegistry
├── plugin/             # WASM plugin system
├── provider/           # LLM provider implementations
├── shell_session/      # Shell session metadata
├── question-response/  # Question/permission response shapes
├── resilience/          # Circuit breaker, FallbackProvider
├── router/             # Model auto-routing
├── sandbox/            # Landlock filesystem sandboxing
├── security/           # SSRF, symlink protection, Landlock
├── server/             # HTTP/WebSocket server for remote TUI
├── session/            # Session storage, database schema
├── skills/             # Skill loading, activation, SkillIndex
├── snapshot/           # File state capture and restore
├── storage/            # SQLite initialization, pragmas
├── subagent/           # SubAgentPool, SubAgentSpawner
├── team/               # Multi-agent team coordination
├── testing/            # Testing guide (unit, integration, E2E)
├── tool/               # Tool path validation, async command
├── tool-search/        # Tool discovery and catalog
├── tts/                # Text-to-speech module
├── tui/                # Terminal UI, keyboard shortcuts
├── tui_input/          # TUI input handling, paste, bindings
├── tui-dialog-maintenance/  # TUI dialog maintenance guide
├── tui-dialog-testing/      # TUI dialog testing guide
├── upgrade/            # Self-upgrade via GitHub releases
├── util/               # Clipboard, fuzzy matching, truncation
└── worktree/           # Git worktree management
```

### Adding New Module Guidance

When adding guidance for a new module:

1. Create `.opencode/skills/<module>/SKILL.md` with YAML frontmatter
2. Add the module to the skills directory structure above
3. Add the module to the Quick Reference table
4. Use frontmatter: `name`, `description`, `version`, `process`

### File Naming Convention

- `AGENTS.md` - Root index file (this file)
- `.opencode/skills/<name>/SKILL.md` - Module-specific skill guides
- `architecture/<module>.md` - Architecture documentation per module

## Quick Reference

| Topic | Location |
|-------|----------|
| Shell Session (shell session metadata) | `.opencode/skills/shell_session/SKILL.md` |
| Agent (AgentLoop, compaction, router, team, turn runtime) | `.opencode/skills/agent-loop/SKILL.md` |
| Event Bus (GlobalEventBus, PermissionRegistry, QuestionRegistry) | `.opencode/skills/event-bus/SKILL.md` |
| TUI (keyboard shortcuts, FocusManager, Component trait) | `.opencode/skills/tui/SKILL.md` |
| Core (CoreClient facade, transports, protocol envelopes) | `.opencode/skills/core/SKILL.md` |
| Security (SSRF, symlinks, Landlock) | `.opencode/skills/security/SKILL.md` |
| Security review workflow (async dispatch, result panel, show/cancel commands) | `.opencode/skills/security/SKILL.md`, [plans/security_review_result_panel.md](plans/security_review_result_panel.md), `src/security/workflow/receipt.rs` |
| WASM plugins | `.opencode/skills/plugin/SKILL.md` |
| MCP (Model Context Protocol) | `.opencode/skills/mcp/SKILL.md` |
| Provider (LLM providers, Arc<String> types, FallbackProvider) | `.opencode/skills/provider/SKILL.md` |
| Codegg Config (Config, paths, validation, watching) | `crates/codegg-config/` |
| Codegg Protocol (CoreRequest, CoreResponse, CoreEvent, TuiMessage) | `crates/codegg-protocol/` |
| Codegg Providers (Provider trait, ProviderRegistry, auth types, CircuitBreaker) | `crates/codegg-providers/` |
| Codegg Core (bus, error, goal, memory, session, storage, snapshot, worktree, task_state, model_profile, resilience, protocol_conversions) | `crates/codegg-core/` |
| Auth (AuthConfig, Credential, AuthResolver, CredentialStore, mask_secret, `codegg auth ...` CLI) | `.opencode/skills/auth/SKILL.md`, `architecture/auth.md` |
| Crypto (API key encryption, Argon2id key derivation) | [architecture/crypto.md](architecture/crypto.md) |
| Error (AppError, ProviderError, ToolError, is_retryable, CircuitOpen) | `.opencode/skills/error/SKILL.md` |
| Resilience (CircuitBreaker, FallbackProvider) | `.opencode/skills/resilience/SKILL.md` |
| Permission (mode system, PermissionChecker, DoomLoop, PermissionRegistry) | `.opencode/skills/permission/SKILL.md` |
| LSP (Language Server Protocol, diagnostics, code operations, semantic context packets, securityContext call expansion, capability discovery, diagnostics freshness, capability-gated operations, diagnostic evidence in context packets, shared semantic context API, initialization coordinator with leader/waiter election, SharedInitError for concurrent init, lifecycle generation tracking, authoritative JoinHandle task ownership, drop-guard cleanup, watch-based concurrent shutdown coordination) | `.opencode/skills/lsp/SKILL.md` |
| Tool (path validation, async command, ToolExecutor, ToolCatalog) | `.opencode/skills/tool/SKILL.md` |
| Exec mode | `.opencode/skills/exec/SKILL.md` |
| Hooks system | `.opencode/skills/hooks/SKILL.md` |
| Client (remote TUI, WebSocket) | `.opencode/skills/client/SKILL.md` |
| Server (HTTP, WebSocket, REST API, SSE) | `.opencode/skills/server/SKILL.md` |
| Snapshot (file state capture and restore) | `.opencode/skills/snapshot/SKILL.md` |
| Skills (skill system overview) | `.opencode/skills/skills/SKILL.md` |
| Command (slash commands, templates, execution) | `.opencode/skills/command/SKILL.md` |
| IDE (VS Code, JetBrains detection, diff viewing) | `.opencode/skills/ide/SKILL.md` |
| Config (loading, validation, encryption, watching) | `.opencode/skills/config/SKILL.md` |
| Memory (session-to-session learning, consolidation) | `.opencode/skills/memory/SKILL.md` |
| Session (storage, SQLite, checkpoint, import/export) | `.opencode/skills/session/SKILL.md` |
| Storage (SQLite initialization, pragmas, pooling) | `.opencode/skills/storage/SKILL.md` |
| Upgrade (GitHub releases, self-upgrade) | `.opencode/skills/upgrade/SKILL.md` |
| Worktree (git worktrees, find_git_root) | `.opencode/skills/worktree/SKILL.md` |
| Subagent (SubAgentPool, SubAgentSpawner, worker) | `.opencode/skills/subagent/SKILL.md` |
| Compaction (context compaction strategies) | `.opencode/skills/compaction/SKILL.md` |
| Router (model auto-routing) | `.opencode/skills/router/SKILL.md` |
| Util (clipboard, fuzzy matching, truncation, metrics) | `.opencode/skills/util/SKILL.md` |
| Context (artifact storage, projection, context_read, cache-aware packer observation layer, `NormalizedProviderUsage`, `EffectiveCostAnalysis`, volatile-tail compaction) | `.opencode/skills/context/SKILL.md` |
| Architecture Review (guide for reviewing architecture docs against actual source code) | `.opencode/skills/architecture-review/SKILL.md` |

## Testing Commands

```bash
# Always run before/after changes
cargo build --all-features
cargo clippy --all-features -- -D warnings
cargo test --all-features

# Specific feature testing
cargo test --all-features -- --test-threads=1  # For integration tests

# TUI tests
cargo test tui::input
cargo test tui
cargo test messages

# Run specific module tests
cargo test --package codegg -- <module>_test_pattern

# Test the native tool crates
cargo test -p eggsentry
cargo test -p eggcontext
cargo test -p egggit
cargo test -p egglsp

# LSP integration tests (fake server + production harness, no network, require lsp-test-support feature)
cargo test -p egglsp --features lsp-test-support --test production_protocol_stdio
cargo test -p egglsp --features lsp-test-support --test production_semantic_stdio
cargo test -p egglsp --features lsp-test-support --test production_service_stdio
cargo test -p egglsp --features lsp-test-support --test scenario_engine
# Root composite tests (semantic/security/hunk collectors + preview safety)
cargo test --features lsp-test-support --test lsp_composite_stdio
# Force single-threaded to validate sequential stability (parallel-safe by default)
cargo test -p egglsp --features lsp-test-support --tests -- --test-threads=1
# Run real-server compatibility tests (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server -- --ignored

# Real-server Tier 1 tests (opt-in, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke

# Test the extracted workspace crates
cargo test -p codegg-config
cargo test -p codegg-protocol
cargo test -p codegg-providers
cargo test -p codegg-core

# Quick cargo aliases (defined in .cargo/config.toml)
cargo ck           # check --workspace --all-targets
cargo ckroot       # check -p codegg
cargo ckprotocol   # check -p codegg-protocol
cargo ckconfig     # check -p codegg-config
cargo ckproviders  # check -p codegg-providers
cargo ckcore        # check -p codegg-core
cargo cksplit      # check split crates (protocol, config, providers) + root (codegg)
```

## Security Reminders

- Security-sensitive changes require additional test coverage
- SSRF protection follows RFC 6892
- Command injection follows OWASP Cheat Sheets
- Path traversal follows OWASP File Upload guidance
- Feature gates: Changes to server/plugin modules need `--all-features` testing
- Security finding synthesis uses same-file evidence scoping — different-file evidence never supports a finding
- Content preflight is heuristic and deterministic, not proof of exploitability
