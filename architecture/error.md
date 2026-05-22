# Error Module

The `error` module provides centralized error handling using `thiserror`.

## Overview

**Location**: `src/error.rs`

**Key Responsibilities**:
- Unified error enum (`AppError`)
- Error context propagation via `From` trait implementations
- HTTP status mapping for server responses
- Retryability determination for resilience patterns

## AppError Enum

```rust
#[derive(Error, Debug)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(#[from] ConfigError),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("agent error: {0}")]
    Agent(#[from] AgentError),

    #[error("tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("permission error: {0}")]
    Permission(#[from] PermissionError),

    #[error("mcp error: {0}")]
    Mcp(#[from] McpError),

    #[error("plugin error: {0}")]
    Plugin(#[from] PluginError),

    #[error("lsp error: {0}")]
    Lsp(#[from] LspError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("general error: {0}")]
    Other(#[from] anyhow::Error),

    #[error("worktree error: {0}")]
    Worktree(String),

    #[error("upgrade error: {0}")]
    Upgrade(String),

    #[error("clipboard error: {0}")]
    Clipboard(String),

    #[error("tui error: {0}")]
    Tui(String),
}
```

## Error Categories

### ProviderError

```rust
#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("provider not found: {0}")]
    NotFound(String),

    #[error("api error: {code}: {message}")]
    Api { code: String, message: String, url: String },

    #[error("stream error: {0}")]
    Stream(String),

    #[error("rate limit exceeded")]
    RateLimit,

    #[error("authentication failed: {0}")]
    Auth(String),

    #[error("model not found: {0}")]
    ModelNotFound(String),

    #[error("timeout: {0}")]
    Timeout(String),

    #[error("circuit breaker open: {0}")]
    CircuitOpen(String),
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ProviderError::RateLimit
                | ProviderError::Timeout(_)
                | ProviderError::Stream(_)
                | ProviderError::CircuitOpen(_)
        )
    }
}
```

### ToolError

```rust
#[derive(Error, Debug)]
pub enum ToolError {
    #[error("tool not found: {0}")]
    NotFound(String),

    #[error("tool execution failed: {0}")]
    Execution(String),

    #[error("tool timeout: {0}")]
    Timeout(String),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("tool formatting failed: {0}")]
    Format(String),

    #[error("tool disabled: {0}")]
    Disabled(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("network error: {0}")]
    Network(String),
}

impl ToolError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            ToolError::Io(_) | ToolError::Network(_) | ToolError::Timeout(_)
        )
    }
}
```

### PermissionError

```rust
#[derive(Error, Debug)]
pub enum PermissionError {
    #[error("permission denied for {tool} on {path}")]
    Denied { tool: String, path: String },

    #[error("permission check failed: {0}")]
    Check(String),
}
```

### Other Error Types

- **ConfigError**: NotFound, Invalid, Parse, Merge, Watch
- **StorageError**: Database, Migration, NotFound, LlmOperation
- **AgentError**: NotFound, Invalid
- **McpError**: Connection, Server, ToolCall, OAuth, Encryption, Timeout
- **LspError**: ServerNotFound, DownloadFailed, LaunchFailed, NotInitialized, RequestFailed, RequestTimeout, UnsupportedLanguage, Io, Json
- **PluginError**: NotFound, LoadFailed, HookFailed, InstallFailed, InvalidManifest
- **ClientError**: Connection, Unreachable, Rpc, WebSocket, Auth (client-side)
- **ServerRuntimeError**: Bind, Shutdown, WebSocket, Rpc, Auth (server-side)

## Key Conversions

| From | To | Notes |
|------|-----|-------|
| `sqlx::Error` | `StorageError::Database` | Database errors |
| `reqwest::Error` | `ProviderError::Api` | HTTP request failures |
| `CircuitError::Open` | `ProviderError::CircuitOpen` | Circuit breaker integration |
| `String` / `&str` | `ProviderError::Api` | Helper constructors |

## HTTP Status Mapping (Server Feature)

The `IntoResponse` implementation maps errors to appropriate HTTP status codes:

| Error Type | Status Code |
|------------|-------------|
| ConfigError::NotFound | 404 |
| ConfigError::Invalid/Parse/Merge | 400 |
| StorageError::NotFound | 404 |
| StorageError::Database/Migration/LlmOperation | 500 |
| ProviderError::Auth | 401 |
| ProviderError::RateLimit | 429 |
| ProviderError::Timeout | 504 |
| ProviderError::NotFound/ModelNotFound | 404 |
| ProviderError::Api/Stream/CircuitOpen | 502 |
| ToolError::NotFound | 404 |
| ToolError::Permission | 403 |
| ToolError::Timeout | 504 |
| McpError::OAuth | 401 |
| McpError::Timeout | 504 |
| McpError::Connection/Server/ToolCall/Encryption | 502 |
| PluginError::NotFound | 404 |
| PluginError::InvalidManifest | 400 |
| PluginError::LoadFailed/HookFailed/InstallFailed | 500 |

## See Also

- `resilience/` - Circuit breaker patterns
- `exec/` - Exec mode error classification
- `provider/` - Provider retry logic using `is_retryable()`