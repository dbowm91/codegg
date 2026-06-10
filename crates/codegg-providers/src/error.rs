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
