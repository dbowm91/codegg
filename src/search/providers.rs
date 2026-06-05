//! Key-based search providers (Exa, Tavily, Brave, Kagi, SerpAPI).
//!
//! Each provider is enabled only when its corresponding API key is
//! present in the environment. The env-var names match the
//! `ProviderConfig::api_key` convention in
//! `src/config/schema.rs::ProviderConfig::api_key()`:
//!
//! - `EXA_API_KEY`     → [`ExaProvider`]
//! - `TAVILY_API_KEY`  → [`TavilyProvider`]
//! - `BRAVE_API_KEY`   → [`BraveProvider`]
//! - `KAGI_API_KEY`    → [`KagiProvider`]
//! - `SERPAPI_API_KEY` → [`SerpApiProvider`]
//!
//! All requests are SSRF-validated via
//! `crate::security::ssrf::validate_host_ip` + `revalidate_dns` so
//! that an attacker who controls config cannot redirect the search
//! tool to internal hosts.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

use crate::security::ssrf::{revalidate_dns, validate_host_ip};

use super::types::{SearchError, SearchHit, SearchProvider, Specificity};

fn build_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent("codegg-websearch/1.0 (+https://github.com/anomalyco/codegg)")
        .build()
        .unwrap_or_default()
}

async fn validate(url: &str) -> Result<(), SearchError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| SearchError::Transport(format!("invalid url {url}: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| SearchError::Transport(format!("url has no host: {url}")))?;
    let port = parsed.port_or_known_default().unwrap_or(443);
    let ips = validate_host_ip(host, port)
        .map_err(|e| SearchError::Transport(format!("SSRF protection: {e}")))?;
    revalidate_dns(host, port, &ips)
        .map_err(|e| SearchError::Transport(format!("SSRF protection: {e}")))?;
    Ok(())
}

// ---------------------------------------------------------------------
// Exa
// ---------------------------------------------------------------------

pub struct ExaProvider {
    client: Client,
    api_key: Option<String>,
}

impl ExaProvider {
    pub fn from_env() -> Self {
        Self {
            client: build_client(),
            api_key: std::env::var("EXA_API_KEY").ok(),
        }
    }
}

#[async_trait]
impl SearchProvider for ExaProvider {
    fn name(&self) -> &'static str {
        "exa"
    }
    fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }
    fn specificity(&self) -> Specificity {
        Specificity::General
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| SearchError::NotConfigured("EXA_API_KEY".into()))?;
        let url = "https://api.exa.ai/search";
        validate(url).await?;
        let body = serde_json::json!({
            "query": query,
            "numResults": num_results,
            "type": "auto",
            "livecrawl": "fallback",
        });
        let resp = self
            .client
            .post(url)
            .header("x-api-key", api_key)
            .json(&body)
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
            results: Vec<RR>,
        }
        #[derive(Deserialize)]
        struct RR {
            title: Option<String>,
            url: String,
            text: Option<String>,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        Ok(r.results
            .into_iter()
            .map(|x| SearchHit {
                title: x.title.unwrap_or_else(|| "Untitled".into()),
                url: x.url,
                snippet: x.text.unwrap_or_default(),
                source: "exa".into(),
            })
            .collect())
    }
}

// ---------------------------------------------------------------------
// Tavily
// ---------------------------------------------------------------------

pub struct TavilyProvider {
    client: Client,
    api_key: Option<String>,
}

impl TavilyProvider {
    pub fn from_env() -> Self {
        Self {
            client: build_client(),
            api_key: std::env::var("TAVILY_API_KEY").ok(),
        }
    }
}

#[async_trait]
impl SearchProvider for TavilyProvider {
    fn name(&self) -> &'static str {
        "tavily"
    }
    fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }
    fn specificity(&self) -> Specificity {
        Specificity::General
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| SearchError::NotConfigured("TAVILY_API_KEY".into()))?;
        let url = "https://api.tavily.com/search";
        validate(url).await?;
        let body = serde_json::json!({
            "query": query,
            "max_results": num_results,
            "search_depth": "basic",
        });
        let resp = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
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
        #[derive(Deserialize)]
        struct R {
            results: Vec<RR>,
        }
        #[derive(Deserialize)]
        struct RR {
            title: String,
            url: String,
            content: String,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        Ok(r.results
            .into_iter()
            .map(|x| SearchHit {
                title: x.title,
                url: x.url,
                snippet: x.content,
                source: "tavily".into(),
            })
            .collect())
    }
}

// ---------------------------------------------------------------------
// Brave
// ---------------------------------------------------------------------

pub struct BraveProvider {
    client: Client,
    api_key: Option<String>,
}

