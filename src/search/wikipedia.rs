//! Wikipedia REST + MediaWiki Action API search (no API key).
//!
//! Uses the public `en.wikipedia.org` Action API for the
//! `list=search` endpoint, which returns clean JSON. This is
//! triggered for entity-shaped queries ("what is X", "who is Y").
//!
//! The Wikipedia API requires a descriptive `User-Agent` per the
//! [Wikimedia User-Agent policy](https://meta.wikimedia.org/wiki/User-Agent_policy).
//! This module sends `codegg-websearch/1.0 (research; contact via codegg)`.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "https://en.wikipedia.org/w/api.php";
const USER_AGENT: &str = "codegg-websearch/1.0 (https://github.com/anomalyco/codegg; research use)";

pub struct WikipediaProvider {
    client: Client,
}

impl WikipediaProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent(USER_AGENT)
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for WikipediaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for WikipediaProvider {
    fn name(&self) -> &'static str {
        "wikipedia"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::Domain(super::types::Domain::Encyclopedic)
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, 20);
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[
                ("action", "query"),
                ("list", "search"),
                ("srsearch", query),
                ("srlimit", &limit.to_string()),
                ("format", "json"),
                ("utf8", "1"),
                ("origin", "*"),
            ])
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
            query: Option<Query>,
        }
        #[derive(Deserialize)]
        struct Query {
            search: Vec<Item>,
        }
        #[derive(Deserialize)]
        struct Item {
            title: String,
            #[serde(default)]
            snippet: String,
        }
        let r: R = resp
            .json()
            .await
            .map_err(|e| SearchError::Parse(e.to_string()))?;
        let items = r.query.map(|q| q.search).unwrap_or_default();
        if items.is_empty() {
            return Err(SearchError::Empty);
        }
        Ok(items
            .into_iter()
            .map(|it| {
                let url_title = it.title.clone();
                SearchHit {
                    title: it.title,
                    url: format!("https://en.wikipedia.org/wiki/{}", urlencoding(&url_title)),
                    snippet: strip_html(&it.snippet),
                    source: "wikipedia".into(),
                }
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

fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    // Collapse whitespace and trim.
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urlencoding_replaces_spaces() {
        assert_eq!(urlencoding("Hello World"), "Hello%20World");
    }

    #[test]
    fn strip_html_removes_tags() {
        let input = "Hello <span class=\"x\">world</span>!";
        let out = strip_html(input);
        assert_eq!(out, "Hello world!");
    }
}
