# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Built-in Language Server Protocol (LSP) support with capability gating,
  preview-only semantic edits, semantic context packets, semantic check
  previews, and security/hunk context operations. Authoritative
  implementation in `crates/egglsp/`; 39 language server configurations
  available. Phase 6 added `/lsp-status` command, `counts_from_packet`
  flag for accurate status rendering, support-tier documentation, and
  troubleshooting guide.
- Native crate extraction: `egglsp`, `egggit`, `eggsentry`, `eggcontext`,
  `codegg-config`, `codegg-protocol`, `codegg-providers`, `codegg-core`
  (see `architecture/native_crates.md`).
- Typed `AuthConfig`, `AuthResolver`, and user-level encrypted credential
  store at `~/.config/codegg/credentials.json` with `codegg auth status |
  set-key | logout` CLI.
- Security review workflow (`/security-review`) with diff-based preset
  selection, evidence-based finding synthesis, opt-in LSP enrichment,
  opt-in `hunkSourceContext` evidence, structured `SecurityReviewReceipt`,
  result panel (`/security-review-show`), and cancellation
  (`/security-review-cancel`).
- Theme system with 50 bundled Halloy-format themes and live-preview
  picker; SQLite-persisted active theme.
- Long-horizon goal runtime with four-axis budget enforcement
  (turns, tokens, tool calls, wall-clock), durable wall-clock across
  session restarts, and `codegg goal` / `/goal` surfaces.
- Cache-aware context packing (observe-only layer), hardened gated
  context-policy layer (tool-palette reduction, base-derived, with
  backoff/starvation detection and Warn dry-run), and volatile-tail
  compaction for late-context token reduction of old tool results with
  recovery handles.
- Server mode (Axum) with HTTP REST, WebSocket TUI protocol, SSE event
  stream, session CRUD, and token-based auth (feature-gated).
- MCP (Model Context Protocol) client with local and remote transports,
  exponential-backoff reconnect, OAuth device-flow scaffolding, and DNS
  re-validation on each connect.
- WASM plugin system with hooks (feature-gated).
- TTS module (macOS `say`).
- Goal budget slash command (`/goal budget show|raise <axis> <n>`).
- TUI slash commands: `/help`, `/tree`, `/model`, `/agent`, `/new`,
  `/compact`, `/connect`, `/status`, `/context`, `/cost`, `/usage`,
  `/themes`, `/tui`, `/sessions`, `/goto`, `/share`, `/unshare`,
  `/timeline`, `/undo`, `/redo`, `/export`, `/import`, `/timestamps`,
  `/thinking`, `/models-refresh`, `/variants`, `/mcps`, `/fork`,
  `/worktree`, `/editor`, `/loop`, `/lsp-status`, `/lsp-servers`,
  `/lsp-capabilities`, `/lsp-errors`, `/lsp-root`, `/lsp-restart`,
  `/lsp-stop`, `/lsp-preview-apply`, `/tasks`, `/task-del`, `/memory`,
  `/memory-search`, `/memory-list`, `/memory-remember`,
  `/memory-forget`, `/memory-consolidate`, `/checkpoint`, `/goal`,
  `/plan`, `/state`, `/pr`, `/issue`, `/review`, `/diff`, `/tests`,
  `/revert`, `/research`, `/research-runs`, `/research-open`,
  `/research-show`, `/search`, `/doctor`, `/tool-backends`,
  `/security-review`, `/security-review-show`, `/security-review-cancel`,
  `/commit`, `/init`, `/skill:*`, `/skills`, plus `/exit` aliases.
- Phase 9 LSP lifecycle commands: `/lsp-servers` (list active servers
  with root, state, generation, capabilities, and supported features),
  `/lsp-capabilities <key>` (effective capability snapshot for a server),
  `/lsp-errors <key>` (error history and health info),
  `/lsp-root <path>` (diagnose workspace root detection without starting
  servers), `/lsp-restart <key>` (manually restart a server),
  `/lsp-stop [key]` (stop all or a specific server). `/lsp-preview-apply`
  now applies patches directly with hash revalidation instead of
  read-only export. Lifecycle-state warnings in agent context (indexing,
  degraded, restarting, failed states produce explicit notes).

  **Deferred:** `/lsp-start` and `/lsp-replay-docs` commands deferred to
  a future phase. Per-key server stop uses `shutdown_all` fallback (stop
  per-key requires service API changes).

