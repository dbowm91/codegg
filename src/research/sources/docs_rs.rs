use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::future::Future;
use std::pin::Pin;

const API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const MAX_RESPONSE_BYTES: usize = 5 * 1024 * 1024; // 5MB

pub struct DocsRsSource {
    client: reqwest::Client,
}

impl DocsRsSource {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(API_TIMEOUT)
            .user_agent("codegg-research")
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }

    fn extract_crate_info(question: &str, plan: &ResearchPlan) -> Option<(String, Option<String>)> {
        // Try to find crate name in source_classes
        for cls in &plan.source_classes {
            if cls.contains("docs.rs") {
                let path = cls.strip_prefix("docs.rs/").unwrap_or(cls);
                let parts: Vec<&str> = path.splitn(2, '/').collect();
                let crate_name = parts[0].to_string();
                let item_path = if parts.len() > 1 {
                    Some(parts[1].to_string())
                } else {
                    None
                };
                return Some((crate_name, item_path));
            }
        }

        let words: Vec<&str> = question.split_whitespace().collect();
        let mut crate_name = None;
        let mut item_path = None;

        const STOP_WORDS: &[&str] = &[
            "the", "and", "for", "what", "which", "this", "that", "are", "is", "was", "were",
            "how", "why", "where", "when", "does", "has", "can", "could", "should", "would", "may",
            "might", "must", "shall", "will", "tell", "about", "best", "use", "with", "from",
            "into", "than", "evaluate", "look", "check", "give", "show", "find", "search",
            "compare", "review", "analyze", "examine", "consider", "think", "module",
        ];

        for word in &words {
            // Handle "crate" keyword: word before it is the crate name
            if word.eq_ignore_ascii_case("crate") {
                if let Some(prev) = words.iter().position(|w| w == word) {
                    if prev > 0 {
                        crate_name = Some(words[prev - 1].to_string());
                    }
                }
            }

            // Handle module::item pattern (e.g., "tokio::sync")
            if word.contains("::") {
                let cleaned = word.trim_start_matches("::").trim_end_matches("::");
                let parts: Vec<&str> = cleaned.splitn(2, "::").collect();
                if crate_name.is_none() {
                    crate_name = Some(parts[0].to_string());
                }
                if parts.len() > 1 {
                    item_path = Some(parts[1..].join("::"));
                }
            }
        }

        // Fallback: find first non-stop word as crate name
        if crate_name.is_none() {
            for word in &words {
                let w = word.to_lowercase().trim_end_matches("::").to_string();
                if w.len() >= 3
                    && w.chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    && !STOP_WORDS.contains(&w.as_str())
                {
                    crate_name = Some(w);
                    break;
                }
            }
        }

        crate_name.map(|n| (n, item_path))
    }

    async fn fetch_docs(&self, crate_name: &str, item_path: Option<&str>) -> Result<SourceRecord> {
        let path = item_path.unwrap_or("");
        let url = if path.is_empty() {
            format!("https://docs.rs/{}/latest", crate_name)
        } else {
            format!("https://docs.rs/{}/latest/{}", crate_name, path)
        };

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "codegg-research")
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!("HTTP {status} for {url}")));
        }

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

        // Convert HTML to text
        let text = html2text::from_read(truncated, 80)
            .unwrap_or_else(|_| String::from_utf8_lossy(truncated).to_string());

        // Extract title from the first meaningful line
        let title = text
            .lines()
            .find(|l| !l.trim().is_empty())
            .map(|l| {
                let trimmed = l.trim().chars().take(120).collect::<String>();
                if trimmed.is_empty() {
                    format!("{} - docs.rs", crate_name)
                } else {
                    trimmed
                }
            })
            .unwrap_or_else(|| format!("{} - docs.rs", crate_name));

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: url.clone(),
            title: Some(title),
            source_type: SourceType::Url,
            source_quality: SourceQuality::OfficialDocs,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: Some(content_hash),
            locator: SourceLocator::Url {
                url,
                heading: item_path.map(|p| p.to_string()),
            },
            notes: vec![
                format!("crate: {}", crate_name),
                format!("bytes: {}", bytes.len()),
            ],
        })
    }
}

