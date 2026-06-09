# codegg-core Extraction — Completed

## Date

2026-06-09 (updated 2026-06-09 with hardening results)

## What was extracted

The `crates/codegg-core` workspace crate was created and hardened. The following modules were moved from root `codegg`:

| Module | Files | Notes |
|--------|-------|-------|
| `bus` | 3 files | Event bus, permission/question registries |
| `error` | 1 file | All error enums; axum IntoResponse impls stay root-side via newtype wrappers |
| `goal` | 6 files | Goal runtime, budget enforcement, store |
| `memory` | 2 files | Persistent memory patterns |
| `model_profile` | 4 files | Model profile resolution and policy |
| `protocol_conversions` | 1 file | Core-safe conversions (session/message/provider/config types) |
| `resilience` | 1 file | Circuit breaker re-export from codegg-providers |
| `session` | 11 files | Session storage, schema, checkpoint |
| `snapshot` | 2 files | File state capture and diff |
| `storage` | 2 files | SQLite initialization, preferences |
| `task_state` | 1 file | Todo state management |
| `worktree` | 1 file | Git worktree operations |

## What stayed root-side and why

| Module | Reason |
|--------|--------|
| `agent` | High coupling to tool, permission, provider, TUI |
| `tool` | Depends on agent, permission, sandbox, LSP |
| `permission` | Depends on tool, agent |
| `mcp` | Depends on provider, auth, crypto |
| `plugin` | Depends on wasmtime, hooks |
| `tui` | Depends on ratatui, crossterm |
| `server` | Depends on axum, tower-http |
| `client` | Depends on tokio-tungstenite |
| `lsp` | Depends on egglsp, diagnostic types |
| `auth` | Depends on crypto, credential store |
| `crypto` | Depends on argon2, aes-gcm |
| `theme` | Depends on ratatui projection |
| `research` | Depends on agent, tool |
| `search` | Depends on grep ecosystem |
| `search_backend` | Pluggable backend for websearch/webfetch |
| `core/daemon` | Agent coupling, stores concrete agent runtime types |

## Error module split

- Pure error enums → `codegg-core/src/error.rs`
- `AxumAppError` and `AxumServerRuntimeError` newtype wrappers → `src/error.rs` (root)
- `IntoResponse` impls for axum stay root-side behind `#[cfg(feature = "server")]`

## protocol_conversions split

- Core conversions (session, message, provider, config) → `codegg-core/src/protocol_conversions.rs`
- Agent-specific conversions → `src/protocol_conversions.rs` (root)
- Root re-exports core conversions via `pub use codegg_core::protocol_conversions::*;`

## Dependency pruning results

Removed from `codegg-core` (not needed by moved modules):

| Crate | Reason |
|-------|--------|
| `eggcontext` | Not imported by any moved module |
| `globset` | Not imported by any moved module |
| `ignore` | Not imported by any moved module |
| `walkdir` | Not imported by any moved module |
| `tiktoken` | Token counting lives root-side |

Removed from root `codegg` (moved to `codegg-core` or unused):

| Crate | Reason |
|-------|--------|
| `tiktoken` | Token counting now in codegg-core |
| `shell-words` | Unused after extraction |
| `bitflags` | Unused after extraction |
| `num-traits` | Unused after extraction |

## Boundary enforcement

`scripts/check-core-boundary.sh` verifies:

1. No forbidden root-domain imports (`agent`, `tool`, `permission`, `mcp`, `plugin`, `tui`, `server`, `client`, `auth`, `crypto`, `search`, `search_backend`, `research`, `theme`, `tts`, `upgrade`) in `crates/codegg-core/src/`
2. No forbidden UI/server/plugin dependencies (`ratatui`, `crossterm`, `axum`, `tower_http`, `tokio_tungstenite`, `wasmtime`) in `codegg-core` Cargo.toml or source

## Validation commands

```bash
cargo check -p codegg-core
cargo check -p codegg
cargo check --workspace --all-targets
cargo test --workspace
bash scripts/check-core-boundary.sh
```

## Remaining extraction work

The next extraction pass should focus on:

1. `core/daemon.rs` — needs trait/factory boundary cleanup; still stores concrete agent runtime types and has agent/tool/permission coupling
2. `permission` — could extract with tool boundary work
3. `mcp` — depends on auth/crypto
4. Protocol conversions — agent-specific functions need agent module boundary

## Completed extraction passes

### Turn runtime wiring cleanup (plans/turn_runtime_wiring_cleanup.md)

- CoreRuntimeDeps now always owns a non-optional `turn_runtime: Arc<dyn TurnRuntime>` (defaults to DefaultTurnRuntime)
- CoreDaemon::TurnSubmit delegates to `self.deps.turn_runtime.run_turn()` instead of constructing DefaultTurnRuntime directly
- AgentRuntimeProvider is marked as transitional (used only internally by DefaultTurnRuntime)
- Provider validation is intentionally duplicated in the daemon for backward-compatible error response shape
- Fake turn runtime injection test proves the wiring works
