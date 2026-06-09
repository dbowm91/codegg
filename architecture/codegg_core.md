# codegg-core Crate

## Purpose

`codegg-core` is a workspace crate containing the core runtime, session management, storage, event bus, domain state, and common error types. It is designed to be a low-coupling foundation that root `codegg` builds upon.

## Modules

| Module | Purpose |
|--------|---------|
| `bus` | Event bus (GlobalEventBus), PermissionRegistry, QuestionRegistry |
| `error` | Central error taxonomy (AppError, ToolError, etc.) |
| `goal` | Long-horizon goal runtime, budget enforcement |
| `memory` | Persistent memory patterns |
| `model_profile` | Model profile resolution and policy |
| `protocol_conversions` | Core-safe DTO↔domain conversions |
| `resilience` | Circuit breaker re-export |
| `session` | Session storage, schema, checkpoint |
| `snapshot` | File state capture and diff |
| `storage` | SQLite initialization, preferences |
| `task_state` | Todo state management |
| `worktree` | Git worktree operations |

## Dependencies

`codegg-core` depends on:
- `codegg-config` — configuration types
- `codegg-protocol` — protocol DTOs
- `codegg-providers` — provider types and circuit breaker
- `eggcontext`, `egggit`, `eggsentry` — extracted tool crates

`codegg-core` does NOT depend on:
- `axum`, `tower-http`, `tokio-tungstenite` (server/client)
- `ratatui`, `crossterm` (TUI)
- `wasmtime` (plugins)
- Root `codegg` crate

## Error Module Split

The error module is split between `codegg-core` and root:

- `codegg-core/src/error.rs` — all error enums and their `From` impls
- Root `src/error.rs` — re-exports from codegg-core + `AxumAppError`/`AxumServerRuntimeError` wrappers with `IntoResponse` impls (behind `#[cfg(feature = "server")]`)

This split is necessary because axum is not a dependency of codegg-core.

## protocol_conversions Split

- `codegg-core/src/protocol_conversions.rs` — session, message, provider, config type conversions
- Root `src/protocol_conversions.rs` — agent-specific conversions + re-exports core conversions

Agent types are not in codegg-core, so agent conversions must stay root-side.
