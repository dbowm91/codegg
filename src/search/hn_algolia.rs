//! Hacker News Algolia search (no API key).
//!
//! Free public search at `hn.algolia.com/api/v1/search`. Returns
//! clean JSON. Useful for "what does the community think about X" and
//! finding primary links from HN threads.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "https://hn.algolia.com/api/v1/search";

pub struct HnAlgoliaProvider {
    client: Client,
}

impl HnAlgoliaProvider {
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

impl Default for HnAlgoliaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for HnAlgoliaProvider {
    fn name(&self) -> &'static str {
        "hn_algolia"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::Domain(super::types::Domain::TechDiscourse)
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, 30);
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[("query", query), ("tags", "story"), ("hitsPerPage", &limit.to_string())])
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
            hits: Vec<Hit>,
        }
        #[derive(Deserialize)]
        struct Hit {
            #[serde(default)]
            title: Option<String>,
            #[serde(default)]
            url: Option<String>,
            #[serde(default)]
            story_text: Option<String>,
            #[serde(default, rename = "objectID")]
            object_id: String,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        if r.hits.is_empty() {
            return Err(SearchError::Empty);
        }
        Ok(r.hits
            .into_iter()
            .map(|h| {
                let url = h
                    .url
                    .clone()
                    .unwrap_or_else(|| format!("https://news.ycombinator.com/item?id={}", h.object_id));
                SearchHit {
                    title: h.title.unwrap_or_else(|| format!("HN: {}", h.object_id)),
                    url,
                    snippet: h.story_text.unwrap_or_default(),
                    source: "hn_algolia".into(),
                }
            })
            .collect())
    }
}
