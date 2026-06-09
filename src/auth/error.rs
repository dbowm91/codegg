//! Auth errors.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("credential not found for provider '{0}'")]
    NotFound(String),

    #[error("credential expired for provider '{0}'")]
    Expired(String),

    #[error("no master key configured; set CODEGG_MASTER_KEY to store new credentials")]
    MasterKeyMissing,

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("auth mode '{0}' is recognized but not yet implemented in this build")]
    Unsupported(String),

    #[error("invalid auth configuration: {0}")]
    Invalid(String),

    #[error("external command '{command}' failed: {message}")]
    ExternalCommand { command: String, message: String },
}

impl From<crate::crypto::CryptoError> for AuthError {
    fn from(value: crate::crypto::CryptoError) -> Self {
        AuthError::Crypto(value.to_string())
    }
}
