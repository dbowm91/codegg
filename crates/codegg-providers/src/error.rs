use thiserror::Error;

#[derive(Debug, Error)]
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

impl From<eggfetch_core::Error> for ProviderError {
    fn from(error: eggfetch_core::Error) -> Self {
        if matches!(error, eggfetch_core::Error::Timeout { .. }) {
            return Self::Timeout(error.kind().to_string());
        }

        // Eggfetch errors intentionally do not cross this boundary verbatim:
        // some request URLs contain credentials (for example Google API keys).
        // The stable category is enough for provider retry/error policy and
        // keeps transport diagnostics free of URL and secret data.
        Self::api(
            "request_error",
            format!("HTTP transport error ({})", error.kind()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eggfetch_transport_errors_are_secret_safe_and_classified() {
        let error: ProviderError = eggfetch_core::Error::InvalidUrl(
            "https://example.test/models?key=secret-api-key".to_string(),
        )
        .into();
        assert!(matches!(
            error,
            ProviderError::Api { ref code, ref message, ref url }
                if code == "request_error"
                    && message == "HTTP transport error (invalid_url)"
                    && url.is_empty()
        ));
        assert!(!error.to_string().contains("secret-api-key"));

        let timeout: ProviderError = eggfetch_core::Error::Timeout {
            phase: eggfetch_core::TimeoutPhase::Connect,
            elapsed: std::time::Duration::from_secs(10),
        }
        .into();
        assert!(matches!(timeout, ProviderError::Timeout(ref phase) if phase == "timeout_connect"));
    }
}

impl From<crate::circuit::CircuitError> for ProviderError {
    fn from(e: crate::circuit::CircuitError) -> Self {
        match e {
            crate::circuit::CircuitError::Open(name) => ProviderError::CircuitOpen(name),
        }
    }
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
