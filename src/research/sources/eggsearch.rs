//! Eggsearch-backed external research source collection.
//!
//! Research owns orchestration and `SourceRecord` conversion. Eggsearch owns
//! external provider selection, credentials, retrieval, and result shaping.
//! This adapter deliberately calls the shared search backend service rather
//! than creating a client or process per research run.

use super::ResearchSourceAdapter;
use crate::research::error::{ResearchError, Result};
use crate::research::types::*;
use crate::search_backend;
use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use std::future::Future;
use std::pin::Pin;

/// Compatibility names retained for callers that used the old research
/// configuration. They are not provider-routing inputs anymore.
#[derive(Debug, Clone, PartialEq, Eq)]
#[deprecated(note = "research provider selection is owned by eggsearch")]
pub enum SearchProvider {
    Tavily,
    Brave,
    SerpApi,
    Kagi,
}

#[allow(clippy::should_implement_trait)]
#[allow(deprecated)]
impl SearchProvider {
    pub fn from_str(value: &str) -> Option<Self> {
        match value.to_lowercase().as_str() {
            "tavily" => Some(Self::Tavily),
            "brave" => Some(Self::Brave),
            "serpapi" | "serp_api" => Some(Self::SerpApi),
            "kagi" => Some(Self::Kagi),
            _ => None,
        }
    }
}

/// The sole external search source adapter used by deep research.
#[derive(Debug, Default, Clone, Copy)]
pub struct EggsearchSource;

impl EggsearchSource {
    pub fn new() -> Self {
        Self
    }

    fn request_input(request: &ResearchRequest) -> Value {
        let security = request.mode == ResearchMode::SecurityReview;
        let workflow = match request.mode {
            ResearchMode::Landscape => "landscape",
            ResearchMode::ArchitectureDecision => "architecture_decision",
            ResearchMode::LibraryEvaluation => "library_evaluation",
            ResearchMode::ApiInvestigation => "api_investigation",
            ResearchMode::DebuggingInvestigation => "debugging",
            ResearchMode::SecurityReview => "security",
            ResearchMode::SpecDigest => "spec_digest",
            ResearchMode::NarrowAnswer => "narrow_answer",
        };
        let depth = match request.depth {
            ResearchDepth::Low => "quick",
            ResearchDepth::Medium => "standard",
            ResearchDepth::High => "deep",
        };

        json!({
            "query": request.question.trim(),
            "workflow": workflow,
            "depth": depth,
            "include_primary_sources": true,
            "include_counterpoints": matches!(
                request.mode,
                ResearchMode::Landscape | ResearchMode::ArchitectureDecision
            ),
            "include_security_considerations": security,
            "max_results": request.budget.max_sources.clamp(1, 15),
        })
    }

    fn payload_from_framed(output: &str) -> &str {
        let body_start = output.find("\n\n").map_or(0, |offset| offset + 2);
        let body_end = output.rfind("\n[/external_").unwrap_or(output.len());
        &output[body_start..body_end]
    }

    fn result_items(payload: &Value) -> Vec<&Map<String, Value>> {
        if let Some(object) = payload.as_object() {
            for key in ["sources", "papers", "results", "hits", "items", "vulns"] {
                if let Some(items) = object.get(key).and_then(Value::as_array) {
                    return items.iter().filter_map(Value::as_object).collect();
                }
            }
            return vec![object];
        }
        payload
            .as_array()
            .map(|items| items.iter().filter_map(Value::as_object).collect())
            .unwrap_or_default()
    }

