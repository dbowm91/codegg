use thiserror::Error;

#[cfg(feature = "server")]
use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

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

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("config file not found: {0}")]
    NotFound(String),

    #[error("invalid config: {0}")]
    Invalid(String),

    #[error("failed to parse config: {0}")]
    Parse(String),

    #[error("config merge error: {0}")]
    Merge(String),

    #[error("config watch error: {0}")]
    Watch(String),
}

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("database error: {0}")]
    Database(String),

    #[error("migration error: {0}")]
    Migration(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("llm operation failed: {operation}: {message}")]
    LlmOperation { operation: String, message: String },
}

impl From<sqlx::Error> for StorageError {
    fn from(e: sqlx::Error) -> Self {
        StorageError::Database(e.to_string())
    }
}

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("provider not found: {0}")]
    NotFound(String),

    #[error("api error: {code}: {message}")]
    Api {
        code: String,
        message: String,
        url: String,
    },

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
    pub fn api(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Api {
            code: code.into(),
            message: message.into(),
            url: String::new(),
        }
    }

    pub fn api_with_url(
        code: impl Into<String>,
        message: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        Self::Api {
            code: code.into(),
            message: message.into(),
            url: url.into(),
        }
    }
}

impl From<String> for ProviderError {
    fn from(s: String) -> Self {
        Self::Api {
            code: "unknown".to_string(),
            message: s,
            url: String::new(),
        }
    }
}

impl From<&str> for ProviderError {
    fn from(s: &str) -> Self {
        Self::Api {
            code: "unknown".to_string(),
            message: s.to_string(),
            url: String::new(),
        }
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(e: reqwest::Error) -> Self {
        let url = e.url().map(|u| u.to_string()).unwrap_or_default();
        Self::Api {
            code: "request_error".to_string(),
            message: e.to_string(),
            url,
        }
    }
}

#[cfg(feature = "server")]
impl IntoResponse for AppError {
    fn into_response(self) -> Response<Body> {
        let status = StatusCode::INTERNAL_SERVER_ERROR;

        match &self {
            AppError::Storage(_) | AppError::Provider(_) | AppError::Agent(_) => {
                tracing::debug!(error = ?self, "internal error for client response");
            }
            _ => {
                tracing::warn!(error = ?self, "propagating error to client");
            }
        }

        let body = serde_json::json!({
            "error": "An internal error occurred",
            "code": status.as_u16(),
        });

        let mut response = Json(body).into_response();
        *response.status_mut() = status;
        response
    }
}

#[derive(Error, Debug)]
pub enum AgentError {
    #[error("agent not found: {0}")]
    NotFound(String),

    #[error("invalid agent config: {0}")]
    Invalid(String),
}

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

#[derive(Error, Debug)]
pub enum PermissionError {
    #[error("permission denied for {tool} on {path}")]
    Denied { tool: String, path: String },

    #[error("permission check failed: {0}")]
    Check(String),
}

#[derive(Error, Debug)]
pub enum McpError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("server error: {0}")]
    Server(String),

    #[error("tool call failed: {0}")]
    ToolCall(String),

    #[error("oauth error: {0}")]
    OAuth(String),

    #[error("encryption error: {0}")]
    Encryption(String),

    #[error("timeout: {0}")]
    Timeout(String),
}

#[derive(Error, Debug)]
pub enum LspError {
    #[error("server not found: {0}")]
    ServerNotFound(String),

    #[error("server download failed: {0}")]
    DownloadFailed(String),

    #[error("server launch failed: {0}")]
    LaunchFailed(String),

    #[error("client not initialized: {0}")]
    NotInitialized(String),

    #[error("request failed: {0}")]
    RequestFailed(String),

    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("plugin not found: {0}")]
    NotFound(String),

    #[error("plugin load failed: {0}")]
    LoadFailed(#[from] crate::plugin::loader::LoadError),

    #[error("plugin hook failed: {0}")]
    HookFailed(String),

    #[error("plugin install failed: {0}")]
    InstallFailed(#[from] crate::plugin::install::InstallError),

    #[error("plugin manifest invalid: {0}")]
    InvalidManifest(String),
}

#[derive(Error, Debug)]
pub enum ServerRuntimeError {
    #[error("server bind failed: {0}")]
    Bind(String),

    #[error("server shutdown error: {0}")]
    Shutdown(String),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("rpc error: {0}")]
    Rpc(String),

    #[error("authentication failed: {0}")]
    Auth(String),
}

#[cfg(feature = "server")]
impl IntoResponse for ServerRuntimeError {
    fn into_response(self) -> Response<Body> {
        let status = match &self {
            ServerRuntimeError::Auth(_) => StatusCode::UNAUTHORIZED,
            ServerRuntimeError::Bind(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerRuntimeError::Shutdown(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerRuntimeError::WebSocket(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerRuntimeError::Rpc(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = serde_json::json!({
            "error": format!("{:?}", self),
            "code": status.as_u16(),
        });
        let mut response = Json(body).into_response();
        *response.status_mut() = status;
        response
    }
}

#[derive(Error, Debug)]
pub enum ClientError {
    #[error("connection failed: {0}")]
    Connection(String),

    #[error("server not reachable: {0}")]
    Unreachable(String),

    #[error("rpc error: {0}")]
    Rpc(String),

    #[error("websocket error: {0}")]
    WebSocket(String),

    #[error("authentication failed: {0}")]
    Auth(String),
}