impl Default for DocsRsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchSourceAdapter for DocsRsSource {
    fn name(&self) -> &'static str {
        "docs_rs"
    }

    fn collect<'a>(
        &'a self,
        request: &'a ResearchRequest,
        plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceRecord>>> + Send + 'a>> {
        Box::pin(async move {
            if !request.budget.allow_network {
                return Err(ResearchError::NetworkNotAllowed);
            }

            let mut sources = Vec::new();

            // Check request sources for docs.rs URLs
            for source_spec in &request.sources {
                if source_spec.spec_type == SourceSpecType::Url
                    && source_spec.value.contains("docs.rs")
                {
                    let url = &source_spec.value;
                    // Parse from URL: https://docs.rs/{crate_name}/latest/{item_path}
                    if let Some(rest) = url.strip_prefix("https://docs.rs/") {
                        let parts: Vec<&str> = rest.split('/').collect();
                        if parts.len() >= 1 && !parts[0].is_empty() {
                            let crate_name = parts[0].to_string();
                            let item_path = if parts.len() > 2 && parts[1] == "latest" {
                                Some(parts[2..].join("/"))
                            } else {
                                None
                            };
                            match self.fetch_docs(&crate_name, item_path.as_deref()).await {
                                Ok(source) => sources.push(source),
                                Err(e) => {
                                    eprintln!("Warning: docs.rs fetch failed: {}", e);
                                }
                            }
                        }
                    }
                }
            }

            // If no explicit sources, try to extract from question/plan
            if sources.is_empty() {
                if let Some((crate_name, item_path)) =
                    Self::extract_crate_info(&request.question, plan)
                {
                    match self.fetch_docs(&crate_name, item_path.as_deref()).await {
                        Ok(source) => sources.push(source),
                        Err(e) => {
                            eprintln!("Warning: docs.rs fetch failed for '{}': {}", crate_name, e);
                        }
                    }
                }
            }

            Ok(sources)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name() {
        let source = DocsRsSource::new();
        assert_eq!(source.name(), "docs_rs");
    }

    #[test]
    fn test_extract_crate_info_from_plan() {
        let plan = ResearchPlan {
            scope: String::new(),
            comparison_axes: vec![],
            source_classes: vec!["docs.rs/tokio/sync".to_string()],
            exclusion_criteria: vec![],
            stopping_conditions: vec![],
            expected_outputs: vec![],
        };
        let info = DocsRsSource::extract_crate_info("some question", &plan);
        let (name, item) = info.unwrap();
        assert_eq!(name, "tokio");
        assert_eq!(item.as_deref(), Some("sync"));
    }

    #[test]
    fn test_extract_crate_info_from_question() {
        let plan = ResearchPlan {
            scope: String::new(),
            comparison_axes: vec![],
            source_classes: vec![],
            exclusion_criteria: vec![],
            stopping_conditions: vec![],
            expected_outputs: vec![],
        };
        let info = DocsRsSource::extract_crate_info("Tell me about tokio crate", &plan);
        let (name, item) = info.unwrap();
        assert_eq!(name, "tokio");
        assert!(item.is_none());
    }

    #[test]
    fn test_extract_crate_info_with_item() {
        let plan = ResearchPlan {
            scope: String::new(),
            comparison_axes: vec![],
            source_classes: vec![],
            exclusion_criteria: vec![],
            stopping_conditions: vec![],
            expected_outputs: vec![],
        };
        let info = DocsRsSource::extract_crate_info("Look at tokio::sync module", &plan);
        let (name, item) = info.unwrap();
        assert_eq!(name, "tokio");
        assert_eq!(item.as_deref(), Some("sync"));
    }
}