    fn source_from_result(item: &Map<String, Value>) -> Option<SourceRecord> {
        let uri = ["url", "link", "uri"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))?
            .trim()
            .to_string();
        let parsed = reqwest::Url::parse(&uri).ok()?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return None;
        }

        let title = ["title", "name"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
            .map(str::to_owned);
        let snippet = ["snippet", "abstract", "content", "description"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
            .map(str::to_owned);
        let provider = ["provider", "source"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str));
        let kind = ["source_type", "type", "kind"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
            .unwrap_or_default()
            .to_lowercase();

        let published_at = ["published_at", "publishedAt", "published"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|value| value.with_timezone(&Utc));

        let source_quality = if kind.contains("official") || kind.contains("documentation") {
            SourceQuality::OfficialDocs
        } else if kind.contains("paper") || kind.contains("academic") {
            SourceQuality::Academic
        } else if kind.contains("code") || kind.contains("repository") {
            SourceQuality::SourceCode
        } else {
            SourceQuality::Secondary
        };

        let mut notes = vec![
            "source=eggsearch".to_string(),
            "trust=external_untrusted".to_string(),
        ];
        if let Some(provider) = provider {
            notes.push(format!("provider={provider}"));
        }
        if let Some(snippet) = snippet {
            notes.push(snippet);
        }

        Some(SourceRecord {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: String::new(),
            uri: uri.clone(),
            title,
            source_type: SourceType::Url,
            source_quality,
            retrieved_at: Utc::now(),
            published_at,
            content_hash: None,
            locator: SourceLocator::Url {
                url: uri,
                heading: None,
            },
            notes,
        })
    }

    fn convert_output(output: &str) -> Result<Vec<SourceRecord>> {
        let payload: Value =
            serde_json::from_str(Self::payload_from_framed(output)).map_err(|e| {
                ResearchError::SourceCollection(format!(
                    "eggsearch returned an unparseable research result: {e}"
                ))
            })?;
        Ok(Self::result_items(&payload)
            .into_iter()
            .filter_map(Self::source_from_result)
            .collect())
    }

    async fn collect_external(&self, request: &ResearchRequest) -> Result<Vec<SourceRecord>> {
        let input = Self::request_input(request);
        let output = if request.mode == ResearchMode::SecurityReview {
            search_backend::dispatch_security_search(&input).await
        } else {
            search_backend::dispatch_research_search(&input).await
        }
        .map_err(|error| ResearchError::SourceCollection(error.to_string()))?;
        Self::convert_output(&output)
    }
}

impl ResearchSourceAdapter for EggsearchSource {
    fn name(&self) -> &'static str {
        "eggsearch"
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

            let mut sources = self.collect_external(request).await?;
            sources.truncate(request.budget.max_sources);
            Ok(sources)
        })
    }
}

/// Compatibility wrapper for code that constructed the old source directly.
/// The provider and API key are intentionally ignored: eggsearch owns both.
#[allow(deprecated)]
pub struct SearchProviderSource {
    inner: EggsearchSource,
}

#[allow(deprecated)]
impl SearchProviderSource {
    pub fn new(_provider: SearchProvider, _api_key: Option<String>) -> Self {
        Self {
            inner: EggsearchSource::new(),
        }
    }
}

impl ResearchSourceAdapter for SearchProviderSource {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn collect<'a>(
        &'a self,
        request: &'a ResearchRequest,
        plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SourceRecord>>> + Send + 'a>> {
        self.inner.collect(request, plan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(mode: ResearchMode) -> ResearchRequest {
        ResearchRequest {
            id: "test".to_string(),
            question: "  how does this work?  ".to_string(),
            mode,
            audience: ResearchAudience::AgentPlanner,
            depth: ResearchDepth::Medium,
            output_profiles: vec![],
            constraints: vec![],
            sources: vec![],
            existing_context_refs: vec![],
            budget: ResearchBudget {
                max_sources: 20,
                max_chunks_per_source: 1,
                max_evidence_spans: 1,
                max_model_calls: 1,
                max_output_tokens: None,
                allow_network: true,
            },
            created_at: Utc::now(),
        }
    }

    #[test]
    fn request_uses_eggsearch_workflow_without_provider_selection() {
        let value = EggsearchSource::request_input(&request(ResearchMode::SecurityReview));
        assert_eq!(value["workflow"], "security");
        assert_eq!(value["include_security_considerations"], true);
        assert!(!value.as_object().unwrap().contains_key("providers"));
        assert_eq!(value["max_results"], 15);
    }

    #[test]
    fn converts_current_eggsearch_source_cards_and_preserves_provenance() {
        let output = "[external_research_evidence trust=external_untrusted source=eggsearch tool=research_search]\n\n{\"papers\":[{\"url\":\"https://example.com/paper\",\"title\":\"Paper\",\"abstract\":\"Summary\",\"provider\":\"arxiv\",\"source_type\":\"paper\"}]}\n[/external_research_evidence]";
        let sources = EggsearchSource::convert_output(output).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].title.as_deref(), Some("Paper"));
        assert_eq!(sources[0].source_quality, SourceQuality::Academic);
        assert!(sources[0].notes.iter().any(|note| note == "provider=arxiv"));
        assert!(sources[0]
            .notes
            .iter()
            .any(|note| note == "trust=external_untrusted"));
    }
}
