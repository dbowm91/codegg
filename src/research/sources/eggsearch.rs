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
const MAX_RETRIEVAL_DIMENSIONS: usize = 256;

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

    fn apply_source_budget(
        mut collection: SourceCollection,
        max_sources: usize,
    ) -> SourceCollection {
        let was_truncated = collection.sources.len() > max_sources;
        collection.sources.truncate(max_sources);
        if was_truncated {
            if let Some(receipt) = collection.receipts.first_mut() {
                receipt.state = RetrievalState::Truncated;
                if !receipt
                    .warning_codes
                    .iter()
                    .any(|code| code == "source_budget_truncated")
                {
                    receipt
                        .warning_codes
                        .push("source_budget_truncated".to_string());
                }
                receipt.source_count = collection.sources.len();
                receipt.sources.retain(|link| {
                    collection
                        .sources
                        .iter()
                        .any(|source| source.id == link.source_id)
                });
            }
        }
        collection
    }

    fn receipt_from_value(
        value: &Value,
        success: bool,
        truncated: bool,
        sources: &[SourceRecord],
    ) -> ResearchRetrievalReceipt {
        let summary = value.get("retrieval_summary");
        let legacy_state_object = value.get("retrieval_state");
        let state_object = legacy_state_object.or(summary);
        let state_value = legacy_state_object.and_then(|state| {
            state
                .get("status")
                .or_else(|| state.get("state"))
                .and_then(Value::as_str)
        });
        let complete = legacy_state_object
            .and_then(|state| state.get("complete"))
            .and_then(Value::as_bool);
        let mut failed_providers = Vec::new();
        if let Some(values) = value.get("providers_failed").and_then(Value::as_array) {
            for provider in values {
                let provider = provider
                    .as_str()
                    .or_else(|| provider.get("id").and_then(Value::as_str));
                if let Some(provider) = provider {
                    let provider = Self::bounded_label(provider);
                    if !provider.is_empty() && !failed_providers.contains(&provider) {
                        failed_providers.push(provider);
                    }
                }
            }
        }
        let mut dimension_states = Vec::new();
        let mut dimension_array_valid = false;
        let mut dimension_unknown = false;
        let mut dimension_failed = false;
        let mut dimension_skipped = false;
        let mut dimension_capability = false;
        let mut dimension_partial = false;
        let mut dimension_has_results = false;
        let mut dimension_conflict = false;
        let mut confirmed_truncation = false;
        let mut uncertain_limit = false;
        if let Some(dimensions) = summary.and_then(|s| s.get("dimensions")) {
            if let Some(dimensions) = dimensions.as_array() {
                dimension_array_valid = !dimensions.is_empty();
                if dimensions.len() > MAX_RETRIEVAL_DIMENSIONS {
                    dimension_unknown = true;
                }
                for dimension in dimensions.iter().take(MAX_RETRIEVAL_DIMENSIONS) {
                    let Some(object) = dimension.as_object() else {
                        dimension_unknown = true;
                        continue;
                    };
                    let state =
                        object.get("state").and_then(Value::as_str).or_else(|| {
                            match object.get("attempt_outcome").and_then(Value::as_str) {
                                Some("success_with_results") => Some("satisfied"),
                                Some("success_zero_results") => Some("completed_no_match"),
                                Some("failed" | "timed_out" | "rate_limited") => Some("failed"),
                                Some("skipped_by_policy") => Some("skipped_by_policy"),
                                Some("skipped_capability_unavailable") => {
                                    Some("capability_unavailable")
                                }
                                Some("interrupted_by_deadline") => Some("interrupted"),
                                Some("truncated_after_partial_success") => Some("partial"),
                                Some("not_applicable") => Some("not_applicable"),
                                _ => None,
                            }
                        });
                    if let (Some(explicit), Some(outcome)) = (
                        object.get("state").and_then(Value::as_str),
                        object.get("attempt_outcome").and_then(Value::as_str),
                    ) {
                        let outcome_state = match outcome {
                            "success_with_results" => Some("satisfied"),
                            "success_zero_results" => Some("completed_no_match"),
                            "failed" | "timed_out" | "rate_limited" => Some("failed"),
                            "skipped_by_policy" => Some("skipped_by_policy"),
                            "skipped_capability_unavailable" => Some("capability_unavailable"),
                            "interrupted_by_deadline" => Some("interrupted"),
                            "truncated_after_partial_success" => Some("partial"),
                            "not_applicable" => Some("not_applicable"),
                            _ => None,
                        };
                        let confirmed = matches!(
                            object.get("truncation_evidence").and_then(Value::as_str),
                            Some("confirmed_by_eggsearch" | "confirmed_by_provider")
                        ) || object.get("truncated").and_then(Value::as_bool)
                            == Some(true);
                        if outcome_state.is_some_and(|outcome| outcome != explicit)
                            && !(explicit == "partial" && confirmed)
                        {
                            dimension_conflict = true;
                        }
                    }
                    match state {
                        Some("satisfied") => {
                            dimension_has_results = true;
                            dimension_states.push("success");
                        }
                        Some("completed_no_match") => dimension_states.push("success"),
                        Some("failed" | "interrupted") => {
                            dimension_failed = true;
                            dimension_states.push("failed");
                        }
                        Some("skipped_by_policy") => {
                            dimension_skipped = true;
                            dimension_states.push("skipped");
                        }
                        Some("capability_unavailable") => {
                            dimension_capability = true;
                            dimension_states.push("capability");
                        }
                        Some("partial") => {
                            dimension_partial = true;
                            dimension_states.push("partial");
                        }
                        Some("not_applicable") => {
                            dimension_unknown = true;
                            dimension_states.push("unknown");
                        }
                        _ => {
                            dimension_unknown = true;
                            dimension_states.push("unknown");
                        }
                    }
                    let truncation = object.get("truncation_evidence").and_then(Value::as_str);
                    if matches!(
                        truncation,
                        Some("confirmed_by_eggsearch" | "confirmed_by_provider")
                    ) || object.get("truncated").and_then(Value::as_bool) == Some(true)
                        || object.get("attempt_outcome").and_then(Value::as_str)
                            == Some("truncated_after_partial_success")
                    {
                        confirmed_truncation = true;
                    } else if truncation == Some("limit_reached_unknown") {
                        uncertain_limit = true;
                    }
                    if matches!(state, Some("failed" | "interrupted")) {
                        if let Some(provider) = object.get("provider_id").and_then(Value::as_str) {
                            let provider = Self::bounded_label(provider);
                            if !provider.is_empty() && !failed_providers.contains(&provider) {
                                failed_providers.push(provider);
                            }
                        }
                    }
                }
            } else {
                dimension_unknown = true;
            }
        }
        failed_providers.truncate(32);
        let parsed_state = state_value.map(str::to_ascii_lowercase);
        let summary_has_failures = summary
            .and_then(|s| s.get("has_failures"))
            .and_then(Value::as_bool)
            == Some(true)
            || summary
                .and_then(|s| s.get("failed_job_count"))
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0)
            || summary
                .and_then(|s| s.get("failed_dimension_count"))
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0);
        let count_is_positive = |key: &str| {
            summary
                .and_then(|s| s.get(key))
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0)
        };
        let summary_failed = summary_has_failures
            || count_is_positive("timed_out_count")
            || count_is_positive("rate_limited_count")
            || count_is_positive("deadline_interrupted_count");
        let summary_skipped = count_is_positive("policy_skipped_count");
        let summary_capability = count_is_positive("capability_skipped_count");
        let roles_indeterminate = summary
            .and_then(|s| s.get("roles_indeterminate"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let role_count_conflict = summary
            .and_then(|s| {
                Some((
                    s.get("roles_attempted")?.as_u64()?,
                    s.get("roles_complete")?.as_u64()?,
                ))
            })
            .is_some_and(|(attempted, complete)| {
                complete > attempted || complete + roles_indeterminate > attempted
            });
        let attempted_job_count = summary
            .and_then(|s| s.get("attempted_job_count"))
            .and_then(Value::as_u64);
        let completed_job_count = summary
            .and_then(|s| s.get("completed_job_count"))
            .and_then(Value::as_u64);
        let attempted_dimension_count = summary
            .and_then(|s| s.get("attempted_dimension_count"))
            .and_then(Value::as_u64);
        let completed_dimension_count = summary
            .and_then(|s| s.get("completed_dimension_count"))
            .and_then(Value::as_u64);
        let unfinished_counts = attempted_job_count
            .zip(completed_job_count)
            .is_some_and(|(a, c)| c < a)
            || attempted_dimension_count
                .zip(completed_dimension_count)
                .is_some_and(|(a, c)| c < a);
        let workflow_coverage = value.get("workflow_coverage");
        let workflow_status = workflow_coverage
            .and_then(|coverage| coverage.get("status"))
            .and_then(Value::as_str);
        let missing_required_roles = workflow_coverage
            .and_then(|coverage| coverage.get("missing_required"))
            .and_then(Value::as_array)
            .is_some_and(|roles| !roles.is_empty());
        let workflow_indeterminate = workflow_status == Some("indeterminate_due_to_failures");
        let workflow_retrieval_failures = workflow_coverage
            .and_then(|coverage| coverage.get("retrieval_failures"))
            .and_then(Value::as_array)
            .is_some_and(|failures| !failures.is_empty());
        let workflow_has_gaps = workflow_status == Some("insufficient") || missing_required_roles;
        let satisfied_without_source_records = dimension_has_results && sources.is_empty();
        let summary_truncated = summary
            .and_then(|s| s.get("has_truncation"))
            .and_then(Value::as_bool)
            == Some(true)
            || summary
                .and_then(|s| s.get("truncated_count"))
                .and_then(Value::as_u64)
                .is_some_and(|count| count > 0);
        let summary_uncertain_limit = summary
            .and_then(|s| s.get("limit_reached_unknown_count"))
            .and_then(Value::as_u64)
            .is_some_and(|count| count > 0);
        let all_dimensions_failed = dimension_array_valid
            && !dimension_states.is_empty()
            && dimension_states.iter().all(|state| *state == "failed");
        let state = if truncated || confirmed_truncation || summary_truncated {
            RetrievalState::Truncated
        } else if !success || all_dimensions_failed {
            if sources.is_empty() {
                RetrievalState::Failed
            } else {
                RetrievalState::Partial
            }
        } else if dimension_partial {
            RetrievalState::Partial
        } else if dimension_failed
            || summary_failed
            || workflow_indeterminate
            || workflow_retrieval_failures
            || !failed_providers.is_empty()
        {
            if sources.is_empty() {
                RetrievalState::Failed
            } else {
                RetrievalState::Partial
            }
        } else if dimension_skipped || summary_skipped {
            RetrievalState::Skipped
        } else if dimension_skipped
            || dimension_capability
            || dimension_unknown
            || uncertain_limit
            || summary_uncertain_limit
            || summary_capability
            || roles_indeterminate > 0
            || role_count_conflict
            || unfinished_counts
            || dimension_conflict
            || satisfied_without_source_records
        {
            RetrievalState::Unknown
        } else if dimension_array_valid
            && dimension_states.iter().all(|state| *state == "success")
            && summary
                .and_then(|s| s.get("roles_indeterminate"))
                .and_then(Value::as_u64)
                .unwrap_or(0)
                == 0
            && !role_count_conflict
            && !unfinished_counts
            && !satisfied_without_source_records
        {
            RetrievalState::Complete
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
        let mut add_warning = |code: &str| {
            let code = Self::bounded_label(code);
            if !code.is_empty() && !warning_codes.contains(&code) && warning_codes.len() < 32 {
                warning_codes.push(code);
            }
        };
        if dimension_failed || summary_failed {
            add_warning("retrieval_dimension_failed");
        }
        if dimension_skipped {
            add_warning("retrieval_dimension_skipped_by_policy");
        }
        if summary_skipped {
            add_warning("retrieval_dimension_skipped_by_policy");
        }
        if dimension_capability || summary_capability {
            add_warning("retrieval_capability_unavailable");
        }
        if dimension_unknown {
            add_warning("retrieval_coverage_unknown");
        }
        if dimension_conflict {
            add_warning("retrieval_dimension_conflict");
        }
        if satisfied_without_source_records {
            add_warning("retrieval_results_missing_after_normalization");
        }
        if roles_indeterminate > 0 || role_count_conflict || unfinished_counts {
            add_warning("retrieval_role_coverage_incomplete");
        }
        if workflow_has_gaps {
            add_warning("workflow_coverage_gaps");
        }
        if workflow_indeterminate || workflow_retrieval_failures {
            add_warning("workflow_coverage_indeterminate");
        }
        if confirmed_truncation || summary_truncated {
            add_warning("retrieval_truncated");
        }
        if uncertain_limit || summary_uncertain_limit {
            add_warning("result_limit_completeness_unknown");
        }
        if summary
            .and_then(|s| s.get("has_failures"))
            .and_then(Value::as_bool)
            == Some(false)
            && (dimension_failed || summary_failed)
        {
            add_warning("retrieval_summary_conflict");
        }
        if let Some(warnings) = value.get("structured_warnings").and_then(Value::as_array) {
            for warning in warnings.iter().take(32) {
                let code = warning
                    .get("code")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown_warning");
                let code = Self::bounded_label(code);
                add_warning(&code);
            }
        }
        let coverage = state_object
            .and_then(|state| state.get("coverage"))
            .and_then(Value::as_str)
            .map(Self::bounded_label)
            .or_else(|| {
                summary
                    .and_then(|s| s.get("workflow_coverage"))
                    .and_then(Value::as_str)
                    .map(Self::bounded_label)
            })
            .or_else(|| workflow_status.map(Self::bounded_label));
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
            let collection = self.collect_external(request).await?;
            Ok(Self::apply_source_budget(
                collection,
                request.budget.max_sources,
            ))
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

    fn tagged_summary(dimensions: Value, flags: Value) -> Value {
        let mut summary = json!({"dimensions": dimensions, "has_failures": false,
            "has_absences": false, "has_truncation": false,
            "roles_attempted": 1, "roles_complete": 1, "roles_indeterminate": 0,
            "attempted_dimension_count": 1, "completed_dimension_count": 1,
            "failed_dimension_count": 0});
        for (key, value) in flags.as_object().into_iter().flatten() {
            summary[key] = value.clone();
        }
        summary
    }

    fn tagged_value(dimensions: Value, flags: Value, groups: Value) -> Value {
        json!({"success": true, "groups": groups,
            "workflow_coverage": {"complete": true},
            "retrieval_summary": tagged_summary(dimensions, flags),
            "structured_warnings": [], "providers_failed": []})
    }

    fn tagged_result(value: Value, success: bool) -> SourceCollection {
        EggsearchSource::convert_structured(search_backend::StructuredSearchResult {
            output: String::new(),
            value: Some(value),
            truncated: false,
            success,
        })
        .unwrap()
    }

    #[test]
    fn tagged_complete_empty_and_all_satisfied_are_positive_completion() {
        for state in ["completed_no_match", "satisfied"] {
            let groups = if state == "satisfied" {
                json!([{"results":[{"stable_id":"found-1","url":"https://example.com/found-1"}]}])
            } else {
                json!([])
            };
            let value = tagged_value(
                json!([{"evidence_role":"primary_sources", "provider_id":"exa",
                "state":state, "attempt_outcome": if state == "satisfied" {"success_with_results"} else {"success_zero_results"},
                "truncation_evidence":"none", "truncated":false}]),
                json!({}),
                groups,
            );
            let result = tagged_result(value, true);
            assert_eq!(
                result.receipts[0].state,
                RetrievalState::Complete,
                "{state}"
            );
            assert_eq!(
                result.receipts[0].source_count,
                usize::from(state == "satisfied")
            );
            let report = crate::research::synthesis::render_retrieval_status(&result.receipts);
            if state == "completed_no_match" {
                assert!(report.contains("complete; 0 source(s)"));
                assert!(report.contains("represented retrieval scope"));
                assert!(!report.contains("not a negative finding"));
            }
        }
        let mut no_match = tagged_value(
            json!([{"evidence_role":"primary_sources", "state":"completed_no_match",
                "attempt_outcome":"success_zero_results"}]),
            json!({}),
            json!([]),
        );
        no_match["workflow_coverage"] = json!({"status":"insufficient",
            "missing_required":["primary_sources"]});
        let receipt = &tagged_result(no_match, true).receipts[0];
        assert_eq!(receipt.state, RetrievalState::Complete);
        assert!(receipt
            .warning_codes
            .contains(&"workflow_coverage_gaps".to_string()));
        let missing_cards = tagged_value(
            json!([{"evidence_role":"primary_sources","state":"satisfied",
                "attempt_outcome":"success_with_results","result_count":1}]),
            json!({}),
            json!([]),
        );
        let result = tagged_result(missing_cards, true);
        assert_eq!(result.receipts[0].state, RetrievalState::Unknown);
        assert!(result.receipts[0]
            .warning_codes
            .contains(&"retrieval_results_missing_after_normalization".to_string()));
        assert!(
            crate::research::synthesis::render_retrieval_status(&result.receipts)
                .contains("not a negative finding")
        );
        let conflicting = tagged_value(
            json!([{"evidence_role":"primary_sources","state":"satisfied",
                "attempt_outcome":"failed"}]),
            json!({}),
            json!([{"results":[{"url":"https://example.com/retained"}]}]),
        );
        let result = tagged_result(conflicting, true);
        assert_eq!(result.receipts[0].state, RetrievalState::Unknown);
        assert!(result.receipts[0]
            .warning_codes
            .contains(&"retrieval_dimension_conflict".to_string()));
        assert!(
            crate::research::synthesis::render_retrieval_status(&result.receipts)
                .contains("not a negative finding")
        );
    }

    #[test]
    fn pinned_v041_research_search_fixture_proves_dimension_contract() {
        let value: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/eggsearch/v0.4.1/research_search-completed-empty.json"
        ))
        .unwrap();
        let result = tagged_result(value, true);
        assert_eq!(result.receipts[0].state, RetrievalState::Complete);
        assert_eq!(result.receipts[0].source_count, 0);
        let failed: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/eggsearch/v0.4.1/research_search-dimension-failure.json"
        ))
        .unwrap();
        let result = tagged_result(failed, true);
        assert_eq!(result.receipts[0].state, RetrievalState::Failed);
        assert_eq!(result.receipts[0].failed_providers, ["exa"]);
    }

    #[test]
    fn legacy_v039_text_only_compat_fixture_stays_unknown() {
        let text = include_str!(
            "../../../tests/fixtures/eggsearch/v0.3.9/research_search-text-only-compat.json"
        );
        let sources = EggsearchSource::convert_output(text).unwrap();
        let receipt =
            EggsearchSource::receipt_from_value(&json!({"groups":[]} ), true, false, &sources);
        assert_eq!(sources.len(), 1);
        assert_eq!(receipt.state, RetrievalState::Unknown);
    }

    #[test]
    fn tagged_mixed_dimension_failure_salvages_sources_and_provider() {
        let value = tagged_value(
            json!([
                {"evidence_role":"primary_sources", "provider_id":"good", "state":"satisfied", "attempt_outcome":"success_with_results"},
                {"evidence_role":"official_docs", "provider_id":"exa", "state":"failed", "attempt_outcome":"failed"}
            ]),
            json!({"has_failures":true,"roles_attempted":2,"roles_complete":1,"roles_indeterminate":1,
            "attempted_dimension_count":2,"completed_dimension_count":1,"failed_dimension_count":1}),
            json!([{"label":"Primary", "results":[{"stable_id":"kept", "url":"https://example.com/kept"}]}]),
        );
        let result = tagged_result(value, true);
        assert_eq!(result.receipts[0].state, RetrievalState::Partial);
        assert_eq!(result.receipts[0].failed_providers, ["exa"]);
        assert_eq!(result.sources.len(), 1);
        let report = crate::research::synthesis::render_retrieval_status(&result.receipts);
        assert!(report.contains("partial; 1 source(s)"));
        assert!(report.contains("Providers failed: exa"));
        assert!(report.contains("not a negative finding"));
    }

    #[test]
    fn tagged_all_failed_empty_is_not_a_completed_absence() {
        let value = tagged_value(
            json!([
                {"evidence_role":"primary_sources", "provider_id":"exa", "state":"failed", "attempt_outcome":"failed"},
                {"evidence_role":"official_docs", "provider_id":"exa", "state":"failed", "attempt_outcome":"failed"}
            ]),
            json!({"has_failures":true,"roles_attempted":2,"roles_complete":0,"roles_indeterminate":2,
            "attempted_dimension_count":2,"completed_dimension_count":0,"failed_dimension_count":2}),
            json!([]),
        );
        let result = tagged_result(value, true);
        assert_eq!(result.receipts[0].state, RetrievalState::Failed);
        assert_eq!(result.receipts[0].failed_providers, ["exa"]);
        let report = crate::research::synthesis::render_retrieval_status(&result.receipts);
        assert!(report.contains("failed; 0 source(s)"));
        assert!(report.contains("not a negative finding"));
    }

    #[test]
    fn tagged_policy_capability_partial_and_unknown_states_are_conservative() {
        for (state, expected) in [
            ("skipped_by_policy", RetrievalState::Skipped),
            ("capability_unavailable", RetrievalState::Unknown),
            ("interrupted", RetrievalState::Failed),
            ("partial", RetrievalState::Partial),
            ("future_state", RetrievalState::Unknown),
            ("not_applicable", RetrievalState::Unknown),
        ] {
            let value = tagged_value(
                json!([{"evidence_role":"primary_sources", "provider_id":"exa", "state":state}]),
                json!({"roles_complete":0,"roles_indeterminate":1}),
                json!([]),
            );
            let result = tagged_result(value, true);
            assert_eq!(result.receipts[0].state, expected, "{state}");
            let report = crate::research::synthesis::render_retrieval_status(&result.receipts);
            assert!(report.contains("not a negative finding"), "{state}");
        }
    }

    #[test]
    fn tagged_truncation_flags_and_unknown_limits_remain_visible() {
        let confirmed = tagged_value(
            json!([{"evidence_role":"primary_sources", "state":"satisfied",
            "truncation_evidence":"confirmed_by_provider", "truncated":true}]),
            json!({"has_truncation":true,"truncated_count":1}),
            json!([]),
        );
        let confirmed = tagged_result(confirmed, true);
        assert_eq!(confirmed.receipts[0].state, RetrievalState::Truncated);
        let confirmed_report =
            crate::research::synthesis::render_retrieval_status(&confirmed.receipts);
        assert!(confirmed_report.contains("not a negative finding"));
        let uncertain = tagged_value(
            json!([{"evidence_role":"primary_sources", "state":"satisfied",
            "truncation_evidence":"limit_reached_unknown"}]),
            json!({"limit_reached_unknown_count":1}),
            json!([]),
        );
        let receipt = &tagged_result(uncertain, true).receipts[0];
        assert_eq!(receipt.state, RetrievalState::Unknown);
        assert!(receipt
            .warning_codes
            .contains(&"result_limit_completeness_unknown".to_string()));
        let uncertain_report =
            crate::research::synthesis::render_retrieval_status(std::slice::from_ref(receipt));
        assert!(uncertain_report.contains("not a negative finding"));
    }

    #[test]
    fn tagged_false_success_overrides_complete_dimensions_but_keeps_cards() {
        let value = tagged_value(
            json!([{"evidence_role":"primary_sources", "state":"satisfied"}]),
            json!({}),
            json!([{"results":[{"stable_id":"usable", "url":"https://example.com/usable"}]}]),
        );
        let result = tagged_result(value, false);
        assert_eq!(result.receipts[0].state, RetrievalState::Partial);
        assert_eq!(result.sources.len(), 1);
        let report = crate::research::synthesis::render_retrieval_status(&result.receipts);
        assert!(report.contains("partial; 1 source(s)"));
        assert!(report.contains("not a negative finding"));
    }

    #[test]
    fn malformed_dimensions_and_compact_hints_never_claim_complete() {
        for summary in [
            json!({"dimensions":[null],"has_failures":false}),
            json!({"dimensions":{},"has_failures":false}),
            json!({"has_failures":false,"has_absences":true}),
            json!({"status":"complete","complete":true,"has_failures":false}),
        ] {
            let value = json!({"retrieval_summary":summary,"retrieval_status":{"has_failures":false},"groups":[]});
            let receipt = &tagged_result(value, true).receipts[0];
            assert_ne!(receipt.state, RetrievalState::Complete);
            let report =
                crate::research::synthesis::render_retrieval_status(std::slice::from_ref(receipt));
            assert!(report.contains("not a negative finding"));
        }
        let conflict = tagged_value(
            json!([{"evidence_role":"primary_sources","state":"failed"}]),
            json!({"has_failures":false,"roles_indeterminate":1,"roles_complete":0}),
            json!([]),
        );
        let receipt = &tagged_result(conflict, true).receipts[0];
        assert_eq!(receipt.state, RetrievalState::Failed);
        assert!(receipt
            .warning_codes
            .contains(&"retrieval_summary_conflict".to_string()));
        assert!(
            crate::research::synthesis::render_retrieval_status(std::slice::from_ref(receipt))
                .contains("not a negative finding")
        );
    }

    #[test]
    fn oversized_dimension_diagnostic_stays_bounded_and_indeterminate() {
        let dimensions = vec![
            json!({"evidence_role":"primary_sources", "state":"satisfied"});
            MAX_RETRIEVAL_DIMENSIONS + 1
        ];
        let receipt =
            &tagged_result(tagged_value(json!(dimensions), json!({}), json!([])), true).receipts[0];
        assert_eq!(receipt.state, RetrievalState::Unknown);
        assert!(receipt
            .warning_codes
            .contains(&"retrieval_coverage_unknown".to_string()));
    }

    #[test]
    fn coordinator_source_budget_cap_marks_receipt_and_retained_links() {
        let mut req = request(ResearchMode::Landscape);
        req.budget.max_sources = 1;
        let sources = (0..2)
            .map(|i| SourceRecord {
                id: format!("source-{i}"),
                run_id: "run".into(),
                title: None,
                uri: format!("https://example.com/{i}"),
                source_type: SourceType::Url,
                retrieved_at: Utc::now(),
                published_at: None,
                source_quality: SourceQuality::Secondary,
                content_hash: None,
                locator: SourceLocator::Url {
                    url: format!("https://example.com/{i}"),
                    heading: None,
                },
                notes: vec![],
            })
            .collect::<Vec<_>>();
        let collection = SourceCollection {
            sources,
            receipts: vec![ResearchRetrievalReceipt {
                adapter: "eggsearch".into(),
                state: RetrievalState::Complete,
                source_count: 2,
                coverage: None,
                warning_codes: vec![],
                trust_markers: vec![],
                failed_providers: vec![],
                sources: (0..2)
                    .map(|i| RetrievalSourceLink {
                        source_id: format!("source-{i}"),
                        upstream_stable_id: None,
                        group: None,
                        providers: vec![],
                        published_at: None,
                        provenance: None,
                    })
                    .collect(),
            }],
        };
        let collection = EggsearchSource::apply_source_budget(collection, req.budget.max_sources);
        assert_eq!(collection.receipts[0].state, RetrievalState::Truncated);
        assert_eq!(collection.receipts[0].source_count, 1);
        assert!(collection.receipts[0]
            .warning_codes
            .contains(&"source_budget_truncated".to_string()));
        assert_eq!(collection.receipts[0].sources.len(), 1);
        assert_eq!(collection.receipts[0].sources[0].source_id, "source-0");
        let report = crate::research::synthesis::render_retrieval_status(&collection.receipts);
        assert!(report.contains("truncated; 1 source(s)"));
        assert!(report.contains("not a negative finding"));
    }
}
