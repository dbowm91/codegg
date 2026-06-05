//! OpenAlex search (no API key).
//!
//! Searches scholarly works via `api.openalex.org/works?search=...`.
//! Polite-pool users append `mailto=`; we add a fixed mailto so
//! queries go to the polite pool and rate limits are generous.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "https://api.openalex.org/works";
const MAILTO: &str = "codegg-research@example.invalid";

pub struct OpenAlexProvider {
    client: Client,
}

impl OpenAlexProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("codegg-websearch/1.0 (mailto:codegg-research@example.invalid)")
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for OpenAlexProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for OpenAlexProvider {
    fn name(&self) -> &'static str {
        "openalex"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::Domain(super::types::Domain::Academic)
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, 25);
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[("search", query), ("per_page", &limit.to_string()), ("mailto", MAILTO)])
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::Http {
                status: status.as_u16(),
                body,
            });
        }
        #[derive(Deserialize)]
        struct R {
            results: Vec<W>,
        }
        #[derive(Deserialize)]
        struct W {
            #[serde(default)]
            title: Option<String>,
            #[serde(default)]
            doi: Option<String>,
            #[serde(default)]
            publication_year: Option<i32>,
            #[serde(default)]
            abstract_inverted_index: Option<serde_json::Value>,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        if r.results.is_empty() {
            return Err(SearchError::Empty);
        }
        Ok(r.results
            .into_iter()
            .filter_map(|w| {
                let title = w.title?;
                let url = w
                    .doi
                    .as_ref()
                    .map(|d| format!("https://doi.org/{d}"))
                    .unwrap_or_else(|| {
                        format!(
                            "https://api.openalex.org/works?search={}",
                            urlencoding(query)
                        )
                    });
                let snippet = match (&w.publication_year, &w.abstract_inverted_index) {
                    (Some(y), Some(_)) => format!("Published {y}."),
                    (Some(y), None) => format!("Published {y}."),
                    _ => String::new(),
                };
                Some(SearchHit {
                    title,
                    url,
                    snippet,
                    source: "openalex".into(),
                })
            })
            .collect())
    }
}

fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
