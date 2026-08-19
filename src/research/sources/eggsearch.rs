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

/// The sole external search source adapter used by deep research.
#[derive(Debug, Default, Clone, Copy)]
pub struct EggsearchSource;

type ResultItem<'a> = (&'a Map<String, Value>, Option<&'a Map<String, Value>>);

impl EggsearchSource {
    pub fn new() -> Self {
        Self
    }

    fn upstream_workflow(mode: &ResearchMode) -> &'static str {
        match mode {
            ResearchMode::Landscape => "ecosystem_survey",
            ResearchMode::ArchitectureDecision => "architecture_decision",
            ResearchMode::LibraryEvaluation => "library_comparison",
            ResearchMode::ApiInvestigation => "api_evaluation",
            ResearchMode::DebuggingInvestigation => "general",
            ResearchMode::SecurityReview => "security_review",
            ResearchMode::SpecDigest => "general",
            ResearchMode::NarrowAnswer => "general",
        }
    }

    fn request_input(request: &ResearchRequest) -> Value {
        let security = request.mode == ResearchMode::SecurityReview;
        let depth = match request.depth {
            ResearchDepth::Low => "quick",
            ResearchDepth::Medium => "standard",
            ResearchDepth::High => "deep",
        };

        json!({
            "query": request.question.trim(),
            "workflow": Self::upstream_workflow(&request.mode),
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

    fn result_items(payload: &Value) -> Vec<ResultItem<'_>> {
        if let Some(object) = payload.as_object() {
            if let Some(groups) = object.get("groups").and_then(Value::as_array) {
                let mut grouped = Vec::new();
                for group in groups {
                    let Some(group_object) = group.as_object() else {
                        continue;
                    };
                    let Some(results) = group_object.get("results").and_then(Value::as_array)
                    else {
                        continue;
                    };
                    for result in results {
                        if let Some(result_object) = result.as_object() {
                            grouped.push((result_object, Some(group_object)));
                        }
                    }
                }
                return grouped;
            }
            for key in [
                "sources",
                "papers",
                "results",
                "hits",
                "items",
                "vulns",
                "advisories",
                "vulnerabilities",
            ] {
                if let Some(items) = object.get(key).and_then(Value::as_array) {
                    return items
                        .iter()
                        .filter_map(Value::as_object)
                        .map(|item| (item, None))
                        .collect();
                }
            }
            return vec![(object, None)];
        }
        payload
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_object)
                    .map(|item| (item, None))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn string_field<'a>(object: &'a Map<String, Value>, fields: &[&str]) -> Option<&'a str> {
        fields
            .iter()
            .find_map(|field| object.get(*field).and_then(Value::as_str))
    }

    fn providers(item: &Map<String, Value>, group: Option<&Map<String, Value>>) -> Vec<String> {
        let mut providers = Vec::new();
        if let Some(values) = item.get("providers").and_then(Value::as_array) {
            for provider in values.iter().filter_map(Value::as_str) {
                let provider = provider.trim();
                if !provider.is_empty() && !providers.iter().any(|value| value == provider) {
                    providers.push(provider.to_string());
                }
            }
        }

        if providers.is_empty() {
            for object in [Some(item), group].into_iter().flatten() {
                if let Some(provider) = Self::string_field(object, &["provider", "source"]) {
                    let provider = provider.trim();
                    if !provider.is_empty() && !providers.iter().any(|value| value == provider) {
                        providers.push(provider.to_string());
                    }
                }
            }
        }

        providers
    }

    fn source_kind(item: &Map<String, Value>, group: Option<&Map<String, Value>>) -> String {
        item.get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| Self::string_field(metadata, &["source_kind"]))
            .or_else(|| Self::string_field(item, &["source_kind", "source_type", "type", "kind"]))
            .or_else(|| {
                group.and_then(|group| {
                    Self::string_field(
                        group,
                        &["kind", "classification", "source_kind", "source_type"],
                    )
                })
            })
            .map(str::trim)
            .filter(|kind| !kind.is_empty())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default()
    }

    fn source_from_result(
        item: &Map<String, Value>,
        group: Option<&Map<String, Value>>,
    ) -> Option<SourceRecord> {
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
        let providers = Self::providers(item, group);
        let kind = Self::source_kind(item, group);

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
        if let Some(stable_id) = ["stable_id", "source_id", "id"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
        {
            notes.push(format!("stable_id={stable_id}"));
        }
        for provider in providers {
            notes.push(format!("provider={provider}"));
        }
        if !kind.is_empty() {
            notes.push(format!("source_kind={kind}"));
        }
        if let Some(group) = group {
            if let Some(label) = group.get("label").and_then(Value::as_str) {
                notes.push(format!("group_label={label}"));
            }
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

    fn convert_value(payload: &Value) -> Vec<SourceRecord> {
        Self::result_items(payload)
            .into_iter()
            .filter_map(|(item, group)| Self::source_from_result(item, group))
            .collect()
    }

    fn convert_output(output: &str) -> Result<Vec<SourceRecord>> {
        let payload: Value =
            serde_json::from_str(Self::payload_from_framed(output)).map_err(|e| {
                ResearchError::SourceCollection(format!(
                    "eggsearch returned an unparseable research result: {e}"
                ))
            })?;
        Ok(Self::convert_value(&payload))
    }

    fn convert_structured(
        result: search_backend::StructuredSearchResult,
    ) -> Result<Vec<SourceRecord>> {
        if let Some(value) = result.value {
            return Ok(Self::convert_value(&value));
        }
        if result.truncated {
            return Err(ResearchError::SourceCollection(
                "eggsearch returned truncated display evidence without a structured result"
                    .to_string(),
            ));
        }
        Self::convert_output(&result.output)
    }

    async fn collect_external(&self, request: &ResearchRequest) -> Result<Vec<SourceRecord>> {
        let input = Self::request_input(request);
        let result = if request.mode == ResearchMode::SecurityReview {
            search_backend::dispatch_security_search_structured(&input).await
        } else {
            search_backend::dispatch_research_search_structured(&input).await
        }
        .map_err(|error| ResearchError::SourceCollection(error.to_string()))?;
        Self::convert_structured(result)
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
    fn every_research_mode_maps_to_a_supported_eggsearch_workflow() {
        let cases = [
            (ResearchMode::Landscape, "ecosystem_survey"),
            (ResearchMode::ArchitectureDecision, "architecture_decision"),
            (ResearchMode::LibraryEvaluation, "library_comparison"),
            (ResearchMode::ApiInvestigation, "api_evaluation"),
            (ResearchMode::DebuggingInvestigation, "general"),
            (ResearchMode::SecurityReview, "security_review"),
            (ResearchMode::SpecDigest, "general"),
            (ResearchMode::NarrowAnswer, "general"),
        ];
        for (mode, workflow) in cases {
            let value = EggsearchSource::request_input(&request(mode));
            assert_eq!(value["workflow"], workflow);
        }
    }

    #[test]
    fn security_review_uses_security_workflow_without_provider_selection() {
        let value = EggsearchSource::request_input(&request(ResearchMode::SecurityReview));
        assert_eq!(value["workflow"], "security_review");
        assert_eq!(value["include_security_considerations"], true);
        assert!(!value.as_object().unwrap().contains_key("providers"));
        assert_eq!(value["max_results"], 15);
    }

    #[test]
    fn converts_grouped_source_cards_in_stable_order_and_preserves_provenance() {
        let value = json!({
            "groups": [
                {
                    "classification": "academic",
                    "label": "Primary papers",
                    "results": [
                        {
                            "id": "src_paper_1",
                            "stable_id": "paper-1",
                            "url": "https://example.com/paper-1",
                            "title": "Paper 1",
                            "snippet": "Summary 1",
                            "providers": ["arxiv", "openalex", "arxiv"],
                            "score": 0.81,
                            "trust": "external_untrusted",
                            "fetched": false,
                            "trust_markers": {},
                            "metadata": {"source_kind": "reference"},
                            "future_field": {"ignored": true}
                        },
                        {"url": "file:///not-external", "title": "Rejected"}
                    ]
                },
                {
                    "classification": "official_docs",
                    "label": "Documentation",
                    "results": [
                        {
                            "id": "src_docs_1",
                            "stable_id": "docs-1",
                            "url": "https://example.com/docs",
                            "title": "Docs",
                            "snippet": "Reference",
                            "providers": ["official", "mojeek"],
                            "score": 0.75,
                            "trust": "external_untrusted",
                            "fetched": false,
                            "trust_markers": {},
                            "metadata": {"source_kind": "official_docs"}
                        },
                        {
                            "id": "paper-2",
                            "url": "https://example.com/paper-2",
                            "title": "Paper 2",
                            "source_type": "academic",
                            "provider": "legacy"
                        }
                    ]
                }
            ],
            "unknown_future_field": {"preserve": true}
        });
        let sources = EggsearchSource::convert_value(&value);
        assert_eq!(sources.len(), 3);
        assert_eq!(sources[0].uri, "https://example.com/paper-1");
        assert_eq!(sources[1].uri, "https://example.com/docs");
        assert_eq!(sources[2].uri, "https://example.com/paper-2");
        assert_eq!(sources[0].source_quality, SourceQuality::Secondary);
        assert!(sources[0]
            .notes
            .iter()
            .any(|note| note == "stable_id=paper-1"));
        assert!(sources[1]
            .notes
            .iter()
            .any(|note| note == "provider=official"));
        assert!(sources[1]
            .notes
            .iter()
            .any(|note| note == "provider=mojeek"));
        assert!(!sources[1]
            .notes
            .iter()
            .any(|note| note == "provider=legacy"));
        assert_eq!(sources[1].source_quality, SourceQuality::OfficialDocs);
        assert!(sources[1]
            .notes
            .iter()
            .any(|note| note == "group_label=Documentation"));
        assert!(sources[0]
            .notes
            .iter()
            .any(|note| note == "trust=external_untrusted"));
    }

    #[test]
    fn structured_value_wins_over_truncated_display_projection() {
        let value = json!({
            "groups": [{
                "results": [{
                    "stable_id": "structured-1",
                    "url": "https://example.com/structured",
                    "title": "Complete result"
                }]
            }]
        });
        let result = search_backend::StructuredSearchResult {
            output: "[external_research_evidence]\n{\"groups\":[".to_string(),
            value: Some(value),
            truncated: true,
        };
        let sources = EggsearchSource::convert_structured(result).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].uri, "https://example.com/structured");
    }

    #[test]
    fn truncated_text_only_projection_fails_explicitly() {
        let result = search_backend::StructuredSearchResult {
            output: "[external_research_evidence]\n{\"results\":[".to_string(),
            value: None,
            truncated: true,
        };
        let error = EggsearchSource::convert_structured(result).unwrap_err();
        assert!(error.to_string().contains("truncated display evidence"));
    }
}