- LSP semantic memory cache (Phase 12): optional bounded in-memory cache
  for LSP-derived evidence packets. Disabled by default; opt-in via
  `[lsp_semantic_cache]` config (`mode = "memory"`, `max_entries = 64`,
  `max_bytes = 4194304`, `ttl_seconds = 300`). Cache keys encode workspace
  root, server ID, operation, request fingerprint, file content hashes,
  capability fingerprint, and budget fingerprint. Cache hits preserve or
  downgrade freshness correctly (e.g., `RetainedAfterRestart` after server
  generation change). `collect_context_cached()` wraps `collect_context()`
  with cache lookup/insert. TUI commands: `/lsp-cache-status`,
  `/lsp-cache-clear [--all|<root>]`. Never caches across workspace roots.
  Disk persistence explicitly deferred.

### Hardening (Phase 9–12 closeout)

- Phase 9 preview apply is gated by `egglsp::tui_summary::validate_preview_apply`,
  a testable boundary that performs all checks (not-found, stale-base,
  no-patches, already-applied, hash mismatch, patch failure) in memory and
  returns a typed `PreviewApplyPlan` without writing to disk. The TUI
  handler performs the actual `std::fs::write` calls and only calls
  `mark_preview_applied` after every write succeeds; failed writes leave
  the preview pending. Write-side hardening via
  `write_preview_apply_plan_atomically_enough()` performs per-file SHA-256
  recheck before each write; `PreviewApplyWriteReport` tracks per-file
  successes/failures; `mark_preview_applied` only called on full success;
  partial failures reported without marking applied. 10 new tests prove the
  write-side invariant.
- Phase 10 known notes-text bug: `crates/egglsp/src/evidence_collector.rs:1633`
  emits the `"references capped"` note when references are **not** capped
  (inverted comparison). Underlying reference count and budget enforcement
  are correct. Tracked as a follow-up.
- Phase 11 known limitation: `LspContextRenderConfig` does not currently
  expose `include_cross_file` / `include_hierarchy` fields, so
  `to_render_config()` does not propagate those policy flags. The
  `RecipeSettings` path (`to_recipe_settings()`) is unaffected.
- Phase 12 production wiring: `LspTool::lsp_context_for_agent_with_input`
  now routes through the cache when enabled, via the sync
  `LspSemanticCache::get` / `insert` API (rather than
  `collect_context_cached`) because the cache guard is `!Send` and cannot
  cross `.await`. Production cache keys now include request-scoped file
  hashes via `collect_cache_file_hashes_for_request()` in
  `src/tool/lsp.rs` (cap of 16 files with debug logging). When the primary
  file is unreadable, cache is bypassed for that request. Pattern: lock,
  lookup, drop lock, await `collect_context` on miss, lock again, insert.
  Unit tests cover `with_cache_config` propagation, `lsp_cache_status`
  reporting, and `clear_semantic_cache` zero-clear behavior in disabled
  mode. Cache eviction is conservative: generation mismatch, file hash
  change, TTL expiry, and capability fingerprint change all remove entries.
- Phase 9–12 safety sweep: all 3 static searches passed with 0 disallowed
  matches. `workspace/applyEdit` is rejected by the dispatcher.
  `workspace/executeCommand` is never invoked. `mark_applied` is only
  called after all writes succeed.

### Security

- SSRF protection with IPv6 ULA/multicast blocking (`fc00::/7`,
  `ff00::/8`) and DNS rebinding protection in MCP client.
- Symlink validation before canonicalization in `security/sandbox.rs`.
- `env_clear()` and minimal safe `PATH` for subprocess invocations.
- AES-256-GCM encryption with Argon2id key derivation for the credential
  store (`src/crypto/mod.rs`).
- Landlock filesystem sandboxing for the bash tool.
- Error redaction (`redact_local_paths()`) so internal paths never
  leak into LLM-facing error messages.
- `#![deny(unsafe_code)]` at the crate root.

## [0.1.0] - 2024-01-01

### Added

- Initial release
- Pure Rust implementation
- Multiple LLM provider support (Anthropic, OpenAI, Google, Azure,
  Bedrock, and more)
- Built-in Language Server Protocol (LSP) support
- WASM-based plugin system
- Terminal user interface (TUI) with syntax highlighting
- Server mode for headless HTTP access
- Persistent session management with SQLite
- Context compaction for long conversations
- Tool system with bash, read, edit, and task capabilities
- MCP (Model Context Protocol) client support
- Security features including SSRF protection and Landlock sandboxing