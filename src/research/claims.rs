use crate::provider::{Provider, ProviderRequestContext};
use crate::research::llm;
use crate::research::templates;
use crate::research::types::*;

/// Normalize for query-restatement comparison: lowercase, collapse whitespace.
fn normalize_for_relevance(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Relevance filter applied BEFORE claim construction and envelope build.
/// Drops empty spans and exact query restatements so the envelope never carries vacuous claims.
pub fn is_relevant_evidence(evidence: &EvidenceSpan, question: &str) -> bool {
    let text = evidence.text.trim();
    if text.len() < 10 {
        return false;
    }
    let norm_text = normalize_for_relevance(text);
    let norm_q = normalize_for_relevance(question.trim());
    if !norm_q.is_empty() && norm_text == norm_q {
        return false;
    }
    text.split(|c: char| !c.is_alphanumeric())
        .any(|tok| tok.len() >= 3)
}

pub fn filter_relevant_evidence<'a>(
    evidence: &'a [EvidenceSpan],
    question: &str,
) -> Vec<&'a EvidenceSpan> {
    evidence
        .iter()
        .filter(|e| is_relevant_evidence(e, question))
        .collect()
}

fn truncate_predicate(s: &str, max_chars: usize) -> String {
    let mut out = crate::util::truncate_prefix(s, max_chars).to_string();
    if out.len() < s.len() {
        out.push('…');
    }
    out
}

/// Build claims deterministically from evidence (no model).
/// Each claim requires a predicate plus evidence_ids/span — query restatements
/// are dropped by [`filter_relevant_evidence`] before construction.
pub fn deterministic_claims(
    run_id: &str,
    evidence: &[EvidenceSpan],
    sources: &[SourceRecord],
    question: &str,
) -> Vec<ClaimRecord> {
    filter_relevant_evidence(evidence, question)
        .into_iter()
        .map(|evid| {
            let source = sources.iter().find(|s| s.id == evid.source_id);
            let source_desc = source
                .map(|s| s.title.as_deref().unwrap_or(&s.uri))
                .unwrap_or("unknown source");

            ClaimRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                text: format!(
                    "Evidence from '{}' supports: {}",
                    source_desc,
                    truncate_predicate(evid.text.trim(), 280),
                ),
                claim_type: ClaimType::Inference,
                confidence: Confidence::Low,
                evidence_ids: vec![evid.id.clone()],
                caveats: vec![
                    "This is a deterministic fallback claim, not model-analyzed".to_string(),
                    "The evidence may or may not be directly relevant".to_string(),
                ],
                applies_to: vec![],
            }
        })
        .collect()
}

/// Build claims from evidence records.
/// With a model, this would do intelligent claim construction.
/// Without a model, uses deterministic fallback.
pub fn build_claims(
    run_id: &str,
    evidence: &[EvidenceSpan],
    sources: &[SourceRecord],
    question: &str,
    _model_available: bool,
) -> Vec<ClaimRecord> {
    deterministic_claims(run_id, evidence, sources, question)
}

/// Parsed claim item from model response.
#[derive(serde::Deserialize)]
struct ModelClaimItem {
    text: String,
    claim_type: Option<String>,
    confidence: Option<String>,
    evidence_ids: Option<Vec<String>>,
    caveats: Option<Vec<String>>,
    applies_to: Option<Vec<String>>,
}

