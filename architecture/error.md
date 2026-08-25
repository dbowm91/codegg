# Error Module

The error module provides centralized error handling using `thiserror`.

## Purpose

Centralized error enum (`AppError`) with per-domain sub-errors, error
context propagation via `From` trait implementations, HTTP status mapping
for server responses, and retryability determination for resilience
patterns.

## Where It Lives

- **Canonical source**: `crates/codegg-core/src/error.rs` — defines
  `AppError` and all domain error enums except `ConfigError` (which wraps
  `codegg_config::ConfigError`).
- **Root re-export + server wrappers**: `src/error.rs` — re-exports
  `codegg_core::error::*` and adds `AxumAppError` / `AxumServerRuntimeError`
  behind `#[cfg(feature = "server")]`.

> **Note:** `ProviderError` and `StorageError` originate in
> `codegg_providers::error` and are re-exported into `codegg_core::error`
> via `pub use codegg_providers::error::{ProviderError, StorageError};`.

## How It Works

All domain error types live in `crates/codegg-core/src/error.rs`. The
`AppError` enum wraps each domain error via `#[from]` so the `?` operator
can convert automatically. The root `src/error.rs` re-exports everything
and adds axum-specific newtype wrappers that implement `IntoResponse`.

The `IntoResponse` implementation maps each `AppError` variant to an HTTP
status code. Server errors (5xx) are logged at `error` level; client
errors (4xx) at `warn`. Response bodies use canonical reason phrases
without leaking internal details.

## Key Types & APIs

### AppError (`crates/codegg-core/src/error.rs:5`)

```rust
#[derive(Error, Debug)]
pub enum AppError {
    Config(ConfigError),        // #[from]
    Storage(StorageError),      // #[from]
    Provider(ProviderError),    // #[from]
    Agent(AgentError),          // #[from]
    Tool(ToolError),            // #[from]
    Permission(PermissionError),// #[from]
    Mcp(McpError),              // #[from]
    Plugin(PluginError),        // #[from]
    Lsp(LspError),              // #[from]
    Io(std::io::Error),         // #[from]
    Json(serde_json::Error),    // #[from]
    Http(reqwest::Error),       // #[from]
    Other(anyhow::Error),       // #[from]
    Worktree(String),
    Upgrade(String),
    Clipboard(String),
    Tui(String),
    RunStore(RunStoreError),    // #[from]
}
```

### ConfigError (`crates/codegg-core/src/error.rs:62`)

Wraps `codegg_config::ConfigError` with an explicit `From` impl (line 80).
Variants: `NotFound`, `Invalid`, `Parse`, `Merge`, `Watch`.

### ProviderError (re-exported from `codegg_providers::error`)

Variants: `NotFound`, `Api { code, message, url }`, `Stream`, `RateLimit`,
`Auth`, `ModelNotFound`, `Timeout`, `CircuitOpen`.

Constructors: `api()` (empty URL), `api_with_url()`. `is_retryable()`
returns `true` for `RateLimit`, `Timeout`, `Stream`, `CircuitOpen`, `Auth`.

### ToolError (`crates/codegg-core/src/error.rs:119`)

Variants: `NotFound`, `Execution`, `Timeout`, `Permission`, `Format`,
`Disabled`, `Io`, `Network`. `is_retryable()` for `Io`, `Network`, `Timeout`.

### PermissionError (`crates/codegg-core/src/error.rs:167`)

Variants: `Denied { tool, path }`, `Check`.

### McpError (`crates/codegg-core/src/error.rs:176`)

Variants: `Connection`, `Server`, `ToolCall`, `OAuth`, `Encryption`,
`Timeout`. `is_retryable()` for `Connection`, `Server`, `ToolCall`,
`OAuth`, `Timeout`. `Encryption` is intentionally NOT retryable.

### LspError (`crates/codegg-core/src/error.rs:271`)

Variants: `ServerNotFound`, `DownloadFailed`, `LaunchFailed`,
`NotInitialized`, `RequestFailed`, `RequestTimeout`, `UnsupportedLanguage`,
`Io`, `Json`, `UnsupportedSourceAction`, `CommandOnlySourceAction`,
`NoEditForSourceAction`, `AmbiguousSourceAction`, `CommandOnlyCodeAction`,
`Unsupported(LspUnavailable)`.

`is_retryable()` for `DownloadFailed`, `LaunchFailed`, `RequestFailed`,
`RequestTimeout`, `Io`.

Note: `egglsp::LspError` has additional variants (`UnsupportedEdit`,
`PathOutsideRoot`, `Utf16Position`, `OverlappingEdits`, `Protocol`,
`WriterClosed`, `InitializationCancelled`, `ServerRestarted`,
`ServerUnavailable`, `ServerDegraded`, `InvalidConfig`) that are
collapsed into `RequestFailed` by the `From` conversion (line 210).

### PluginError (`crates/codegg-core/src/error.rs:335`)

Variants: `NotFound`, `LoadFailed`, `HookFailed`, `InstallFailed`,
`InvalidManifest`.

### ServerRuntimeError (`crates/codegg-core/src/error.rs:353`)

