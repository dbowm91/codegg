use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use chrono::Utc;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchProvider {
    Tavily,
    Brave,
    SerpApi,
    Kagi,
}

impl SearchProvider {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tavily" => Some(SearchProvider::Tavily),
            "brave" => Some(SearchProvider::Brave),
            "serpapi" | "serp_api" => Some(SearchProvider::SerpApi),
            "kagi" => Some(SearchProvider::Kagi),
            _ => None,
        }
    }
}

pub struct SearchProviderSource {
    client: reqwest::Client,
    provider: SearchProvider,
    api_key: Option<String>,
}

impl SearchProviderSource {
    pub fn new(provider: SearchProvider, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .expect("failed to build HTTP client");
        Self {
            client,
            provider,
            api_key,
        }
    }

    pub fn provider(&self) -> &SearchProvider {
        &self.provider
    }

    async fn search_tavily(&self, query: &str) -> Result<Vec<SourceRecord>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ResearchError::Config("Tavily API key not configured".to_string()))?;

        let body = serde_json::json!({
            "query": query,
            "max_results": 5,
            "search_depth": "basic",
        });

        let response = self
            .client
            .post("https://api.tavily.com/search")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {api_key}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("Tavily request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("Tavily HTTP {status}")));
        }

        #[derive(Deserialize)]
        struct TavilyResponse {
            results: Vec<TavilyResult>,
        }

        #[derive(Deserialize)]
        struct TavilyResult {
            title: String,
            url: String,
            content: String,
        }

        let data: TavilyResponse = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("Tavily parse error: {e}")))?;

        Ok(data
            .results
            .into_iter()
            .map(|r| SourceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: String::new(),
                uri: r.url.clone(),
                title: Some(r.title),
                source_type: SourceType::Url,
                source_quality: SourceQuality::Secondary,
                retrieved_at: Utc::now(),
                published_at: None,
                content_hash: None,
                locator: SourceLocator::Url {
                    url: r.url,
                    heading: None,
                },
                notes: vec![r.content],
            })
            .collect())
    }

    async fn search_brave(&self, query: &str) -> Result<Vec<SourceRecord>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ResearchError::Config("Brave API key not configured".to_string()))?;

        let response = self
            .client
            .get("https://api.search.brave.com/res/v1/web/search")
            .header("Accept", "application/json")
            .header("X-Subscription-Token", api_key)
            .query(&[("q", query), ("count", "5")])
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("Brave request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("Brave HTTP {status}")));
        }

        #[derive(Deserialize)]
        struct BraveResponse {
            web: BraveWeb,
        }

        #[derive(Deserialize)]
        struct BraveWeb {
            results: Vec<BraveResult>,
        }

        #[derive(Deserialize)]
        struct BraveResult {
            title: String,
            url: String,
            description: String,
        }

        let data: BraveResponse = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("Brave parse error: {e}")))?;

        Ok(data
            .web
            .results
            .into_iter()
            .map(|r| SourceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: String::new(),
                uri: r.url.clone(),
                title: Some(r.title),
                source_type: SourceType::Url,
                source_quality: SourceQuality::Secondary,
                retrieved_at: Utc::now(),
                published_at: None,
                content_hash: None,
                locator: SourceLocator::Url {
                    url: r.url,
                    heading: None,
                },
                notes: vec![r.description],
            })
            .collect())
    }

    async fn search_serpapi(&self, query: &str) -> Result<Vec<SourceRecord>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ResearchError::Config("SerpAPI key not configured".to_string()))?;

        let response = self
            .client
            .get("https://serpapi.com/search")
            .query(&[
                ("q", query),
                ("api_key", api_key.as_str()),
                ("engine", "google"),
                ("num", "5"),
            ])
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("SerpAPI request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("SerpAPI HTTP {status}")));
        }

        #[derive(Deserialize)]
        struct SerpApiResponse {
            organic_results: Option<Vec<SerpApiResult>>,
        }

        #[derive(Deserialize)]
        struct SerpApiResult {
            title: String,
            link: String,
            snippet: String,
        }

        let data: SerpApiResponse = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("SerpAPI parse error: {e}")))?;

        let results = data.organic_results.unwrap_or_default();

        Ok(results
            .into_iter()
            .map(|r| SourceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: String::new(),
                uri: r.link.clone(),
                title: Some(r.title),
                source_type: SourceType::Url,
                source_quality: SourceQuality::Secondary,
                retrieved_at: Utc::now(),
                published_at: None,
                content_hash: None,
                locator: SourceLocator::Url {
                    url: r.link,
                    heading: None,
                },
                notes: vec![r.snippet],
            })
            .collect())
    }

    async fn search_kagi(&self, query: &str) -> Result<Vec<SourceRecord>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| ResearchError::Config("Kagi API key not configured".to_string()))?;

        let response = self
            .client
            .get("https://kagi.com/api/search")
            .header("Authorization", format!("Bot {api_key}"))
            .query(&[("q", query)])
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("Kagi request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("Kagi HTTP {status}")));
        }

        #[derive(Deserialize)]
        struct KagiResponse {
            data: Vec<KagiResult>,
        }

        #[derive(Deserialize)]
        struct KagiResult {
            title: String,
            url: String,
            snippet: String,
        }

        let data: KagiResponse = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("Kagi parse error: {e}")))?;

        Ok(data
            .data
            .into_iter()
            .take(5)
            .map(|r| SourceRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: String::new(),
                uri: r.url.clone(),
                title: Some(r.title),
                source_type: SourceType::Url,
                source_quality: SourceQuality::Secondary,
                retrieved_at: Utc::now(),
                published_at: None,
                content_hash: None,
                locator: SourceLocator::Url {
                    url: r.url,
                    heading: None,
                },
                notes: vec![r.snippet],
            })
            .collect())
    }

    async fn search(&self, query: &str) -> Result<Vec<SourceRecord>> {
        match self.provider {
            SearchProvider::Tavily => self.search_tavily(query).await,
            SearchProvider::Brave => self.search_brave(query).await,
            SearchProvider::SerpApi => self.search_serpapi(query).await,
            SearchProvider::Kagi => self.search_kagi(query).await,
        }
    }
}

