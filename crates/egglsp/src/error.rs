//! Crate-local `LspError`. Codegg converts via `From<egglsp::LspError>`
//! into its own `error::LspError` at the boundary.

use thiserror::Error;

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

    #[error("unsupported edit: {0}")]
    UnsupportedEdit(String),

    #[error("path outside allowed root: {0}")]
    PathOutsideRoot(String),

    #[error("utf16 position error: {0}")]
    Utf16Position(String),

    #[error("overlapping edits")]
    OverlappingEdits,
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