Variants: `Bind`, `Shutdown`, `WebSocket`, `Rpc`, `Auth`.

### ClientError (`crates/codegg-core/src/error.rs:371`)

Variants: `Connection`, `Unreachable`, `Rpc`, `WebSocket`, `Auth`.

### RunStoreError (`crates/codegg-core/src/error.rs:389`)

Variants: `Io`, `Json`, `NotFound`, `PathTraversal`, `IntegrityViolation`,
`RetentionError`, `ConcurrentWrite`.

### AgentError (`crates/codegg-core/src/error.rs:110`)

Variants: `NotFound`, `Invalid`.

### AxumAppError (`src/error.rs:16`)

Newtype wrapper for `AppError` implementing `IntoResponse`. Also has `From`
impls for `StorageError`, `std::io::Error`, `serde_json::Error`,
`anyhow::Error`, and `reqwest::Error` so `?` works directly in axum
handlers.

### AxumServerRuntimeError (`src/error.rs:171`)

Newtype wrapper for `ServerRuntimeError` implementing `IntoResponse`.

## Configuration Surface

None. Error types are determined by the codebase, not configuration.

## Invariants & Gotchas

- **Orphan rule**: `AppError` lives in `codegg-core` but `IntoResponse`
  is implemented in root `src/error.rs` via newtype wrappers because axum
  is a forbidden dependency of `codegg-core`.
- **ConfigError bridge**: `codegg_config::ConfigError` is converted to
  `codegg_core::error::ConfigError` via explicit `From` impl (line 80),
  not a direct `#[from]` on `AppError`.
- **McpError::Encryption not retryable**: Intentional; encryption failures
  require manual intervention.
- **ProviderError::api()** sets `url` to empty string. Use
  `api_with_url()` when the URL is available.
- **LspError variant collapse**: Several `egglsp::LspError` variants are
  collapsed into `LspError::RequestFailed` by the `From` conversion.
  Callers see less granularity than the LSP crate provides.

## HTTP Status Mapping (`src/error.rs:63-147`)

| Error Type | Status |
|------------|--------|
| Config::NotFound | 404 |
| Config::Invalid/Parse/Merge | 400 |
| Config::Watch | 500 |
| Storage::NotFound | 404 |
| Storage::Database/Migration/Import/Export/LlmOperation | 500 |
| Provider::Auth | 401 |
| Provider::RateLimit | 429 |
| Provider::Timeout | 504 |
| Provider::NotFound/ModelNotFound | 404 |
| Provider::Api/Stream/CircuitOpen | 502 |
| Agent::NotFound | 404 |
| Agent::Invalid | 400 |
| Tool::NotFound | 404 |
| Tool::Permission / Permission::Denied | 403 |
| Tool::Timeout | 504 |
| Tool::Disabled | 403 |
| Tool::Execution/Format/Io/Network | 502 |
| Permission::Check | 500 |
| Mcp::OAuth | 401 |
| Mcp::Timeout | 504 |
| Mcp::Connection/Server/ToolCall/Encryption | 502 |
| Plugin::NotFound | 404 |
| Plugin::InvalidManifest | 400 |
| Plugin::LoadFailed/HookFailed/InstallFailed | 500 |
| Lsp::ServerNotFound | 404 |
| Lsp::UnsupportedLanguage | 400 |
| Lsp::NotInitialized | 409 |
| Lsp::RequestTimeout/DownloadFailed/LaunchFailed/RequestFailed + source action errors + CommandOnlyCodeAction + Unsupported | 502 |
| Lsp::Io/Json | 500 |
| Json | 400 |
| Http | upstream status or 502 |
| Io/Other/Worktree/Upgrade/Clipboard/Tui/RunStore | 500 |

**ServerRuntimeError IntoResponse** (`src/error.rs:181-206`):

| Status | Variants |
|--------|----------|
| 401 | `Auth` |
| 500 | `Bind`, `Shutdown`, `WebSocket`, `Rpc` |

## Key Conversions

| From | To | Notes |
|------|-----|-------|
| `codegg_config::ConfigError` | `ConfigError` | Explicit `From` impl |
| `codegg_config::AppError` | `AppError` | Matches Config/Io/Other |
| `sqlx::Error` | `StorageError::Database` | Via `codegg-providers` |
| `reqwest::Error` | `ProviderError::Api` | HTTP failures; `.url()` extracts endpoint |
| `CircuitError::Open` | `ProviderError::CircuitOpen` | Circuit breaker integration |
| `egglsp::LspError` | `LspError` | Several variants collapsed to `RequestFailed` |
| `eggsentry::EggsecError` | `ToolError` | Io/FileTooLarge/Join mapped |

## Testing

```bash
cargo test -p codegg --features server -- error::tests
```

Tests verify: HTTP status mapping for each error family, canonical reason
phrases in response bodies, no secret leakage in error messages, and
`ServerRuntimeError` status mapping.

## Related Docs

- `architecture/core.md` — root `src/error.rs` rationale and boundary
- `resilience/` — Circuit breaker patterns using `is_retryable()`
- `exec/` — Exec mode error classification
- `provider/` — Provider retry logic using `is_retryable()`
