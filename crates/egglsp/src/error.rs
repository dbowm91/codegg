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

    #[error("unsupported source action: '{0}'; supported actions: source.organizeImports")]
    UnsupportedSourceAction(String),

    #[error("source action '{0}' returned only command actions; command execution is disabled")]
    CommandOnlySourceAction(String),

    #[error("source action '{0}' returned no edit-bearing actions")]
    NoEditForSourceAction(String),

    #[error("source action '{0}' returned multiple edit-bearing actions: {1}")]
    AmbiguousSourceAction(String, String),

    #[error("protocol error: {0}")]
    Protocol(String),

    #[error("writer closed: {0}")]
    WriterClosed(String),

    #[error("initialization cancelled: {0}")]
    InitializationCancelled(String),
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
        // Protocol, WriterClosed, InitializationCancelled are not retryable.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_error_variants_are_not_retryable() {
        assert!(!LspError::Protocol("test".into()).is_retryable());
        assert!(!LspError::WriterClosed("test".into()).is_retryable());
        assert!(!LspError::InitializationCancelled("test".into()).is_retryable());
    }

    #[test]
    fn existing_retryable_variants_still_retryable() {
        assert!(LspError::DownloadFailed("test".into()).is_retryable());
        assert!(LspError::LaunchFailed("test".into()).is_retryable());
        assert!(LspError::RequestFailed("test".into()).is_retryable());
        assert!(LspError::RequestTimeout("test".into()).is_retryable());
        assert!(LspError::Io(std::io::Error::other("test")).is_retryable());
    }
}
