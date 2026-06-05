use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("transport: {0}")]
    Transport(String),
    #[error("http status {status}: {body}")]
    Http { status: u16, body: String },
    #[error("parse: {0}")]
    Parse(String),
    #[error("rate limited")]
    RateLimited,
    #[error("not configured: {0}")]
    NotConfigured(String),
    #[error("empty result set")]
    Empty,
}

impl From<reqwest::Error> for SearchError {
    fn from(e: reqwest::Error) -> Self {
        SearchError::Transport(e.to_string())
    }
}

impl From<SearchError> for crate::error::ToolError {
    fn from(e: SearchError) -> Self {
        crate::error::ToolError::Execution(format!("search: {e}"))
    }
}

/// A single search result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub snippet: String,
    /// The name of the provider that produced this hit (e.g. "duckduckgo").
    pub source: String,
}

/// Coarse classification of a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Specificity {
    /// Returns general web results (DDG, Mojeek, Exa, Tavily, Brave, …).
    General,
    /// Returns results from a specific domain (Wikipedia, arXiv, …).
    Domain(Domain),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    Encyclopedic,
    Academic,
    Biomedical,
    News,
    TechDiscourse,
    Code,
}

/// A pluggable web search backend.
#[async_trait]
pub trait SearchProvider: Send + Sync {
    /// Short identifier used in [`SearchHit::source`].
    fn name(&self) -> &'static str;
    /// Whether this provider has the credentials (or, for no-key
    /// providers, the configuration) it needs to operate.
    fn is_configured(&self) -> bool;
    /// Coarse classification. Used by the registry to decide when to
    /// call a domain-specific provider.
    fn specificity(&self) -> Specificity;
    /// Run a search and return up to `num_results` hits.
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError>;
}
