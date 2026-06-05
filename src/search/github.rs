//! GitHub repository search (no API key).
//!
//! Unauthenticated GitHub search is rate-limited to 60 req/hour per
//! IP. We use this as an opt-in provider for queries with
//! "github", "repo", "repository", "crate on" hints. Reserved for
//! explicit "github:" hints to avoid burning the budget on noisy
//! queries.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "https://api.github.com/search/repositories";

pub struct GitHubProvider {
    client: Client,
}

impl GitHubProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent("codegg-websearch/1.0")
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for GitHubProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for GitHubProvider {
    fn name(&self) -> &'static str {
        "github"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::Domain(super::types::Domain::Code)
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, 15);
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[("q", query), ("per_page", &limit.to_string())])
            .header("Accept", "application/vnd.github+json")
            .send()
            .await?;
        let status = resp.status();
        if status.as_u16() == 403 {
            // 403 here is usually rate-limited; treat as RateLimited.
            return Err(SearchError::RateLimited);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::Http {
                status: status.as_u16(),
                body,
            });
        }
        #[derive(Deserialize)]
        struct R {
            items: Vec<Repo>,
        }
        #[derive(Deserialize)]
        struct Repo {
            full_name: String,
            html_url: String,
            description: Option<String>,
            stargazers_count: Option<u64>,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        if r.items.is_empty() {
            return Err(SearchError::Empty);
        }
        Ok(r.items
            .into_iter()
            .map(|it| {
                let snippet = match (it.description, it.stargazers_count) {
                    (Some(d), Some(s)) => format!("{d} (★ {s})"),
                    (Some(d), None) => d,
                    (None, Some(s)) => format!("★ {s}"),
                    (None, None) => String::new(),
                };
                SearchHit {
                    title: it.full_name,
                    url: it.html_url,
                    snippet,
                    source: "github".into(),
                }
            })
            .collect())
    }
}
