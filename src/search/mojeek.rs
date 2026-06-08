//! Mojeek HTML search (no API key).
//!
//! Last-resort fallback that uses Mojeek's own crawler-based index.
//! Returns a different corpus from DuckDuckGo, useful for
//! corroboration.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use std::time::Duration;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "https://www.mojeek.com/search";
const MAX_RESULTS_CAP: usize = 30;

static RESULT_BLOCK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<li[^>]*class="[^"]*\bresults-standard\b[^"]*"[^>]*>(.*?)</li>"#)
        .expect("RESULT_BLOCK regex")
});

static TITLE_HREF: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<a[^>]*class="[^"]*\bresult-title\b[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
        .expect("TITLE_HREF regex")
});

static SNIPPET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<p[^>]*class="[^"]*\bs\b[^"]*"[^>]*>(.*?)</p>"#).expect("SNIPPET regex")
});

static TAG_STRIP: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").expect("TAG_STRIP regex"));

static WHITESPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("WHITESPACE regex"));

pub struct MojeekProvider {
    client: Client,
}

impl MojeekProvider {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent(
                    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) \
                     Chrome/124.0.0.0 Safari/537.36",
                )
                .build()
                .unwrap_or_default(),
        }
    }
}

impl Default for MojeekProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for MojeekProvider {
    fn name(&self) -> &'static str {
        "mojeek"
    }
    fn is_configured(&self) -> bool {
        true
    }
    fn specificity(&self) -> Specificity {
        Specificity::General
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let limit = num_results.clamp(1, MAX_RESULTS_CAP);
        let resp = self
            .client
            .get(ENDPOINT)
            .query(&[("q", query)])
            .header("Accept", "text/html")
            .send()
            .await?;
        let status = resp.status();
        if status.as_u16() == 429 {
            return Err(SearchError::RateLimited);
        }
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(SearchError::Http {
                status: status.as_u16(),
                body,
            });
        }
        let html = resp.text().await?;
        parse_mojeek_html(&html, limit)
    }
}

pub fn parse_mojeek_html(html: &str, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
    let mut out = Vec::new();
    for cap in RESULT_BLOCK.captures_iter(html) {
        let block = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let Some(href_cap) = TITLE_HREF.captures(block) else {
            continue;
        };
        let url = href_cap
            .get(1)
            .map(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        if url.is_empty() {
            continue;
        }
        let title = href_cap
            .get(2)
            .map(|m| clean(m.as_str()))
            .unwrap_or_default();
        let snippet = SNIPPET
            .captures(block)
            .and_then(|c| c.get(1).map(|m| clean(m.as_str())))
            .unwrap_or_default();
        out.push(SearchHit {
            title,
            url,
            snippet,
            source: "mojeek".into(),
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
    let stripped = TAG_STRIP.replace_all(s, " ");
    let collapsed = WHITESPACE.replace_all(&stripped, " ");
    collapsed.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
<html><body>
<ul>
<li class="results-standard">
  <h2><a class="result-title" href="https://example.com/a">Example A</a></h2>
  <a class="link">https://example.com/a</a>
  <p class="s">A short description about A.</p>
</li>
<li class="results-standard">
  <h2><a class="result-title" href="https://example.com/b">Example B</a></h2>
  <p class="s">Another description.</p>
</li>
</ul>
</body></html>
"#;

    #[test]
    fn parses_two_results_from_sample() {
        let hits = parse_mojeek_html(SAMPLE, 10).expect("should parse");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://example.com/a");
        assert_eq!(hits[0].title, "Example A");
        assert!(hits[0].snippet.contains("short description"));
        assert_eq!(hits[1].url, "https://example.com/b");
        assert_eq!(hits[0].source, "mojeek");
    }

    #[test]
    fn empty_html_returns_empty_error() {
        let err = parse_mojeek_html("<html><body>no results</body></html>", 10).unwrap_err();
        assert!(matches!(err, SearchError::Empty));
    }
}
