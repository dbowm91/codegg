# Error Module

The `error` module provides centralized error handling using `thiserror`.

## Overview

**Location**: `src/error/`

**Key Responsibilities**:
- Unified error enum
- Error context propagation
- Display formatting

## AppError Enum

```rust
#[derive(Error, Debug)]
pub enum AppError {
    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Tool error: {0}")]
    Tool(#[from] ToolError),

    #[error("Permission error: {0}")]
    Permission(#[from] PermissionError),

    #[error("Session error: {0}")]
    Session(#[from] SessionError),

    #[error("MCP error: {0}")]
    Mcp(#[from] McpError),

    #[error("LSP error: {0}")]
    Lsp(#[from] LspError),

    #[error("Plugin error: {0}")]
    Plugin(#[from] PluginError),

    #[error("Storage error: {0}")]
    Storage(#[from] StorageError),

    #[error("Config error: {0}")]
    Config(#[from] ConfigError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{context}: {error}")]
    Context { context: String, error: Box<AppError> },
}
```

## Error Categories

### ProviderError

```rust
pub enum ProviderError {
    ApiError { message: String, code: Option<u16> },
    RateLimit { retry_after: Duration },
    AuthError { message: String },
    ModelNotFound { model: String },
    InvalidRequest { message: String },
}
```

### ToolError

```rust
pub enum ToolError {
    ExecutionFailed { message: String },
    InvalidParams { message: String },
    NotFound { tool: String },
    PermissionDenied { tool: String },
    Timeout { tool: String },
}
```

### PermissionError

```rust
pub enum PermissionError {
    Denied { tool: String },
    DoomLoopDetected { tool: String },
    InvalidRule { rule: String },
}
```

## See Also

- All modules use `AppError` for error handling
