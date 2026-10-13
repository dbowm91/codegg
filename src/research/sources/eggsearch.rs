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
///
/// Holds an optional explicit [`SearchRuntimeContext`]. When `Some`
/// (the production registry path via `ResearchTool::with_search_runtime`),
/// evidence collection executes against that context and never touches
/// the deprecated process-global slots. When `None` (legacy direct
/// construction), collection falls back to the legacy global wrappers
/// for backward compatibility.
#[derive(Debug, Default, Clone)]
pub struct EggsearchSource {
    runtime: Option<crate::search_backend::SearchRuntimeContext>,
}

type ResultItem<'a> = (&'a Map<String, Value>, Option<&'a Map<String, Value>>);
const MAX_UPSTREAM_LABEL_CHARS: usize = 128;
const MAX_SNIPPET_CHARS: usize = 4_000;

impl EggsearchSource {
    pub fn new() -> Self {
        Self { runtime: None }
    }

    /// Attach an explicit runtime-owned search/MCP context (M005).
    pub fn with_search_runtime(
        mut self,
        runtime: crate::search_backend::SearchRuntimeContext,
    ) -> Self {
        self.runtime = Some(runtime);
        self
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
                    providers.push(provider.chars().take(MAX_UPSTREAM_LABEL_CHARS).collect());
                }
            }
        }

        if providers.is_empty() {
            for object in [Some(item), group].into_iter().flatten() {
                if let Some(provider) = Self::string_field(object, &["provider", "source"]) {
                    let provider = provider.trim();
                    if !provider.is_empty() && !providers.iter().any(|value| value == provider) {
                        providers.push(provider.chars().take(MAX_UPSTREAM_LABEL_CHARS).collect());
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
        let parsed = url::Url::parse(&uri).ok()?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return None;
        }

        let title = ["title", "name"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
            .map(str::to_owned);
        let snippet: Option<String> = ["snippet", "abstract", "content", "description"]
            .iter()
            .find_map(|field| item.get(*field).and_then(Value::as_str))
            .map(|value| value.chars().take(MAX_SNIPPET_CHARS).collect());
        let providers = Self::providers(item, group);
        let kind = Self::source_kind(item, group);

        let published_at = item
            .get("metadata")
            .and_then(Value::as_object)
            .and_then(|metadata| Self::string_field(metadata, &["published_at"]))
            .or_else(|| {
                ["published_at", "publishedAt", "published"]
                    .iter()
                    .find_map(|field| item.get(*field).and_then(Value::as_str))
            })
            .and_then(Self::parse_timestamp);

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
            notes.push(format!("stable_id={}", Self::bounded_label(stable_id)));
        }
        for provider in providers {
            notes.push(format!("provider={provider}"));
        }
        if !kind.is_empty() {
            notes.push(format!("source_kind={kind}"));
        }
        if let Some(group) = group {
            if let Some(label) = group.get("label").and_then(Value::as_str) {
                notes.push(format!("group_label={}", Self::bounded_label(label)));
            }
        }
        if let Some(provenance) = item
            .get("provenance")
            .and_then(Value::as_str)
            .or_else(|| item.get("trust").and_then(Value::as_str))
        {
            notes.push(format!(
                "upstream_provenance={}",
                Self::bounded_label(provenance)
            ));
        }
        if let Some(snippet) = snippet {
            // Tagged with an explicit key so the coordinator can recover the
            // retrieved text for extraction. This is the only content a
            // search adapter returns; storing it bare (as a note with no
            // `key=`) made it indistinguishable from the provenance notes.
            notes.push(format!("snippet={snippet}"));
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

    fn bounded_label(value: &str) -> String {
        value
            .chars()
            .filter(|character| !character.is_control() && *character != '\n' && *character != '\r')
            .take(MAX_UPSTREAM_LABEL_CHARS)
            .collect()
    }

    fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
        DateTime::parse_from_rfc3339(value)
            .ok()
            .map(|value| value.with_timezone(&Utc))
            .or_else(|| {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .ok()
                    .and_then(|date| date.and_hms_opt(0, 0, 0))
                    .map(|date| DateTime::from_naive_utc_and_offset(date, Utc))
            })
    }

    fn receipt_from_value(
        value: &Value,
        success: bool,
        truncated: bool,
        sources: &[SourceRecord],
    ) -> ResearchRetrievalReceipt {
        let state_object = value
            .get("retrieval_state")
            .or_else(|| value.get("retrieval_summary"));
        let state_value = state_object.and_then(|state| {
            state
                .get("status")
                .or_else(|| state.get("state"))
                .and_then(Value::as_str)
        });
        let complete = state_object
            .and_then(|state| state.get("complete"))
            .and_then(Value::as_bool);
        let failed_providers: Vec<String> = value
            .get("providers_failed")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(Self::bounded_label)
            .filter(|item| !item.is_empty())
            .take(32)
            .collect();
        let parsed_state = state_value.map(str::to_ascii_lowercase);
        let state = if truncated {
            RetrievalState::Truncated
        } else if !success || !failed_providers.is_empty() {
            if sources.is_empty() {
                RetrievalState::Failed
            } else {
                RetrievalState::Partial
            }
        } else {
            match (parsed_state.as_deref(), complete) {
                (Some("complete" | "completed" | "success" | "succeeded"), Some(true) | None) => {
                    RetrievalState::Complete
                }
                (Some("partial" | "degraded" | "incomplete"), _) | (_, Some(false)) => {
                    RetrievalState::Partial
                }
                (Some("failed" | "error"), _) => RetrievalState::Failed,
                (Some("skipped" | "policy_skipped"), _) => RetrievalState::Skipped,
                (Some("truncated"), _) => RetrievalState::Truncated,
                _ => RetrievalState::Unknown,
            }
        };
        let mut warning_codes = Vec::new();
        if let Some(warnings) = value.get("structured_warnings").and_then(Value::as_array) {
            for warning in warnings.iter().take(32) {
                let code = warning
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_warning");
                let code = Self::bounded_label(code);
                if !warning_codes.contains(&code) {
                    warning_codes.push(code);
                }
            }
        }
        let coverage = state_object
            .and_then(|state| state.get("coverage"))
            .and_then(Value::as_str)
            .map(Self::bounded_label);
        let mut trust_markers: Vec<String> = value
            .get("trust_markers")
            .and_then(Value::as_object)
            .map(|markers| {
                markers
                    .iter()
                    .filter(|(_, marker)| !marker.is_null() && *marker != &Value::Bool(false))
                    .take(32)
                    .map(|(name, _)| Self::bounded_label(name))
                    .collect()
            })
            .unwrap_or_default();
        trust_markers.sort();
        let source_links = sources
            .iter()
            .map(|source| RetrievalSourceLink {
                source_id: source.id.clone(),
                upstream_stable_id: source.note_value("stable_id").map(str::to_owned),
                group: source.note_value("group_label").map(str::to_owned),
                providers: source
                    .notes
                    .iter()
                    .filter_map(|note| note.strip_prefix("provider=").map(str::to_owned))
                    .collect(),
                published_at: source.published_at,
                provenance: source.note_value("upstream_provenance").map(str::to_owned),
            })
            .collect();
        ResearchRetrievalReceipt {
            adapter: "eggsearch".to_string(),
            state,
            source_count: sources.len(),
            coverage,
            warning_codes,
            trust_markers,
            failed_providers,
            sources: source_links,
        }
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
    ) -> Result<SourceCollection> {
        if let Some(value) = result.value {
            let sources = Self::convert_value(&value);
            // The full structured value is authoritative even when the
            // separately bounded display projection was truncated.
            let receipt = Self::receipt_from_value(&value, result.success, false, &sources);
            return Ok(SourceCollection {
                sources,
                receipts: vec![receipt],
            });
        }
        if result.truncated {
            return Ok(SourceCollection {
                sources: Vec::new(),
                receipts: vec![ResearchRetrievalReceipt {
                    adapter: "eggsearch".to_string(),
                    state: RetrievalState::Truncated,
                    source_count: 0,
                    coverage: None,
                    warning_codes: vec![],
                    trust_markers: vec![],
                    failed_providers: vec![],
                    sources: vec![],
                }],
            });
        }
        if !result.success {
            return Ok(SourceCollection {
                sources: Vec::new(),
                receipts: vec![ResearchRetrievalReceipt {
                    adapter: "eggsearch".to_string(),
                    state: RetrievalState::Failed,
                    source_count: 0,
                    coverage: None,
                    warning_codes: vec!["tool_error".to_string()],
                    trust_markers: vec![],
                    failed_providers: vec![],
                    sources: vec![],
                }],
            });
        }
        let sources = Self::convert_output(&result.output)?;
        Ok(SourceCollection {
            receipts: vec![ResearchRetrievalReceipt {
                adapter: "eggsearch".to_string(),
                state: RetrievalState::Unknown,
                source_count: sources.len(),
                coverage: None,
                warning_codes: vec![],
                trust_markers: vec![],
                failed_providers: vec![],
                sources: sources
                    .iter()
                    .map(|source| RetrievalSourceLink {
                        source_id: source.id.clone(),
                        upstream_stable_id: source.note_value("stable_id").map(str::to_owned),
                        group: source.note_value("group_label").map(str::to_owned),
                        providers: vec![],
                        published_at: source.published_at,
                        provenance: source.note_value("upstream_provenance").map(str::to_owned),
                    })
                    .collect(),
            }],
            sources,
        })
    }

    async fn collect_external(&self, request: &ResearchRequest) -> Result<SourceCollection> {
        let input = Self::request_input(request);
        // Explicit runtime context (production registry path) avoids the
        // deprecated process-global slots entirely. Legacy construction
        // without a context retains the global-wrapper fallback.
        let result = if let Some(runtime) = self.runtime.as_ref() {
            if request.mode == ResearchMode::SecurityReview {
                runtime.dispatch_security_search_structured(&input).await
            } else {
                runtime.dispatch_research_search_structured(&input).await
            }
        } else if request.mode == ResearchMode::SecurityReview {
            search_backend::dispatch_security_search_structured(&input).await
        } else {
            search_backend::dispatch_research_search_structured(&input).await
        };
        match result {
            Ok(result) => Self::convert_structured(result),
            Err(_error) => Ok(SourceCollection {
                sources: Vec::new(),
                receipts: vec![ResearchRetrievalReceipt {
                    adapter: "eggsearch".to_string(),
                    state: RetrievalState::Failed,
                    source_count: 0,
                    coverage: None,
                    warning_codes: vec!["tool_error".to_string()],
                    trust_markers: vec![],
                    failed_providers: vec![],
                    sources: vec![],
                }],
            }),
        }
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
            self.collect_with_receipt(request, _plan)
                .await
                .map(|collection| collection.sources)
        })
    }

    fn collect_with_receipt<'a>(
        &'a self,
        request: &'a ResearchRequest,
        _plan: &'a ResearchPlan,
    ) -> Pin<Box<dyn Future<Output = Result<SourceCollection>> + Send + 'a>> {
        Box::pin(async move {
            if !request.budget.allow_network {
                return Ok(SourceCollection {
                    sources: Vec::new(),
                    receipts: vec![ResearchRetrievalReceipt {
                        adapter: "eggsearch".to_string(),
                        state: RetrievalState::Skipped,
                        source_count: 0,
                        coverage: None,
                        warning_codes: vec!["network_policy_disabled".to_string()],
                        trust_markers: vec![],
                        failed_providers: vec![],
                        sources: vec![],
                    }],
                });
            }
            let mut collection = self.collect_external(request).await?;
            let was_truncated = collection.sources.len() > request.budget.max_sources;
            collection.sources.truncate(request.budget.max_sources);
            if was_truncated {
                if let Some(receipt) = collection.receipts.first_mut() {
                    receipt.state = RetrievalState::Truncated;
                    receipt
                        .warning_codes
                        .push("source_budget_truncated".to_string());
                    receipt.source_count = collection.sources.len();
                    receipt.sources.retain(|link| {
                        collection
                            .sources
                            .iter()
                            .any(|source| source.id == link.source_id)
                    });
                }
            }
            Ok(collection)
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
            success: true,
        };
        let collection = EggsearchSource::convert_structured(result).unwrap();
        assert_eq!(collection.sources.len(), 1);
        assert_eq!(collection.sources[0].uri, "https://example.com/structured");
    }

    #[test]
    fn truncated_text_only_projection_fails_explicitly() {
        let result = search_backend::StructuredSearchResult {
            output: "[external_research_evidence]\n{\"results\":[".to_string(),
            value: None,
            truncated: true,
            success: true,
        };
        let collection = EggsearchSource::convert_structured(result).unwrap();
        assert_eq!(collection.receipts[0].state, RetrievalState::Truncated);
    }

    #[test]
    fn preserves_nested_date_stable_identity_and_structured_outcome() {
        let result = search_backend::StructuredSearchResult {
            output: String::new(),
            value: Some(json!({
                "retrieval_state": {"status": "partial", "complete": false, "coverage": "2 of 3 providers"},
                "providers_failed": ["exa"],
                "structured_warnings": [{"code": "prompt_injection", "severity": "high"}],
                "trust_markers": {"sanitized": true, "injection_detected": true},
                "groups": [{"label": "Research", "results": [{
                    "stable_id": "src-42", "url": "https://example.com/x", "title": "X",
                    "metadata": {"published_at": "2025-03-04", "source_kind": "paper"},
                    "trust": "local_trusted", "providers": ["arxiv"]
                }]}]
            })),
            truncated: false,
            success: true,
        };
        let collection = EggsearchSource::convert_structured(result).unwrap();
        assert_eq!(collection.receipts[0].state, RetrievalState::Partial);
        assert_eq!(collection.receipts[0].failed_providers, ["exa"]);
        assert_eq!(collection.receipts[0].warning_codes, ["prompt_injection"]);
        assert_eq!(
            collection.receipts[0].trust_markers,
            ["injection_detected", "sanitized"]
        );
        let source = &collection.sources[0];
        assert_eq!(source.note_value("stable_id"), Some("src-42"));
        assert_eq!(
            source.published_at.unwrap().to_rfc3339(),
            "2025-03-04T00:00:00+00:00"
        );
        assert_eq!(source.note_value("trust"), Some("external_untrusted"));
        assert_eq!(
            source.note_value("upstream_provenance"),
            Some("local_trusted")
        );
        assert_eq!(collection.receipts[0].sources[0].source_id, source.id);
    }

    #[test]
    fn failed_complete_empty_and_unknown_results_remain_distinct() {
        let make = |success, value| search_backend::StructuredSearchResult {
            output: String::new(),
            value: Some(value),
            truncated: false,
            success,
        };
        let complete_empty = EggsearchSource::convert_structured(make(
            true,
            json!({
                "retrieval_state": {"status": "complete", "complete": true}, "groups": []
            }),
        ))
        .unwrap();
        let failed_empty =
            EggsearchSource::convert_structured(make(false, json!({"groups": []}))).unwrap();
        let partial_with_sources = EggsearchSource::convert_structured(make(
            false,
            json!({"groups": [{"results": [{"stable_id": "kept", "url": "https://example.com/kept"}]}]}),
        ))
        .unwrap();
        let providers_failed_empty = EggsearchSource::convert_structured(make(
            true,
            json!({"providers_failed": ["exa", "tavily"], "groups": []}),
        ))
        .unwrap();
        let unknown_empty =
            EggsearchSource::convert_structured(make(true, json!({"groups": []}))).unwrap();
        assert_eq!(complete_empty.receipts[0].state, RetrievalState::Complete);
        assert_eq!(failed_empty.receipts[0].state, RetrievalState::Failed);
        assert_eq!(
            partial_with_sources.receipts[0].state,
            RetrievalState::Partial
        );
        assert_eq!(partial_with_sources.sources.len(), 1);
        assert_eq!(
            providers_failed_empty.receipts[0].state,
            RetrievalState::Failed
        );
        assert_eq!(unknown_empty.receipts[0].state, RetrievalState::Unknown);
    }
}
