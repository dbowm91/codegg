use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use chrono::Utc;
use serde::Deserialize;
use std::future::Future;
use std::pin::Pin;

const API_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

#[derive(Debug, Deserialize)]
struct CratesIoResponse {
    #[serde(default)]
    crate_data: Option<CrateData>,
}

#[derive(Debug, Deserialize)]
struct CrateData {
    name: String,
    description: Option<String>,
    repository: Option<String>,
    downloads: Option<u64>,
    updated_at: Option<String>,
    max_version: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
    documentation: Option<String>,
}

pub struct CratesIoSource {
    client: reqwest::Client,
}

impl CratesIoSource {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(API_TIMEOUT)
            .user_agent("codegg-research")
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }

    fn extract_crate_name(question: &str, plan: &ResearchPlan) -> Option<String> {
        // Try to find a crate name in source_classes first
        for cls in &plan.source_classes {
            if cls.contains("crates.io") || cls.contains("crate") {
                return Some(cls.clone());
            }
        }

        // Try to find a crate-like name in the question (e.g., "tokio crate" or "tokio")
        let words: Vec<&str> = question.split_whitespace().collect();
        for (i, word) in words.iter().enumerate() {
            if word.eq_ignore_ascii_case("crate") && i > 0 {
                return Some(words[i - 1].to_string());
            }
        }

        // Find words that look like crate names (lowercase, alphanumeric + underscores)
        const STOP_WORDS: &[&str] = &[
            "the", "and", "for", "what", "which", "this", "that", "are", "is", "was", "were",
            "how", "why", "where", "when", "does", "has", "can", "could", "should", "would", "may",
            "might", "must", "shall", "will", "tell", "about", "best", "use", "with", "from",
            "into", "than", "evaluate", "look", "check", "give", "show", "find", "search",
            "compare", "review", "analyze", "examine", "consider", "think", "consider", "consider",
            "consider", "consider", "consider",
        ];

        for word in &words {
            let w = word.to_lowercase();
            if w.len() >= 3
                && w.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && !STOP_WORDS.contains(&w.as_str())
            {
                return Some(w);
            }
        }

        None
    }

    async fn fetch_crate(&self, name: &str) -> Result<SourceRecord> {
        let url = format!("https://crates.io/api/v1/crates/{}", name);

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

        let resp: CratesIoResponse = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("failed to parse JSON: {e}")))?;

        let data = resp.crate_data.ok_or_else(|| {
            ResearchError::SourceCollection(format!("no crate data returned for '{}'", name))
        })?;

        let mut notes = Vec::new();
        if let Some(ref desc) = data.description {
            notes.push(format!("description: {}", desc));
        }
        if let Some(repo) = &data.repository {
            notes.push(format!("repository: {}", repo));
        }
        if let Some(dl) = data.downloads {
            notes.push(format!("downloads: {}", dl));
        }
        if let Some(ref ver) = data.max_version {
            notes.push(format!("max_version: {}", ver));
        }
        if let Some(ref lic) = data.license {
            notes.push(format!("license: {}", lic));
        }
        if let Some(ref home) = data.homepage {
            notes.push(format!("homepage: {}", home));
        }
        if let Some(ref docs) = data.documentation {
            notes.push(format!("documentation: {}", docs));
        }
        if let Some(ref updated) = data.updated_at {
            notes.push(format!("updated_at: {}", updated));
        }

        let published_at = data
            .updated_at
            .as_ref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: format!("https://crates.io/crates/{}", data.name),
            title: Some(format!(
                "{} - {}",
                data.name,
                data.description.as_deref().unwrap_or("no description")
            )),
            source_type: SourceType::CratesIoMetadata,
            source_quality: SourceQuality::Secondary,
            retrieved_at: Utc::now(),
            published_at,
            content_hash: None,
            locator: SourceLocator::Url {
                url: format!("https://crates.io/crates/{}", data.name),
                heading: None,
            },
            notes,
        })
    }
}

impl Default for CratesIoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchSourceAdapter for CratesIoSource {
    fn name(&self) -> &'static str {
        "crates_io"
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

            // Extract crate name from question or plan
            if let Some(name) = Self::extract_crate_name(&request.question, plan) {
                match self.fetch_crate(&name).await {
                    Ok(source) => sources.push(source),
                    Err(e) => {
                        eprintln!("Warning: crates.io fetch failed for '{}': {}", name, e);
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
        let source = CratesIoSource::new();
        assert_eq!(source.name(), "crates_io");
    }

    #[test]
    fn test_extract_crate_name_from_question() {
        let plan = ResearchPlan {
            scope: String::new(),
            comparison_axes: vec![],
            source_classes: vec![],
            exclusion_criteria: vec![],
            stopping_conditions: vec![],
            expected_outputs: vec![],
        };

        // "tokio crate" pattern
        let name = CratesIoSource::extract_crate_name("Is tokio crate good?", &plan);
        assert_eq!(name.as_deref(), Some("tokio"));

        // Direct crate name
        let name = CratesIoSource::extract_crate_name("Evaluate serde", &plan);
        assert_eq!(name.as_deref(), Some("serde"));
    }

    #[test]
    fn test_extract_crate_name_from_plan() {
        let plan = ResearchPlan {
            scope: String::new(),
            comparison_axes: vec![],
            source_classes: vec!["crates.io/axum".to_string()],
            exclusion_criteria: vec![],
            stopping_conditions: vec![],
            expected_outputs: vec![],
        };
        let name = CratesIoSource::extract_crate_name("some question", &plan);
        assert_eq!(name.as_deref(), Some("crates.io/axum"));
    }
}
