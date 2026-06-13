use thiserror::Error;

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

impl From<codegg_config::ConfigError> for ConfigError {
    fn from(e: codegg_config::ConfigError) -> Self {
        match e {
            codegg_config::ConfigError::NotFound(s) => ConfigError::NotFound(s),
            codegg_config::ConfigError::Invalid(s) => ConfigError::Invalid(s),
            codegg_config::ConfigError::Parse(s) => ConfigError::Parse(s),
            codegg_config::ConfigError::Merge(s) => ConfigError::Merge(s),
            codegg_config::ConfigError::Watch(s) => ConfigError::Watch(s),
        }
    }
}

pub use codegg_providers::error::{ProviderError, StorageError};

impl From<codegg_config::ConfigError> for AppError {
    fn from(e: codegg_config::ConfigError) -> Self {
        AppError::Config(e.into())
    }
}

impl From<codegg_config::AppError> for AppError {
    fn from(e: codegg_config::AppError) -> Self {
        match e {
            codegg_config::AppError::Config(c) => AppError::Config(c.into()),
            codegg_config::AppError::Io(e) => AppError::Io(e),
            codegg_config::AppError::Other(e) => AppError::Other(e),
        }
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

impl From<eggsentry::EggsecError> for ToolError {
    fn from(err: eggsentry::EggsecError) -> Self {
        match err {
            eggsentry::EggsecError::Io(msg) => ToolError::Io(msg),
            eggsentry::EggsecError::FileTooLarge(size, max) => {
                ToolError::Execution(format!("file too large: {} bytes (max {})", size, max))
            }
            eggsentry::EggsecError::Join(msg) => ToolError::Execution(msg),
        }
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

impl From<egglsp::LspError> for LspError {
    fn from(e: egglsp::LspError) -> Self {
        match e {
            egglsp::LspError::ServerNotFound(s) => LspError::ServerNotFound(s),
            egglsp::LspError::DownloadFailed(s) => LspError::DownloadFailed(s),
            egglsp::LspError::LaunchFailed(s) => LspError::LaunchFailed(s),
            egglsp::LspError::NotInitialized(s) => LspError::NotInitialized(s),
            egglsp::LspError::RequestFailed(s) => LspError::RequestFailed(s),
            egglsp::LspError::RequestTimeout(s) => LspError::RequestTimeout(s),
            egglsp::LspError::UnsupportedLanguage(s) => LspError::UnsupportedLanguage(s),
            egglsp::LspError::Io(e) => LspError::Io(e),
            egglsp::LspError::Json(e) => LspError::Json(e),
            egglsp::LspError::UnsupportedEdit(s) => {
                LspError::RequestFailed(format!("unsupported edit: {}", s))
            }
            egglsp::LspError::PathOutsideRoot(s) => {
                LspError::RequestFailed(format!("path outside root: {}", s))
            }
            egglsp::LspError::Utf16Position(s) => {
                LspError::RequestFailed(format!("utf16 position: {}", s))
            }
            egglsp::LspError::OverlappingEdits => {
                LspError::RequestFailed("overlapping edits".to_string())
            }
            egglsp::LspError::UnsupportedSourceAction(s) => LspError::UnsupportedSourceAction(s),
            egglsp::LspError::CommandOnlySourceAction(s) => LspError::CommandOnlySourceAction(s),
            egglsp::LspError::NoEditForSourceAction(s) => LspError::NoEditForSourceAction(s),
            egglsp::LspError::AmbiguousSourceAction(kind, titles) => {
                LspError::AmbiguousSourceAction(kind, titles)
            }
            egglsp::LspError::Protocol(s) => {
                LspError::RequestFailed(format!("protocol error: {}", s))
            }
            egglsp::LspError::WriterClosed(s) => {
                LspError::RequestFailed(format!("writer closed: {}", s))
            }
            egglsp::LspError::InitializationCancelled(s) => {
                LspError::RequestFailed(format!("initialization cancelled: {}", s))
            }
        }
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

    #[error("unsupported source action: {0}")]
    UnsupportedSourceAction(String),

    #[error("source action returned only command actions: {0}")]
    CommandOnlySourceAction(String),

    #[error("source action returned no edit-bearing actions: {0}")]
    NoEditForSourceAction(String),

    #[error("source action returned multiple edit-bearing actions: {0}: {1}")]
    AmbiguousSourceAction(String, String),
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
    LoadFailed(String),

    #[error("plugin hook failed: {0}")]
    HookFailed(String),

    #[error("plugin install failed: {0}")]
    InstallFailed(String),

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
