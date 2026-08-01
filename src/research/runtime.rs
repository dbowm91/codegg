//! Bounded host-side coordination contracts for the specialized research
//! runtime. The ordinary agent loop remains authoritative for tools,
//! permissions, scheduling, cancellation, and final model interaction.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::types::{Confidence, SourceRecord};

pub const MAX_CHILD_TASKS: usize = 3;
pub const MAX_SOURCES: usize = 32;
pub const MAX_EVIDENCE: usize = 96;
pub const MAX_CLAIMS: usize = 48;
pub const MAX_TEXT_CHARS: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestKind {
    QuickLookup,
    DirectInvestigation,
    MultiSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResearchRequest {
    pub question: String,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChildRole {
    SourceScout,
    RepositoryInvestigator,
    ClaimVerifier,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchTask {
    pub id: String,
    pub role: ChildRole,
    pub objective: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundedResearchPlan {
    pub request: RuntimeResearchRequest,
    pub kind: RequestKind,
    pub tasks: Vec<ResearchTask>,
    pub max_sources: usize,
    pub max_evidence: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRelation {
    Supports,
    Contradicts,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: String,
    pub source_id: String,
    pub claim_fragment: String,
    pub relation: EvidenceRelation,
    pub excerpt: String,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimConflict {
    pub claim_ids: Vec<String>,
    pub explanation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchEvidenceReport {
    pub task_id: String,
    pub sources: Vec<SourceRecord>,
    pub evidence: Vec<EvidenceRecord>,
    pub claims: Vec<ClaimRecord>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimRecord {
    pub id: String,
    pub text: String,
    pub confidence: Confidence,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResearchReport {
    pub question: String,
    pub claims: Vec<ClaimRecord>,
    pub sources: Vec<SourceRecord>,
    pub evidence: Vec<EvidenceRecord>,
    pub conflicts: Vec<ClaimConflict>,
    pub unresolved_questions: Vec<String>,
    pub limitations: Vec<String>,
}

pub fn classify(request: &RuntimeResearchRequest) -> RequestKind {
    let q = request.question.to_ascii_lowercase();
    if q.split_whitespace().count() <= 12
        && !["compare", "versus", "tradeoff", "landscape", "alternatives"]
            .iter()
            .any(|word| q.contains(word))
    {
        if request.scope.is_some()
            || ["repository", "code", "spec", "architecture"]
                .iter()
                .any(|word| q.contains(word))
        {
            RequestKind::DirectInvestigation
        } else {
            RequestKind::QuickLookup
        }
    } else {
        RequestKind::MultiSource
    }
}

pub fn build_plan(request: RuntimeResearchRequest) -> BoundedResearchPlan {
    let kind = classify(&request);
    let tasks = match kind {
        RequestKind::QuickLookup => Vec::new(),
        RequestKind::DirectInvestigation => vec![ResearchTask {
            id: "repository-investigator".into(),
            role: ChildRole::RepositoryInvestigator,
            objective: request.question.clone(),
            scope: request
                .scope
                .clone()
                .unwrap_or_else(|| "repository and specifications".into()),
        }],
        RequestKind::MultiSource => vec![
            ResearchTask {
                id: "source-scout-1".into(),
                role: ChildRole::SourceScout,
                objective: request.question.clone(),
                scope: "primary and official sources".into(),
            },
            ResearchTask {
                id: "source-scout-2".into(),
                role: ChildRole::SourceScout,
                objective: request.question.clone(),
                scope: "independent or comparative sources".into(),
            },
            ResearchTask {
                id: "claim-verifier".into(),
                role: ChildRole::ClaimVerifier,
                objective: "verify collected claims and identify conflicts".into(),
                scope: "collected evidence only".into(),
            },
        ],
    };
    BoundedResearchPlan {
        request,
        kind,
        tasks,
        max_sources: MAX_SOURCES,
        max_evidence: MAX_EVIDENCE,
    }
}

/// Normalize common URL/document variants to a stable deduplication key.
pub fn normalize_source_identity(locator: &str) -> String {
    let trimmed = locator.trim();
    if let Ok(mut url) = url::Url::parse(trimmed) {
        url.set_fragment(None);
        if url.path().ends_with('/') && url.path() != "/" {
            let path = url.path().trim_end_matches('/').to_string();
            url.set_path(&path);
        }
        return url.to_string();
    }
    std::path::Path::new(trimmed)
        .components()
        .collect::<std::path::PathBuf>()
        .to_string_lossy()
        .into_owned()
}

pub fn deduplicate_sources(sources: impl IntoIterator<Item = SourceRecord>) -> Vec<SourceRecord> {
    let mut seen = BTreeSet::new();
    sources
        .into_iter()
        .filter(|source| seen.insert(normalize_source_identity(&source.uri)))
        .take(MAX_SOURCES)
        .collect()
}

pub fn validate_report(report: &ResearchReport) -> Result<(), Vec<String>> {
    // This validator is the completion gate for the specialized runtime, not
    // merely a provider-schema check. Providers may ignore the requested
    // schema, so all references and bounded payloads are checked locally.
    let source_ids: BTreeSet<_> = report
        .sources
        .iter()
        .map(|source| source.id.as_str())
        .collect();
    let evidence_ids: BTreeSet<_> = report
        .evidence
        .iter()
        .map(|evidence| evidence.id.as_str())
        .collect();
    let cited_source_ids: BTreeSet<_> = report
        .evidence
        .iter()
        .map(|evidence| evidence.source_id.as_str())
        .collect();
    let mut errors = Vec::new();
    if report.sources.len() > MAX_SOURCES {
        errors.push("source limit exceeded".into());
    }
    if report.claims.len() > MAX_CLAIMS {
        errors.push("claim limit exceeded".into());
    }
    if report.evidence.len() > MAX_EVIDENCE {
        errors.push("evidence limit exceeded".into());
    }
    if report.unresolved_questions.len() > 32 || report.limitations.len() > 32 {
        errors.push("limitation or unresolved-question limit exceeded".into());
    }
    for evidence in &report.evidence {
        if evidence.claim_fragment.len() > MAX_TEXT_CHARS || evidence.excerpt.len() > MAX_TEXT_CHARS
        {
            errors.push(format!("evidence {} exceeds text bound", evidence.id));
        }
    }
    for claim in &report.claims {
        if claim.text.trim().is_empty() {
            errors.push(format!("claim {} is empty", claim.id));
        }
        for evidence_id in &claim.evidence_ids {
            if !evidence_ids.contains(evidence_id.as_str()) {
                errors.push(format!(
                    "claim {} has malformed evidence reference",
                    claim.id
                ));
            }
        }
    }
    for source_id in &cited_source_ids {
        if !source_ids.contains(source_id) {
            errors.push(format!("unknown source reference {source_id}"));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn bounded_text(text: &str) -> String {
    text.chars().take(MAX_TEXT_CHARS).collect()
}

pub fn progress_summary(
    plan: &BoundedResearchPlan,
    reports: &[ResearchEvidenceReport],
) -> BTreeMap<&'static str, usize> {
    BTreeMap::from([
        ("planned_tasks", plan.tasks.len()),
        ("completed_tasks", reports.len().min(plan.tasks.len())),
        (
            "sources",
            deduplicate_sources(reports.iter().flat_map(|r| r.sources.clone())).len(),
        ),
        (
            "evidence",
            reports
                .iter()
                .flat_map(|r| r.evidence.iter())
                .count()
                .min(MAX_EVIDENCE),
        ),
    ])
}

pub fn report_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object", "additionalProperties": false,
        "required": ["question", "claims", "sources", "evidence", "conflicts", "unresolved_questions", "limitations"],
        "properties": {
            "question": {"type": "string"}, "claims": {"type": "array"},
            "sources": {"type": "array"}, "evidence": {"type": "array"}, "conflicts": {"type": "array"},
            "unresolved_questions": {"type": "array"}, "limitations": {"type": "array"}
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(question: &str) -> RuntimeResearchRequest {
        RuntimeResearchRequest {
            question: question.into(),
            scope: None,
        }
    }

    #[test]
    fn classification_is_bounded_and_deterministic() {
        assert_eq!(
            classify(&request("What is Tokio?")),
            RequestKind::QuickLookup
        );
        assert_eq!(
            classify(&request("Inspect this repository architecture")),
            RequestKind::DirectInvestigation
        );
        assert_eq!(
            classify(&request(
                "Compare Tokio versus async-std tradeoffs and alternatives"
            )),
            RequestKind::MultiSource
        );
    }

    #[test]
    fn multi_source_plan_has_non_overlapping_roles_and_three_tasks() {
        let plan = build_plan(request("Compare two approaches and their tradeoffs"));
        assert_eq!(plan.tasks.len(), MAX_CHILD_TASKS);
        let ids: BTreeSet<_> = plan.tasks.iter().map(|task| task.id.as_str()).collect();
        assert_eq!(ids.len(), plan.tasks.len());
    }

    #[test]
    fn urls_are_deduplicated_and_fragments_ignored() {
        assert_eq!(
            normalize_source_identity("https://example.test/docs/#intro"),
            "https://example.test/docs"
        );
    }

    #[test]
    fn report_rejects_unknown_citation() {
        let report = ResearchReport {
            question: "q".into(),
            claims: vec![ClaimRecord {
                id: "c".into(),
                text: "claim".into(),
                confidence: Confidence::Low,
                evidence_ids: vec!["missing".into()],
            }],
            sources: vec![],
            evidence: vec![],
            conflicts: vec![],
            unresolved_questions: vec![],
            limitations: vec![],
        };
        assert!(validate_report(&report).is_err());
    }
}
