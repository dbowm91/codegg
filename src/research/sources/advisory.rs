use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use chrono::Utc;
use std::future::Future;
use std::pin::Pin;

pub struct AdvisorySource {
    client: reqwest::Client,
}

impl AdvisorySource {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("failed to build HTTP client");
        Self { client }
    }

    async fn fetch_crate_versions(&self, crate_name: &str) -> Result<SourceRecord> {
        let url = format!("https://crates.io/api/v1/crates/{crate_name}/versions");

        let response = self
            .client
            .get(&url)
            .header("User-Agent", "codegg-research/0.1")
            .send()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("request failed: {e}")))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ResearchError::UrlFetch(format!(
                "HTTP {status} for {url}"
            )));
        }

        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ResearchError::UrlFetch(format!("failed to parse JSON: {e}")))?;

        let versions = body["versions"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        let mut notes = Vec::new();
        let mut has_yanked = false;
        let mut yanked_versions = Vec::new();

        for version in &versions {
            if let Some(is_yanked) = version["yanked"].as_bool() {
                if is_yanked {
                    has_yanked = true;
                    if let Some(num) = version["num"].as_str() {
                        yanked_versions.push(num.to_string());
                    }
                }
            }
        }

        if has_yanked {
            notes.push(format!(
                "Yanked versions detected: {}",
                yanked_versions.join(", ")
            ));
        } else {
            notes.push("No yanked versions found in recent releases".to_string());
        }

        if let Some(Some(newest)) = versions.first().map(|v| v["num"].as_str()) {
            notes.push(format!("Latest version: {newest}"));
        }

        let advisory_url = format!("https://crates.io/crates/{crate_name}");

        Ok(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: advisory_url.clone(),
            title: Some(format!("Advisory metadata for {crate_name}")),
            source_type: SourceType::Url,
            source_quality: SourceQuality::StandardOrSpec,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: None,
            locator: SourceLocator::Url {
                url: advisory_url,
                heading: None,
            },
            notes,
        })
    }
}

impl Default for AdvisorySource {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchSourceAdapter for AdvisorySource {
    fn name(&self) -> &'static str {
        "advisory"
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

            let mut crate_names = Vec::new();

            for source_spec in &request.sources {
                if source_spec.spec_type == SourceSpecType::Url {
                    let val = &source_spec.value;
                    if let Some(name) = val
                        .strip_prefix("crate:")
                        .or_else(|| val.strip_prefix("crates.io/crates/"))
                    {
                        crate_names.push(name.to_string());
                    }
                }
            }

            for class in &plan.source_classes {
                if class.contains("SecurityReview") || class.contains("LibraryEvaluation") {
                    for source_spec in &request.sources {
                        if !source_spec.value.is_empty()
                            && !source_spec.value.starts_with("http://")
                            && !source_spec.value.starts_with("https://")
                            && !source_spec.value.starts_with('/')
                        {
                            crate_names.push(source_spec.value.clone());
                        }
                    }
                }
            }

            crate_names.sort();
            crate_names.dedup();

            let mut sources = Vec::new();
            for name in &crate_names {
                match self.fetch_crate_versions(name).await {
                    Ok(source) => sources.push(source),
                    Err(e) => {
                        eprintln!(
                            "Warning: advisory source failed for crate '{}': {}",
                            name, e
                        );
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
    fn name_returns_correct_value() {
        let source = AdvisorySource::new();
        assert_eq!(source.name(), "advisory");
    }

    #[test]
    fn default_creates_client() {
        let source = AdvisorySource::default();
        assert_eq!(source.name(), "advisory");
    }
}
