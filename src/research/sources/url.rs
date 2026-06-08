use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;

const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024; // 5MB

pub struct UrlSource {
    client: reqwest::Client,
}

impl UrlSource {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }

    async fn fetch_url(&self, url: &str) -> Result<SourceRecord> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("HTTP {status} for {url}")));
        }

        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let content_length = response.content_length().map(|v| v as usize);
        if let Some(len) = content_length {
            if len > MAX_RESPONSE_BYTES {
                return Err(ResearchError::UrlFetch(format!(
                    "response too large: {len} bytes (max {MAX_RESPONSE_BYTES})"
                )));
            }
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("failed to read body: {e}")))?;

        let truncated = if bytes.len() > MAX_RESPONSE_BYTES {
            &bytes[..MAX_RESPONSE_BYTES]
        } else {
            &bytes
        };

        let content_hash = format!("{:x}", Sha256::digest(truncated));

        let (text, source_type) = if content_type.contains("text/html") {
            let html_bytes: &[u8] = truncated;
            let text = html2text::from_read(html_bytes, 80)
                .unwrap_or_else(|_| String::from_utf8_lossy(truncated).to_string());
            (text, SourceType::HtmlPage)
        } else if content_type.contains("text/markdown") || content_type.contains("text/x-markdown")
        {
            let text = String::from_utf8_lossy(truncated).to_string();
            (text, SourceType::MarkdownPage)
        } else {
            let text = String::from_utf8_lossy(truncated).to_string();
            (text, SourceType::Url)
        };

        let title = text
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| l.trim().chars().take(120).collect::<String>());

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: url.to_string(),
            title,
            source_type,
            source_quality: SourceQuality::Secondary,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: Some(content_hash),
            locator: SourceLocator::Url {
                url: url.to_string(),
                heading: None,
            },
            notes: vec![format!(
                "fetched {} bytes, content-type: {}",
                bytes.len(),
                content_type
            )],
        })
    }
}

impl Default for UrlSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchSourceAdapter for UrlSource {
    fn name(&self) -> &'static str {
        "url"
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

            let mut sources = Vec::new();

            for source_spec in &request.sources {
                if source_spec.spec_type == SourceSpecType::Url {
                    match self.fetch_url(&source_spec.value).await {
                        Ok(source) => sources.push(source),
                        Err(e) => {
                            eprintln!("Warning: failed to fetch {}: {}", source_spec.value, e);
                        }
                    }
                }
            }

            Ok(sources)
        })
    }
}
