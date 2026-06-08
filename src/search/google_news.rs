//! Google News RSS search (no API key).
//!
//! Uses `news.google.com/rss/search?q=...` which is officially
//! provided for consumption. Returns RSS 2.0 XML; we parse `<item>`
//! blocks for title, link, pubDate.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "https://news.google.com/rss/search";

static ITEM: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<item>(.*?)</item>").expect("ITEM"));
static TITLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<title>(.*?)</title>").expect("TITLE"));
static LINK: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?s)<link>([^<]+)</link>").expect("LINK"));
static PUBDATE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?s)<pubDate>([^<]+)</pubDate>").expect("PUBDATE"));

pub struct GoogleNewsProvider {
    client: Client,
}

impl GoogleNewsProvider {
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

impl Default for GoogleNewsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for GoogleNewsProvider {
    fn name(&self) -> &'static str {
        "google_news"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::Domain(super::types::Domain::News)
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, 50);
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[
                ("q", query),
                ("hl", "en-US"),
                ("gl", "US"),
                ("ceid", "US:en"),
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
        parse_google_news_rss(&xml, limit)
    }
}

pub fn parse_google_news_rss(xml: &str, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
    let mut out = Vec::new();
    for cap in ITEM.captures_iter(xml) {
        let block = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let title = TITLE
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| clean(m.as_str()))
            .unwrap_or_default();
        let link = LINK
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let pubdate = PUBDATE
            .captures(block)
            .and_then(|c| c.get(1))
            .map(|m| clean(m.as_str()))
            .unwrap_or_default();
        if title.is_empty() || link.is_empty() {
            continue;
        }
        out.push(SearchHit {
            title,
            url: link,
            snippet: pubdate,
            source: "google_news".into(),
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
<rss><channel>
  <item>
    <title>Headline 1</title>
    <link>https://news.example.com/a</link>
    <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
  </item>
  <item>
    <title>Headline 2</title>
    <link>https://news.example.com/b</link>
    <pubDate>Tue, 02 Jan 2024 00:00:00 GMT</pubDate>
  </item>
</channel></rss>"#;

    #[test]
    fn parses_two_items() {
        let hits = parse_google_news_rss(SAMPLE, 10).expect("should parse");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://news.example.com/a");
        assert!(hits[0].title.contains("Headline"));
        assert_eq!(hits[0].source, "google_news");
    }

    #[test]
    fn empty_xml_returns_empty_error() {
        let err = parse_google_news_rss("<rss></rss>", 10).unwrap_err();
        assert!(matches!(err, SearchError::Empty));
    }
}