impl ResearchSourceAdapter for SearchProviderSource {
    fn name(&self) -> &'static str {
        "search_provider"
    }

    fn collect<'a>(
        &'a self,
        request: &'a ResearchRequest,
        _plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceRecord>>> + Send + 'a>> {
        Box::pin(async move {
            if !request.budget.allow_network {
                return Err(ResearchError::NetworkNotAllowed);
            }

            if self.api_key.is_none() {
                eprintln!(
                    "Warning: {:?} search provider API key not configured, skipping",
                    self.provider
                );
                return Ok(Vec::new());
            }

            let max_results = request.budget.max_sources.min(10);

            match self.search(&request.question).await {
                Ok(mut sources) => {
                    sources.truncate(max_results);
                    Ok(sources)
                }
                Err(e) => {
                    eprintln!("Warning: {:?} search failed: {}", self.provider, e);
                    Ok(Vec::new())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_correct_value() {
        let source = SearchProviderSource::new(SearchProvider::Tavily, None);
        assert_eq!(source.name(), "search_provider");
    }

    #[test]
    fn provider_from_str_tavily() {
        assert_eq!(
            SearchProvider::from_str("tavily"),
            Some(SearchProvider::Tavily)
        );
    }

    #[test]
    fn provider_from_str_brave() {
        assert_eq!(
            SearchProvider::from_str("brave"),
            Some(SearchProvider::Brave)
        );
    }

    #[test]
    fn provider_from_str_serpapi() {
        assert_eq!(
            SearchProvider::from_str("serpapi"),
            Some(SearchProvider::SerpApi)
        );
        assert_eq!(
            SearchProvider::from_str("serp_api"),
            Some(SearchProvider::SerpApi)
        );
    }

    #[test]
    fn provider_from_str_kagi() {
        assert_eq!(SearchProvider::from_str("kagi"), Some(SearchProvider::Kagi));
    }

    #[test]
    fn provider_from_str_unknown() {
        assert_eq!(SearchProvider::from_str("unknown"), None);
    }

    #[test]
    fn missing_api_key_does_not_panic() {
        let source = SearchProviderSource::new(SearchProvider::Tavily, None);
        assert!(source.api_key.is_none());
    }
}
