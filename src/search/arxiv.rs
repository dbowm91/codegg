//! arXiv API search (no API key).
//!
//! Returns Atom XML. We parse the `<entry>` blocks and extract
//! `<title>`, `<id>`, and `<summary>`. arXiv asks for ≤1 request per
//! 3 seconds for sustained traffic; this module does not enforce that
//! (the registry layer is responsible for global rate limits).

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "http://export.arxiv.org/api/query";

static ENTRY: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<entry>(.*?)</entry>").expect("ENTRY"));
static TITLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<title>(.*?)</title>").expect("TITLE"));
static ID: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<id>([^<]+)</id>").expect("ID"));
static SUMMARY: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<summary>(.*?)</summary>").expect("SUMMARY"));

pub struct ArxivProvider {
    client: Client,
}

impl ArxivProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("codegg-websearch/1.0 (research use; arxiv)")
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for ArxivProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for ArxivProvider {
    fn name(&self) -> &'static str {
        "arxiv"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::Domain(super::types::Domain::Academic)
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, 20);
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[
                ("search_query", format!("all:{query}").as_str()),
                ("start", "0"),
                ("max_results", &limit.to_string()),
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
        let xml = resp.text().await?;
        parse_arxiv_xml(&xml, limit)
    }
}

pub fn parse_arxiv_xml(xml: &str, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
    let mut out = Vec::new();
    for cap in ENTRY.captures_iter(xml) {
        let block = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let title = TITLE
            .captures(block)
            .and_then(|c| c.get(1).map(|m| clean(m.as_str())))
            .unwrap_or_default();
        let url = ID
            .captures(block)
            .and_then(|c| c.get(1).map(|m| m.as_str().trim().to_string()))
            .unwrap_or_default();
        let snippet = SUMMARY
            .captures(block)
            .and_then(|c| c.get(1).map(|m| clean(m.as_str())))
            .unwrap_or_default();
        if title.is_empty() || url.is_empty() {
            continue;
        }
        out.push(SearchHit {
            title,
            url,
            snippet,
            source: "arxiv".into(),
        });
        if out.len() >= limit {
            break;
        }
    }
    if out.is_empty() {
        Err(SearchError::Empty)
    } else {
        Ok(out)
    }
}

fn clean(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0"?>
<feed>
  <entry>
    <id>http://arxiv.org/abs/2406.12345v1</id>
    <title>Attention Is All You Need</title>
    <summary>The dominant sequence transduction models are based on complex recurrent or
    convolutional neural networks.</summary>
  </entry>
  <entry>
    <id>http://arxiv.org/abs/2406.99999v1</id>
    <title>Another Paper</title>
    <summary>Another abstract.</summary>
  </entry>
</feed>
"#;

    #[test]
    fn parses_two_entries() {
        let hits = parse_arxiv_xml(SAMPLE, 10).expect("should parse");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "http://arxiv.org/abs/2406.12345v1");
        assert!(hits[0].title.contains("Attention"));
        assert_eq!(hits[0].source, "arxiv");
    }

    #[test]
    fn empty_xml_returns_empty_error() {
        let err = parse_arxiv_xml("<feed></feed>", 10).unwrap_err();
        assert!(matches!(err, SearchError::Empty));
    }
}
