---
name: error
description: AppError, ProviderError, ToolError, is_retryable, CircuitOpen error handling
version: 1.1.0
tags: [error, exception, provider, tool]
---

# Error Module Skill (v1.1.0)

## Overview

Error handling in this codebase uses `thiserror` for derive-based error enums with `AppError` as the central unifying type.

## AppError Structure

`AppError` is a root error enum that wraps domain-specific errors:

```rust
pub enum AppError {
    Config(ConfigError),
    Storage(StorageError),
    Provider(ProviderError),
    Agent(AgentError),
    Tool(ToolError),
    Permission(PermissionError),
    Mcp(McpError),
    Plugin(PluginError),
    Lsp(LspError),
    Io(std::io::Error),
    Json(serde_json::Error),
    Http(reqwest::Error),
    Other(anyhow::Error),
    Worktree(String),
    Upgrade(String),
    Clipboard(String),
    Tui(String),
}
```

## Key Methods

### ProviderError::is_retryable()

Determines if a provider error should trigger retry:

```rust
impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimit
                | ProviderError::Timeout(_)
                | ProviderError::Stream(_)
                | ProviderError::CircuitOpen(_)
                | ProviderError::Auth(_)
        )
    }
}
```

Used in `agent/loop.rs` for retry logic and `tool/executor.rs` for tool retry.

### ToolError::is_retryable()

```rust
impl ToolError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_)
        )
    }
}
```

## Error Variants

### StorageError Variants

```rust
pub enum StorageError {
    Database(String),      // sqlx errors
    Migration(String),     // schema migration errors
    NotFound(String),      // resource not found
    LlmOperation { operation: String, message: String },
    Import(String),        // session import errors
    Export(String),        // session export errors
}
```

## Error Conversions

| From | To | Usage |
|------|-----|-------|
| `CircuitError::Open(name)` | `ProviderError::CircuitOpen(name)` | Circuit breaker integration |
| `sqlx::Error` | `StorageError::Database` | Database errors |
| `reqwest::Error` | `ProviderError::Api` | HTTP errors with URL capture |
| `String` / `&str` | `ProviderError::Api` | Anonymous API errors |

## Exec Mode Error Classification

`exec.rs` classifies errors for JSON output. The `classify_error()` function uses direct type imports for cleaner pattern matching:

```rust
use crate::error::{AppError, ProviderError, ToolError};
// ...
AppError::Tool(ToolError::NotFound(_)) => { ... }
AppError::Tool(ToolError::Timeout(_)) => { ... }
AppError::Tool(ToolError::Permission(_)) => { ... }
AppError::Tool(ToolError::Disabled(_)) => { ... }
```

| Error Type | Code | Message |
|------------|------|---------|
| Permission | PERMISSION_ERROR | Permission denied |
| ProviderError::Auth | AUTH_ERROR | Authentication failed |
| ProviderError::RateLimit | RATE_LIMIT | Rate limit exceeded |
| ProviderError::Timeout | TIMEOUT | Request timed out |
| ProviderError::ModelNotFound | MODEL_NOT_FOUND | Model not found |
| ProviderError::CircuitOpen | CIRCUIT_OPEN | Provider circuit open |
| ProviderError::Api | API_ERROR | API error [code]: message |
| ProviderError::Stream | STREAM_ERROR | Stream error |
| ProviderError::NotFound | PROVIDER_NOT_FOUND | Provider not found |
| Storage | STORAGE_ERROR | Storage error |
| ToolError::NotFound | TOOL_NOT_FOUND | Tool not found |
| ToolError::Timeout | TOOL_TIMEOUT | Tool timeout |
| ToolError::Permission | TOOL_PERMISSION | Tool permission denied |
| ToolError::Disabled | TOOL_DISABLED | Tool disabled |
| Mcp | MCP_ERROR | MCP error |
| Lsp | LSP_ERROR | LSP error |
| Plugin | PLUGIN_ERROR | Plugin error |
| Agent | AGENT_ERROR | Agent error |
| Json | JSON_ERROR | JSON error |
| Http | HTTP_ERROR | HTTP error |
| Other | EXECUTION_ERROR | Execution error |
| Worktree | WORKTREE_ERROR | Worktree error |
| Upgrade | UPGRADE_ERROR | Upgrade error |
| Clipboard | CLIPBOARD_ERROR | Clipboard error |
| Tui | TUI_ERROR | TUI error |

## HTTP Status Mapping

When the server feature is enabled, `AppError` implements `IntoResponse` for Axum:

- 400: ConfigError (Invalid/Parse/Merge), LspError::UnsupportedLanguage, PluginError::InvalidManifest, Json errors
- 401: ProviderError::Auth, McpError::OAuth, ServerRuntimeError::Auth
- 403: ToolError::Permission, ToolError::Disabled, PermissionError::Denied
- 404: ConfigError::NotFound, StorageError::NotFound, ProviderError::NotFound/ModelNotFound, ToolError::NotFound, PluginError::NotFound, LspError::ServerNotFound, AgentError::NotFound
- 409: LspError::NotInitialized
- 429: ProviderError::RateLimit
- 500: StorageError::Database/Migration/LlmOperation/Import/Export, ToolError::Execution/Format/Io/Network, PermissionError::Check, PluginError::LoadFailed/HookFailed/InstallFailed, LspError::Io/Json, ServerRuntimeError, Other/Io/Worktree/Upgrade/Clipboard/Tui variants
- 502: ProviderError::Api/Stream/CircuitOpen, ToolError (some), McpError::Connection/Server/ToolCall/Encryption, LspError::DownloadFailed/LaunchFailed/RequestFailed
- 504: ProviderError::Timeout, ToolError::Timeout, McpError::Timeout

Note: StorageError::Import and Export map to 500 (Internal Server Error).

## Security Note

Error responses use canonical reason strings (e.g., "Unauthorized") rather than actual error details to prevent information leakage. Tests verify that secrets don't appear in error responses.

## Adding New Error Types

1. Add variant to appropriate error enum with `#[error(...)]` attribute
2. If it should convert to `AppError`, add `#[from]` attribute and implement `From<NewError>` for `AppError`
3. Add HTTP status mapping in `IntoResponse` impl if server feature enabled
4. Add error classification in `exec.rs::classify_error()` if exec mode is relevant
5. Update architecture/error.md and this skill file
6. Add test case in the `#[cfg(all(test, feature = "server"))]` module