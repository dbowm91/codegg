use thiserror::Error;

#[derive(Error, Debug)]
pub enum ResearchError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("SQL error: {0}")]
    Sql(#[from] sqlx::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Research run not found: {0}")]
    RunNotFound(String),

    #[error("Research run failed: {0}")]
    RunFailed(String),

    #[error("Source collection failed: {0}")]
    SourceCollection(String),

    #[error("Evidence extraction failed: {0}")]
    EvidenceExtraction(String),

    #[error("Claim construction failed: {0}")]
    ClaimConstruction(String),

    #[error("Verification failed: {0}")]
    VerificationFailed(String),

    #[error("Network not allowed: enable with --allow-network")]
    NetworkNotAllowed,

    #[error("URL fetch failed: {0}")]
    UrlFetch(String),

    #[error("File too large: {path} ({size} bytes, max {max})")]
    FileTooLarge {
        path: String,
        size: usize,
        max: usize,
    },

    #[error("Invalid source spec: {0}")]
    InvalidSourceSpec(String),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Provider error: {0}")]
    Provider(String),
}

pub type Result<T> = std::result::Result<T, ResearchError>;
