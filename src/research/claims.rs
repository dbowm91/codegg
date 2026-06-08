use crate::provider::Provider;
use crate::research::llm;
use crate::research::templates;
use crate::research::types::*;

/// Build claims deterministically from evidence (no model).
/// Creates one low-confidence claim per evidence span.
pub fn deterministic_claims(
    run_id: &str,
    evidence: &[EvidenceSpan],
    sources: &[SourceRecord],
) -> Vec<ClaimRecord> {
    evidence
        .iter()
        .map(|evid| {
            let source = sources.iter().find(|s| s.id == evid.source_id);
            let source_desc = source
                .map(|s| s.title.as_deref().unwrap_or(&s.uri))
                .unwrap_or("unknown source");

            ClaimRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                text: format!(
                    "Source '{}' contains potentially relevant information about the research question.",
                    source_desc
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
    _model_available: bool,
) -> Vec<ClaimRecord> {
    // For MVP, always use deterministic fallback
    deterministic_claims(run_id, evidence, sources)
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
    question: &str,
) -> Vec<ClaimRecord> {
    if evidence.is_empty() {
        return Vec::new();
    }

    // Build a compact evidence summary for the prompt
    let evidence_brief: Vec<serde_json::Value> = evidence
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

    let json_val = match llm::call_llm_json(provider, model, None, &prompt, Some(4096)).await {
        Ok(v) => v,
        Err(_) => {
            return deterministic_claims(run_id, evidence, sources);
        }
    };

    let items: Vec<ModelClaimItem> = match serde_json::from_value(json_val) {
        Ok(v) => v,
        Err(_) => {
            return deterministic_claims(run_id, evidence, sources);
        }
    };

    if items.is_empty() {
        return deterministic_claims(run_id, evidence, sources);
    }

    items
        .into_iter()
        .map(|item| {
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

            // Filter evidence_ids to only those that actually exist
            let evidence_ids: Vec<String> = item
                .evidence_ids
                .unwrap_or_default()
                .into_iter()
                .filter(|id| evidence.iter().any(|e| &e.id == id))
                .collect();

            let mut caveats = item.caveats.unwrap_or_default();
            if evidence_ids.is_empty() {
                caveats
                    .push("Model-generated claim with no matched evidence references".to_string());
            }

            ClaimRecord {
                id: uuid::Uuid::new_v4().to_string(),
                run_id: run_id.to_string(),
                text: item.text,
                claim_type,
                confidence,
                evidence_ids,
                caveats,
                applies_to: item.applies_to.unwrap_or_default(),
            }
        })
        .collect()
}

fn truncate_for_prompt(s: &str, max_chars: usize) -> String {
    if s.len() <= max_chars {
        s.to_string()
    } else {
        format!("{}...", &s[..max_chars])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

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
        let claims = deterministic_claims("run-1", &evidence, &sources);
        assert_eq!(claims.len(), 3);
    }

    #[test]
    fn deterministic_claims_uses_source_title() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &sources);
        assert!(claims[0].text.contains("Title of foo.rs"));
    }

    #[test]
    fn deterministic_claims_falls_back_to_uri() {
        let mut source = make_source("src-1", "https://example.com");
        source.title = None;
        let sources = vec![source];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &sources);
        assert!(claims[0].text.contains("https://example.com"));
    }

    #[test]
    fn deterministic_claims_unknown_source() {
        let evidence = vec![make_evidence("ev-1", "nonexistent")];
        let claims = deterministic_claims("run-1", &evidence, &[]);
        assert!(claims[0].text.contains("unknown source"));
    }

    #[test]
    fn deterministic_claims_sets_low_confidence() {
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &[]);
        assert_eq!(claims[0].confidence, Confidence::Low);
    }

    #[test]
    fn deterministic_claims_includes_caveats() {
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = deterministic_claims("run-1", &evidence, &[]);
        assert_eq!(claims[0].caveats.len(), 2);
        assert!(claims[0].caveats[0].contains("deterministic fallback"));
    }

    #[test]
    fn deterministic_claims_links_evidence() {
        let evidence = vec![
            make_evidence("ev-1", "src-1"),
            make_evidence("ev-2", "src-1"),
        ];
        let claims = deterministic_claims("run-1", &evidence, &[]);
        assert_eq!(claims[0].evidence_ids, vec!["ev-1"]);
        assert_eq!(claims[1].evidence_ids, vec!["ev-2"]);
    }

    #[test]
    fn build_claims_delegates_to_deterministic() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let claims = build_claims("run-1", &evidence, &sources, false);
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].confidence, Confidence::Low);
    }

    #[test]
    fn build_claims_ignores_model_flag() {
        let sources = vec![make_source("src-1", "foo.rs")];
        let evidence = vec![make_evidence("ev-1", "src-1")];
        let with_model = build_claims("run-1", &evidence, &sources, true);
        let without_model = build_claims("run-1", &evidence, &sources, false);
        // Both should produce identical results (deterministic fallback)
        assert_eq!(with_model.len(), without_model.len());
        assert_eq!(with_model[0].confidence, without_model[0].confidence);
    }

    #[test]
    fn deterministic_claims_empty_evidence() {
        let claims = deterministic_claims("run-1", &[], &[]);
        assert!(claims.is_empty());
    }
}