/// Build claims using the LLM for intelligent claim construction.
///
/// Sends all evidence to the model with the CLAIM_CONSTRUCTION_PROMPT template
/// and parses structured claim records from the JSON response. Falls back to
/// deterministic claims on any error.
pub async fn build_claims_with_model(
    run_id: &str,
    evidence: &[EvidenceSpan],
    sources: &[SourceRecord],
    provider: &dyn Provider,
    model: &str,
    context: ProviderRequestContext,
    question: &str,
) -> Vec<ClaimRecord> {
    let relevant: Vec<&EvidenceSpan> = filter_relevant_evidence(evidence, question);
    if relevant.is_empty() {
        return Vec::new();
    }

    // Build a compact evidence summary for the prompt
    let evidence_brief: Vec<serde_json::Value> = relevant
        .iter()
        .map(|e| {
            let source = sources.iter().find(|s| s.id == e.source_id);
            let source_desc = source.and_then(|s| s.title.as_deref()).unwrap_or("unknown");
            serde_json::json!({
                "id": e.id,
                "source": source_desc,
                "text_preview": truncate_for_prompt(&e.text, 500),
                "summary": e.summary,
            })
        })
        .collect();

    let evidence_json = serde_json::to_string_pretty(&evidence_brief).unwrap_or_default();

    let prompt = templates::CLAIM_CONSTRUCTION_PROMPT
        .replace("{question}", question)
        .replace("{evidence_json}", &evidence_json);

    let json_val =
        match llm::call_llm_json(provider, model, context, None, &prompt, Some(4096)).await {
            Ok(v) => v,
            Err(_) => {
                return deterministic_claims(run_id, evidence, sources, question);
            }
        };

    let items: Vec<ModelClaimItem> = match serde_json::from_value(json_val) {
        Ok(v) => v,
        Err(_) => {
            return deterministic_claims(run_id, evidence, sources, question);
        }
    };

    if items.is_empty() {
        return deterministic_claims(run_id, evidence, sources, question);
    }

    let norm_q = normalize_for_relevance(question);
    items
        .into_iter()
        .filter_map(|item| {
            let text = item.text.trim().to_string();
            if text.len() < 10 {
                return None;
            }
            if !norm_q.is_empty() && normalize_for_relevance(&text) == norm_q {
                return None;
            }
            let claim_type = match item.claim_type.as_deref() {
                Some("fact") => ClaimType::Fact,
                Some("comparison") => ClaimType::Comparison,
                Some("recommendation") => ClaimType::Recommendation,
                Some("risk") => ClaimType::Risk,
                Some("caveat") => ClaimType::Caveat,
                Some("open_question") => ClaimType::OpenQuestion,
                _ => ClaimType::Inference,
            };

            let confidence = match item.confidence.as_deref() {
                Some("high") => Confidence::High,
                Some("medium") => Confidence::Medium,
                _ => Confidence::Low,
            };

            let evidence_ids: Vec<String> = item
                .evidence_ids
                .unwrap_or_default()
                .into_iter()
                .filter(|id| evidence.iter().any(|e| &e.id == id))
                .collect();
            if evidence_ids.is_empty() {
                return None;
            }

            let caveats = item.caveats.unwrap_or_default();

            Some(ClaimRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                text,
                claim_type,
                confidence,
                evidence_ids,
                caveats,
                applies_to: item.applies_to.unwrap_or_default(),
            })
        })
        .collect()
}

