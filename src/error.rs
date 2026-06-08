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

    #[error("import error: {0}")]
    Import(String),

    #[error("export error: {0}")]
    Export(String),
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

impl From<crate::resilience::circuit::CircuitError> for ProviderError {
    fn from(e: crate::resilience::circuit::CircuitError) -> Self {
        match e {
            crate::resilience::circuit::CircuitError::Open(name) => {
                ProviderError::CircuitOpen(name)
            }
        }
    }
}

#[cfg(feature = "server")]
impl IntoResponse for AppError {
    fn into_response(self) -> Response<Body> {
        let status = match &self {
            AppError::Config(ConfigError::NotFound(_)) => StatusCode::NOT_FOUND,
            AppError::Config(ConfigError::Invalid(_))
            | AppError::Config(ConfigError::Parse(_))
            | AppError::Config(ConfigError::Merge(_)) => StatusCode::BAD_REQUEST,
            AppError::Config(ConfigError::Watch(_)) => StatusCode::INTERNAL_SERVER_ERROR,

            AppError::Storage(StorageError::NotFound(_)) => StatusCode::NOT_FOUND,
            AppError::Storage(StorageError::Database(_))
            | AppError::Storage(StorageError::Migration(_))
            | AppError::Storage(StorageError::Import(_))
            | AppError::Storage(StorageError::Export(_))
            | AppError::Storage(StorageError::LlmOperation { .. }) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }

            AppError::Provider(ProviderError::Auth(_)) => StatusCode::UNAUTHORIZED,
            AppError::Provider(ProviderError::RateLimit) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Provider(ProviderError::Timeout(_)) => StatusCode::GATEWAY_TIMEOUT,
            AppError::Provider(ProviderError::NotFound(_))
            | AppError::Provider(ProviderError::ModelNotFound(_)) => StatusCode::NOT_FOUND,
            AppError::Provider(ProviderError::Api { .. })
            | AppError::Provider(ProviderError::Stream(_))
            | AppError::Provider(ProviderError::CircuitOpen(_)) => StatusCode::BAD_GATEWAY,

            AppError::Agent(AgentError::NotFound(_)) => StatusCode::NOT_FOUND,
            AppError::Agent(AgentError::Invalid(_)) => StatusCode::BAD_REQUEST,

            AppError::Tool(ToolError::NotFound(_)) => StatusCode::NOT_FOUND,
            AppError::Tool(ToolError::Permission(_))
            | AppError::Permission(PermissionError::Denied { .. }) => StatusCode::FORBIDDEN,
            AppError::Tool(ToolError::Timeout(_)) => StatusCode::GATEWAY_TIMEOUT,
            AppError::Tool(ToolError::Disabled(_)) => StatusCode::FORBIDDEN,
            AppError::Tool(ToolError::Execution(_))
            | AppError::Tool(ToolError::Format(_))
            | AppError::Tool(ToolError::Io(_))
            | AppError::Tool(ToolError::Network(_)) => StatusCode::BAD_GATEWAY,

            AppError::Permission(PermissionError::Check(_)) => StatusCode::INTERNAL_SERVER_ERROR,

            AppError::Mcp(McpError::OAuth(_)) => StatusCode::UNAUTHORIZED,
            AppError::Mcp(McpError::Timeout(_)) => StatusCode::GATEWAY_TIMEOUT,
            AppError::Mcp(McpError::Connection(_))
            | AppError::Mcp(McpError::Server(_))
            | AppError::Mcp(McpError::ToolCall(_))
            | AppError::Mcp(McpError::Encryption(_)) => StatusCode::BAD_GATEWAY,

            AppError::Plugin(PluginError::NotFound(_)) => StatusCode::NOT_FOUND,
            AppError::Plugin(PluginError::InvalidManifest(_)) => StatusCode::BAD_REQUEST,
            AppError::Plugin(PluginError::LoadFailed(_))
            | AppError::Plugin(PluginError::HookFailed(_))
            | AppError::Plugin(PluginError::InstallFailed(_)) => StatusCode::INTERNAL_SERVER_ERROR,

            AppError::Lsp(LspError::ServerNotFound(_)) => StatusCode::NOT_FOUND,
            AppError::Lsp(LspError::UnsupportedLanguage(_)) => StatusCode::BAD_REQUEST,
            AppError::Lsp(LspError::NotInitialized(_)) => StatusCode::CONFLICT,
            AppError::Lsp(LspError::RequestTimeout(_))
            | AppError::Lsp(LspError::DownloadFailed(_))
            | AppError::Lsp(LspError::LaunchFailed(_))
            | AppError::Lsp(LspError::RequestFailed(_)) => StatusCode::BAD_GATEWAY,
            AppError::Lsp(LspError::Io(_)) | AppError::Lsp(LspError::Json(_)) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }

