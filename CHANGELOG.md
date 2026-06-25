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
  `/worktree`, `/editor`, `/loop`, `/lsp-status`, `/tasks`, `/task-del`, `/memory`,
  `/memory-search`, `/memory-list`, `/memory-remember`,
  `/memory-forget`, `/memory-consolidate`, `/checkpoint`, `/goal`,
  `/plan`, `/state`, `/pr`, `/issue`, `/review`, `/diff`, `/tests`,
  `/revert`, `/research`, `/research-runs`, `/research-open`,
  `/research-show`, `/search`, `/doctor`, `/tool-backends`,
  `/security-review`, `/security-review-show`, `/security-review-cancel`,
  `/commit`, `/init`, `/skill:*`, `/skills`, plus `/exit` aliases.

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