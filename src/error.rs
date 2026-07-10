pub use codegg_core::error::*;

#[cfg(feature = "server")]
use axum::{
    body::Body,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};

/// Newtype wrapper for `AppError` that implements axum's `IntoResponse`.
///
/// The root crate owns this wrapper so it can implement the external `IntoResponse`
/// trait, which would violate orphan rules if applied directly to `codegg_core::error::AppError`.
#[cfg(feature = "server")]
pub struct AxumAppError(pub AppError);

#[cfg(feature = "server")]
impl From<AppError> for AxumAppError {
    fn from(e: AppError) -> Self {
        AxumAppError(e)
    }
}

/// Blanket `From` impls for common error types so the `?` operator works
/// directly in axum handlers without requiring two-step conversion through `AppError`.
#[cfg(feature = "server")]
impl From<StorageError> for AxumAppError {
    fn from(e: StorageError) -> Self {
        AxumAppError(AppError::Storage(e))
    }
}

#[cfg(feature = "server")]
impl From<std::io::Error> for AxumAppError {
    fn from(e: std::io::Error) -> Self {
        AxumAppError(AppError::Io(e))
    }
}

#[cfg(feature = "server")]
impl From<serde_json::Error> for AxumAppError {
    fn from(e: serde_json::Error) -> Self {
        AxumAppError(AppError::Json(e))
    }
}

#[cfg(feature = "server")]
impl From<anyhow::Error> for AxumAppError {
    fn from(e: anyhow::Error) -> Self {
        AxumAppError(AppError::Other(e))
    }
}

#[cfg(feature = "server")]
impl From<reqwest::Error> for AxumAppError {
    fn from(e: reqwest::Error) -> Self {
        AxumAppError(AppError::Http(e))
    }
}

#[cfg(feature = "server")]
impl IntoResponse for AxumAppError {
    fn into_response(self) -> Response<Body> {
        let status = match &self.0 {
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
            | AppError::Lsp(LspError::RequestFailed(_))
            | AppError::Lsp(LspError::UnsupportedSourceAction(_))
            | AppError::Lsp(LspError::CommandOnlySourceAction(_))
            | AppError::Lsp(LspError::NoEditForSourceAction(_))
            | AppError::Lsp(LspError::AmbiguousSourceAction(_, _))
            | AppError::Lsp(LspError::CommandOnlyCodeAction(_))
            | AppError::Lsp(LspError::Unsupported(_)) => StatusCode::BAD_GATEWAY,
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
            | AppError::Tui(_)
            | AppError::RunStore(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };

        if status.is_server_error() {
            tracing::error!(error = ?self.0, http_status = status.as_u16(), "request failed");
        } else {
            tracing::warn!(error = ?self.0, http_status = status.as_u16(), "request rejected");
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

/// Newtype wrapper for `ServerRuntimeError` that implements axum's `IntoResponse`.
#[cfg(feature = "server")]
pub struct AxumServerRuntimeError(pub ServerRuntimeError);

#[cfg(feature = "server")]
impl From<ServerRuntimeError> for AxumServerRuntimeError {
    fn from(e: ServerRuntimeError) -> Self {
        AxumServerRuntimeError(e)
    }
}

#[cfg(feature = "server")]
impl IntoResponse for AxumServerRuntimeError {
    fn into_response(self) -> Response<Body> {
        let status = match &self.0 {
            ServerRuntimeError::Auth(_) => StatusCode::UNAUTHORIZED,
            ServerRuntimeError::Bind(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerRuntimeError::Shutdown(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerRuntimeError::WebSocket(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ServerRuntimeError::Rpc(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        if status.is_server_error() {
            tracing::error!(error = ?self.0, http_status = status.as_u16(), "server runtime error");
        } else {
            tracing::warn!(error = ?self.0, http_status = status.as_u16(), "server runtime rejected");
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

#[cfg(all(test, feature = "server"))]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::response::IntoResponse;
    use serde_json::Value;
    use tokio::runtime::Builder;

    fn assert_app_status(error: AppError, expected: StatusCode) {
        let response = AxumAppError(error).into_response();
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
        let response = AxumAppError(AppError::Provider(ProviderError::Auth(
            "super-secret-token".into(),
        )))
        .into_response();
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
        let response =
            AxumServerRuntimeError(ServerRuntimeError::Auth("bad token".into())).into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn server_runtime_error_maps_bind_to_500() {
        let response =
            AxumServerRuntimeError(ServerRuntimeError::Bind("port in use".into())).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn server_runtime_error_body_does_not_expose_debug_details() {
        let response = AxumServerRuntimeError(ServerRuntimeError::Rpc("db://prod.internal".into()))
            .into_response();
        let body = response_json(response);
        assert_eq!(
            body.get("error").and_then(Value::as_str),
            Some("Internal Server Error")
        );
        assert_eq!(body.get("code").and_then(Value::as_u64), Some(500));
        assert!(!body.to_string().contains("db://prod.internal"));
    }
}
