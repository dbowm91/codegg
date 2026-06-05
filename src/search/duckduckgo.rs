//! DuckDuckGo HTML search (no API key).
//!
//! Scrapes the public `html.duckduckgo.com/html/` endpoint. The
//! response is rendered HTML; we extract result blocks using a small
//! regex-based parser. Rate-limited by the upstream; we apply a
//! conservative per-instance limit.

use async_trait::async_trait;
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::Client;
use std::time::Duration;
use url::Url;

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

const ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const MAX_RESULTS_CAP: usize = 30;

/// Matches a single result block. Two forms appear in the wild:
/// `<div class="result ...">...</div>` and the more modern
/// `<article class="result ...">...</article>`. Both end with
/// `</div>` / `</article>`.
static RESULT_BLOCK: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<(?:div|article)[^>]*class="[^"]*\bresult\b[^"]*"[^>]*>(.*?)</(?:div|article)>"#)
        .expect("RESULT_BLOCK regex")
});

/// Captures the title link's href inside a result block.
static TITLE_HREF: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<a[^>]*class="[^"]*\bresult__a\b[^"]*"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#)
        .expect("TITLE_HREF regex")
});

/// Captures the snippet text inside a result block.
static SNIPPET: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?s)<a[^>]*class="[^"]*\bresult__snippet\b[^"]*"[^>]*>(.*?)</a>"#)
        .expect("SNIPPET regex")
});

static TAG_STRIP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<[^>]+>").expect("TAG_STRIP regex")
});

static WHITESPACE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("WHITESPACE regex"));

pub struct DuckDuckGoProvider {
    client: Client,
}

impl DuckDuckGoProvider {
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

impl Default for DuckDuckGoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SearchProvider for DuckDuckGoProvider {
    fn name(&self) -> &'static str {
        "duckduckgo"
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
            .post(ENDPOINT)
            .header("Accept", "text/html")
            .form(&[("q", query), ("kl", "us-en")])
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
        parse_duckduckgo_html(&html, limit)
    }
}

/// Parse DuckDuckGo's `html.duckduckgo.com/html/` HTML response.
///
/// Returns [`SearchError::Empty`] when no result blocks are found so
/// the registry can fall through to the next provider.
pub fn parse_duckduckgo_html(html: &str, limit: usize) -> Result<Vec<SearchHit>, SearchError> {
    let mut out = Vec::new();
    for cap in RESULT_BLOCK.captures_iter(html) {
        let block = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let Some(href_cap) = TITLE_HREF.captures(block) else {
            continue;
        };
        let href_raw = href_cap.get(1).map(|m| m.as_str()).unwrap_or("").to_string();
        let url = resolve_uddg(&href_raw).unwrap_or(href_raw);
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
            source: "duckduckgo".into(),
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

/// Resolve DuckDuckGo's `//duckduckgo.com/l/?uddg=<encoded>` redirect
/// to the canonical URL. Returns `None` if the URL can't be parsed.
pub fn resolve_uddg(href: &str) -> Option<String> {
    if href.is_empty() {
        return None;
    }
    if !(href.contains("duckduckgo.com/l/") || href.contains("duckduckgo.com/l?")) {
        return Some(href.to_string());
    }
    let normalized = if href.starts_with("//") {
        format!("https:{href}")
    } else if href.starts_with('/') {
        format!("https://duckduckgo.com{href}")
    } else {
        href.to_string()
    };
    let parsed = Url::parse(&normalized).ok()?;
    if parsed.host_str() == Some("duckduckgo.com") && (parsed.path() == "/l/" || parsed.path() == "/l")
    {
        if let Some((_, target)) = parsed.query_pairs().find(|(k, _)| k == "uddg") {
            return Some(percent_decode(&target));
        }
    }
    Some(normalized)
}

fn percent_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("00"),
                16,
            ) {
                out.push(b as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
<html><body>
<div class="result">
  <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Farticle&amp;rut=abc">
    Example Article Title
  </a>
  <a class="result__snippet">A short snippet about the example.</a>
</div>
<div class="result">
  <a class="result__a" href="https://other.example/foo">Other</a>
  <a class="result__snippet">Another snippet.</a>
</div>
</body></html>
"#;

    #[test]
    fn parses_two_results_from_sample() {
        let hits = parse_duckduckgo_html(SAMPLE, 10).expect("should parse");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://example.com/article");
        assert!(hits[0].title.contains("Example Article"));
        assert!(hits[0].snippet.contains("short snippet"));
        assert_eq!(hits[1].url, "https://other.example/foo");
        assert_eq!(hits[0].source, "duckduckgo");
    }

    #[test]
    fn empty_html_returns_empty_error() {
        let err = parse_duckduckgo_html("<html><body>no results</body></html>", 10).unwrap_err();
        assert!(matches!(err, SearchError::Empty));
    }

    #[test]
    fn respects_limit() {
        let hits = parse_duckduckgo_html(SAMPLE, 1).expect("should parse");
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn uddg_resolver_handles_already_canonical_url() {
        let out = resolve_uddg("https://other.example/foo").unwrap();
        assert_eq!(out, "https://other.example/foo");
    }

    #[test]
    fn uddg_resolver_decodes_redirector() {
        let out = resolve_uddg("//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Farticle")
            .unwrap();
        assert_eq!(out, "https://example.com/article");
    }
}