fn truncate_for_prompt(s: &str, max_chars: usize) -> String {
    let mut out = crate::util::truncate_prefix(s, max_chars).to_string();
    if out.len() < s.len() {
        out.push_str("...");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    #[test]
    fn truncate_for_prompt_is_utf8_boundary_safe() {
        let s = "证据".repeat(300); // 1800 bytes
        let out = truncate_for_prompt(&s, 500);
        assert!(out.starts_with("证据"));
        assert!(out.ends_with("..."));
        assert_eq!(truncate_for_prompt(&s, s.len()), s);
    }

    fn make_source(id: &str, uri: &str) -> SourceRecord {
        SourceRecord {
            id: id.to_string(),
            run_id: "run-1".to_string(),
            uri: uri.to_string(),
            title: Some(format!("Title of {uri}")),
            source_type: SourceType::LocalFile,
            source_quality: SourceQuality::SourceCode,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: None,
            locator: SourceLocator::FileRange {
                path: PathBuf::from(uri),
                start_line: 1,
                end_line: 100,
            },
            notes: vec![],
        }
    }

    fn make_evidence(id: &str, source_id: &str) -> EvidenceSpan {
        EvidenceSpan {
            id: id.to_string(),
            run_id: "run-1".to_string(),
            source_id: source_id.to_string(),
            locator: SourceLocator::TextSpan {
                label: "test".to_string(),
            },
            text: "some evidence text".to_string(),
            summary: None,
            extracted_at: Utc::now(),
        }
    }

    #[test]
    fn deterministic_claims_creates_one_per_span() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![
            make_evidence("ev-1", "src-1"),
            make_evidence("ev-2", "src-1"),
            make_evidence("ev-3", "src-1"),
        ];
        let claims = deterministic_claims("run-1", &evidence, &sources, "What is foo?");
        assert_eq!(claims.len(), 3);
    }

    #[test]
    fn deterministic_claims_uses_source_title() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &sources, "What is foo?");
        assert!(claims[0].text.contains("Title of foo.rs"));
    }

    #[test]
    fn deterministic_claims_falls_back_to_uri() {
        let mut source = make_source("src-1", "https://example.com");
        source.title = None;
        let sources = vec![source];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &sources, "What is foo?");
        assert!(claims[0].text.contains("https://example.com"));
    }

    #[test]
    fn deterministic_claims_unknown_source() {
        let evidence = vec![make_evidence("ev-1", "nonexistent")];
        let claims = deterministic_claims("run-1", &evidence, &[], "What is foo?");
        assert!(claims[0].text.contains("unknown source"));
    }

    #[test]
    fn deterministic_claims_sets_low_confidence() {
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &[], "What is foo?");
        assert_eq!(claims[0].confidence, Confidence::Low);
    }

    #[test]
    fn deterministic_claims_includes_caveats() {
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &[], "What is foo?");
        assert_eq!(claims[0].caveats.len(), 2);
        assert!(claims[0].caveats[0].contains("deterministic fallback"));
    }

    #[test]
    fn deterministic_claims_links_evidence() {
        let evidence = vec![
            make_evidence("ev-1", "src-1"),
            make_evidence("ev-2", "src-1"),
        ];
        let claims = deterministic_claims("run-1", &evidence, &[], "What is foo?");
        assert_eq!(claims[0].evidence_ids, vec!["ev-1"]);
        assert_eq!(claims[1].evidence_ids, vec!["ev-2"]);
    }

    #[test]
    fn build_claims_delegates_to_deterministic() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = build_claims("run-1", &evidence, &sources, "What is foo?", false);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].confidence, Confidence::Low);
    }

    #[test]
    fn build_claims_ignores_model_flag() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let with_model = build_claims("run-1", &evidence, &sources, "What is foo?", true);
        let without_model = build_claims("run-1", &evidence, &sources, "What is foo?", false);
        // Both should produce identical results (deterministic fallback)
        assert_eq!(with_model.len(), without_model.len());
        assert_eq!(with_model[0].confidence, without_model[0].confidence);
    }

    #[test]
    fn deterministic_claims_empty_evidence() {
        let claims = deterministic_claims("run-1", &[], &[], "What is foo?");
        assert!(claims.is_empty());
    }

    #[test]
    fn deterministic_claims_drops_query_restatements() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let mut restatement = make_evidence("ev-1", "src-1");
        restatement.text = "What is foo?".to_string();
        let claims = deterministic_claims("run-1", &[restatement], &sources, "What is foo?");
        assert!(
            claims.is_empty(),
            "query restatement must not become a claim"
        );
    }

    #[test]
    fn deterministic_claims_requires_predicate_text() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &sources, "What is foo?");
        assert_eq!(claims.len(), 1);
        assert!(!claims[0].evidence_ids.is_empty());
        assert!(
            claims[0].text.contains("some evidence text"),
            "claim must quote its predicate, got: {}",
            claims[0].text
        );
    }
}
