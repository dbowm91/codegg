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

// ── SharedInitError ──────────────────────────────────────────────────

/// A cloneable error type shared across concurrent initialization waiters.
///
/// Holds a categorized [`SharedInitErrorKind`] and a free-form message.
/// Convertible to/from [`LspError`] for propagation at the boundary.
#[derive(Debug, Clone)]
pub struct SharedInitError {
    pub kind: SharedInitErrorKind,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedInitErrorKind {
    ServerNotFound,
    DownloadFailed,
    LaunchFailed,
    InitializeFailed,
    Timeout,
    Cancelled,
    Protocol,
    Other,
}

impl From<&LspError> for SharedInitError {
    fn from(e: &LspError) -> Self {
        match e {
            LspError::ServerNotFound(msg) => Self {
                kind: SharedInitErrorKind::ServerNotFound,
                message: msg.clone(),
            },
            LspError::DownloadFailed(msg) => Self {
                kind: SharedInitErrorKind::DownloadFailed,
                message: msg.clone(),
            },
            LspError::LaunchFailed(msg) => Self {
                kind: SharedInitErrorKind::LaunchFailed,
                message: msg.clone(),
            },
            LspError::NotInitialized(msg) | LspError::RequestFailed(msg) => Self {
                kind: SharedInitErrorKind::InitializeFailed,
                message: msg.clone(),
            },
            LspError::RequestTimeout(msg) => Self {
                kind: SharedInitErrorKind::Timeout,
                message: msg.clone(),
            },
            LspError::InitializationCancelled(msg) => Self {
                kind: SharedInitErrorKind::Cancelled,
                message: msg.clone(),
            },
            LspError::Protocol(msg) => Self {
                kind: SharedInitErrorKind::Protocol,
                message: msg.clone(),
            },
            _ => Self {
                kind: SharedInitErrorKind::Other,
                message: format!("{e}"),
            },
        }
    }
}

impl SharedInitError {
    pub fn into_lsp_error(self) -> LspError {
        match self.kind {
            SharedInitErrorKind::ServerNotFound => LspError::ServerNotFound(self.message),
            SharedInitErrorKind::DownloadFailed => LspError::DownloadFailed(self.message),
            SharedInitErrorKind::LaunchFailed => LspError::LaunchFailed(self.message),
            SharedInitErrorKind::InitializeFailed => LspError::NotInitialized(self.message),
            SharedInitErrorKind::Timeout => LspError::RequestTimeout(self.message),
            SharedInitErrorKind::Cancelled => LspError::InitializationCancelled(self.message),
            SharedInitErrorKind::Protocol => LspError::Protocol(self.message),
            SharedInitErrorKind::Other => LspError::InitializationCancelled(self.message),
        }
    }
}

impl std::fmt::Display for SharedInitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for SharedInitError {}

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

    #[test]
    fn shared_init_error_round_trip() {
        let original = LspError::LaunchFailed("boom".into());
        let shared: SharedInitError = (&original).into();
        assert_eq!(shared.kind, SharedInitErrorKind::LaunchFailed);
        let back = shared.into_lsp_error();
        match back {
            LspError::LaunchFailed(msg) => assert_eq!(msg, "boom"),
            other => panic!("expected LaunchFailed, got {:?}", other),
        }
    }

    #[test]
    fn shared_init_error_display() {
        let err = SharedInitError {
            kind: SharedInitErrorKind::Timeout,
            message: "timed out".into(),
        };
        assert_eq!(format!("{err}"), "Timeout: timed out");
    }

    #[test]
    fn shared_init_error_is_clone() {
        let err = SharedInitError {
            kind: SharedInitErrorKind::Cancelled,
            message: "cancelled".into(),
        };
        let err2 = err.clone();
        assert_eq!(err.kind, err2.kind);
        assert_eq!(err.message, err2.message);
    }

    #[test]
    fn shared_init_error_from_all_lsp_variants() {
        let cases: Vec<(LspError, SharedInitErrorKind)> = vec![
            (
                LspError::ServerNotFound("x".into()),
                SharedInitErrorKind::ServerNotFound,
            ),
            (
                LspError::DownloadFailed("x".into()),
                SharedInitErrorKind::DownloadFailed,
            ),
            (
                LspError::LaunchFailed("x".into()),
                SharedInitErrorKind::LaunchFailed,
            ),
            (
                LspError::NotInitialized("x".into()),
                SharedInitErrorKind::InitializeFailed,
            ),
            (
                LspError::RequestFailed("x".into()),
                SharedInitErrorKind::InitializeFailed,
            ),
            (
                LspError::RequestTimeout("x".into()),
                SharedInitErrorKind::Timeout,
            ),
            (
                LspError::InitializationCancelled("x".into()),
                SharedInitErrorKind::Cancelled,
            ),
            (
                LspError::Protocol("x".into()),
                SharedInitErrorKind::Protocol,
            ),
            (
                LspError::UnsupportedLanguage("x".into()),
                SharedInitErrorKind::Other,
            ),
        ];
        for (lsp_err, expected_kind) in cases {
            let shared: SharedInitError = (&lsp_err).into();
            assert_eq!(shared.kind, expected_kind, "for {:?}", lsp_err);
        }
    }
}