            AppError::Json(_) => StatusCode::BAD_REQUEST,
            AppError::Http(e) => e
                .status()
                .and_then(|s| StatusCode::from_u16(s.as_u16()).ok())
                .unwrap_or(StatusCode::BAD_GATEWAY),
            AppError::Io(_)
            | AppError::Other(_)
            | AppError::Worktree(_)
            | AppError::Upgrade(_)
            | AppError::Clipboard(_)
            | AppError::Tui(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        if status.is_server_error() {
            tracing::error!(error = ?self, http_status = status.as_u16(), "request failed");
        } else {
            tracing::warn!(error = ?self, http_status = status.as_u16(), "request rejected");
        }

        let body = serde_json::json!({
            "error": status
                .canonical_reason()
                .unwrap_or("Request failed")
                .to_string(),
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

impl McpError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            McpError::Connection(_)
                | McpError::Server(_)
                | McpError::ToolCall(_)
                | McpError::OAuth(_)
                | McpError::Timeout(_)
        )
    }
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

    #[error("request timeout: {0}")]
    RequestTimeout(String),

    #[error("unsupported language: {0}")]
    UnsupportedLanguage(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl LspError {
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            LspError::DownloadFailed(_)
                | LspError::LaunchFailed(_)
                | LspError::RequestFailed(_)
                | LspError::RequestTimeout(_)
                | LspError::Io(_)
        )
    }
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
        if status.is_server_error() {
            tracing::error!(error = ?self, http_status = status.as_u16(), "server runtime error");
        } else {
            tracing::warn!(error = ?self, http_status = status.as_u16(), "server runtime rejected");
        }
        let body = serde_json::json!({
            "error": status
                .canonical_reason()
                .unwrap_or("Request failed")
                .to_string(),
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

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use serde_json::Value;
    use tokio::runtime::Builder;

    fn assert_app_status(error: AppError, expected: StatusCode) {
        let response = error.into_response();
        assert_eq!(response.status(), expected);
    }

    fn response_json(response: Response<Body>) -> Value {
        let runtime = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime should build");
        runtime.block_on(async move {
            let body = to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("body should be readable");
            serde_json::from_slice(&body).expect("body should be valid json")
        })
    }

    #[test]
    fn app_error_maps_not_found_to_404() {
        assert_app_status(
            AppError::Storage(StorageError::NotFound("session".into())),
            StatusCode::NOT_FOUND,
        );
    }

    #[test]
    fn app_error_maps_invalid_input_to_400() {
        assert_app_status(
            AppError::Config(ConfigError::Invalid("bad config".into())),
            StatusCode::BAD_REQUEST,
        );
    }

    #[test]
    fn app_error_maps_auth_to_401() {
        assert_app_status(
            AppError::Provider(ProviderError::Auth("bad token".into())),
            StatusCode::UNAUTHORIZED,
        );
    }

    #[test]
    fn app_error_maps_permission_to_403() {
        assert_app_status(
            AppError::Permission(PermissionError::Denied {
                tool: "bash".into(),
                path: "/tmp/a".into(),
            }),
            StatusCode::FORBIDDEN,
        );
    }

    #[test]
    fn app_error_maps_rate_limit_to_429() {
        assert_app_status(
            AppError::Provider(ProviderError::RateLimit),
            StatusCode::TOO_MANY_REQUESTS,
        );
    }

    #[test]
    fn app_error_maps_timeout_to_504() {
        assert_app_status(
            AppError::Provider(ProviderError::Timeout("deadline exceeded".into())),
            StatusCode::GATEWAY_TIMEOUT,
        );
    }

    #[test]
    fn app_error_maps_internal_to_500() {
        assert_app_status(
            AppError::Other(anyhow::anyhow!("unexpected internal failure")),
            StatusCode::INTERNAL_SERVER_ERROR,
        );
    }

    #[test]
    fn app_error_body_uses_canonical_reason_without_leaking_details() {
        let response =
            AppError::Provider(ProviderError::Auth("super-secret-token".into())).into_response();
        let body = response_json(response);
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("Unauthorized")
        );
        assert_eq!(body.get("code").and_then(Value::as_u64), Some(401));
        assert!(!body.to_string().contains("super-secret-token"));
    }

    #[test]
    fn server_runtime_error_maps_auth_to_401() {
        let response = ServerRuntimeError::Auth("bad token".into()).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn server_runtime_error_maps_bind_to_500() {
        let response = ServerRuntimeError::Bind("port in use".into()).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn server_runtime_error_body_does_not_expose_debug_details() {
        let response = ServerRuntimeError::Rpc("db://prod.internal".into()).into_response();
        let body = response_json(response);
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("Internal Server Error")
        );
        assert_eq!(body.get("code").and_then(Value::as_u64), Some(500));
        assert!(!body.to_string().contains("db://prod.internal"));
    }
}