impl BraveProvider {
    pub fn from_env() -> Self {
        Self {
            client: build_client(),
            api_key: std::env::var("BRAVE_API_KEY").ok(),
        }
    }
}

#[async_trait]
impl SearchProvider for BraveProvider {
    fn name(&self) -> &'static str {
        "brave"
    }
    fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }
    fn specificity(&self) -> Specificity {
        Specificity::General
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| SearchError::NotConfigured("BRAVE_API_KEY".into()))?;
        let url = "https://api.search.brave.com/res/v1/web/search";
        validate(url).await?;
        let resp = self
            .client
            .get(url)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", api_key)
            .query(&[("q", query), ("count", &num_results.to_string())])
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
        #[derive(Deserialize)]
        struct R {
            web: Web,
        }
        #[derive(Deserialize)]
        struct Web {
            results: Vec<RR>,
        }
        #[derive(Deserialize)]
        struct RR {
            title: String,
            url: String,
            description: String,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        Ok(r.web
            .results
            .into_iter()
            .map(|x| SearchHit {
                title: x.title,
                url: x.url,
                snippet: x.description,
                source: "brave".into(),
            })
            .collect())
    }
}

// ---------------------------------------------------------------------
// SerpAPI
// ---------------------------------------------------------------------

pub struct SerpApiProvider {
    client: Client,
    api_key: Option<String>,
}

impl SerpApiProvider {
    pub fn from_env() -> Self {
        Self {
            client: build_client(),
            api_key: std::env::var("SERPAPI_API_KEY").ok(),
        }
    }
}

#[async_trait]
impl SearchProvider for SerpApiProvider {
    fn name(&self) -> &'static str {
        "serpapi"
    }
    fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }
    fn specificity(&self) -> Specificity {
        Specificity::General
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| SearchError::NotConfigured("SERPAPI_API_KEY".into()))?;
        let url = "https://serpapi.com/search";
        validate(url).await?;
        let resp = self
            .client
            .get(url)
            .query(&[
                ("q", query),
                ("api_key", api_key),
                ("engine", "google"),
                ("num", &num_results.to_string()),
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
            organic_results: Option<Vec<RR>>,
        }
        #[derive(Deserialize)]
        struct RR {
            title: String,
            link: String,
            snippet: String,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        Ok(r.organic_results
            .unwrap_or_default()
            .into_iter()
            .map(|x| SearchHit {
                title: x.title,
                url: x.link,
                snippet: x.snippet,
                source: "serpapi".into(),
            })
            .collect())
    }
}

// ---------------------------------------------------------------------
// Kagi
// ---------------------------------------------------------------------

pub struct KagiProvider {
    client: Client,
    api_key: Option<String>,
}

impl KagiProvider {
    pub fn from_env() -> Self {
        Self {
            client: build_client(),
            api_key: std::env::var("KAGI_API_KEY").ok(),
        }
    }
}

#[async_trait]
impl SearchProvider for KagiProvider {
    fn name(&self) -> &'static str {
        "kagi"
    }
    fn is_configured(&self) -> bool {
        self.api_key.is_some()
    }
    fn specificity(&self) -> Specificity {
        Specificity::General
    }
    async fn search(&self, query: &str, num_results: usize) -> Result<Vec<SearchHit>, SearchError> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| SearchError::NotConfigured("KAGI_API_KEY".into()))?;
        let url = "https://kagi.com/api/search";
        validate(url).await?;
        let resp = self
            .client
            .get(url)
            .header("Authorization", format!("Bot {api_key}"))
            .query(&[("q", query), ("limit", &num_results.to_string())])
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
            data: Vec<RR>,
        }
        #[derive(Deserialize)]
        struct RR {
            title: String,
            url: String,
            snippet: String,
        }
        let r: R = resp.json().await.map_err(|e| SearchError::Parse(e.to_string()))?;
        Ok(r.data
            .into_iter()
            .take(num_results)
            .map(|x| SearchHit {
                title: x.title,
                url: x.url,
                snippet: x.snippet,
                source: "kagi".into(),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unconfigured_providers_report_not_configured() {
        // Ensure env doesn't have these set
        std::env::remove_var("EXA_API_KEY");
        std::env::remove_var("TAVILY_API_KEY");
        std::env::remove_var("BRAVE_API_KEY");
        std::env::remove_var("SERPAPI_API_KEY");
        std::env::remove_var("KAGI_API_KEY");
        assert!(!ExaProvider::from_env().is_configured());
        assert!(!TavilyProvider::from_env().is_configured());
        assert!(!BraveProvider::from_env().is_configured());
        assert!(!SerpApiProvider::from_env().is_configured());
        assert!(!KagiProvider::from_env().is_configured());
    }
}
