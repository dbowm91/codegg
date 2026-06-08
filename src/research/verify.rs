use std::collections::HashSet;

use crate::provider::Provider;
use crate::research::llm;
use crate::research::templates;
use crate::research::types::*;

#[derive(Debug)]
pub struct VerificationResult {
    pub passed: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Result of semantic citation verification for a single claim.
#[derive(Debug, Clone)]
pub struct SemanticCheckResult {
    pub claim_id: String,
    pub support_status: String, // "supported", "partially_supported", "unsupported", "unverifiable"
    pub explanation: String,
    pub suggested_confidence: Option<String>,
}

/// Verify structural citation support for all outputs.
pub fn verify_structural(
    _request: &ResearchRequest,
    sources: &[SourceRecord],
    evidence: &[EvidenceSpan],
    claims: &[ClaimRecord],
    contradictions: &[ContradictionRecord],
) -> VerificationResult {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();

    let source_ids: HashSet<&str> = sources.iter().map(|s| s.id.as_str()).collect();
    let evidence_ids: HashSet<&str> = evidence.iter().map(|e| e.id.as_str()).collect();
    let claim_ids: HashSet<&str> = claims.iter().map(|c| c.id.as_str()).collect();

    // Rule 1: Every evidence record must point to an existing source
    for evid in evidence {
        if !source_ids.contains(evid.source_id.as_str()) {
            errors.push(format!(
                "Evidence {} references missing source {}",
                evid.id, evid.source_id
            ));
        }
    }

    // Rule 2: Every non-OpenQuestion claim's evidence_ids must exist
    for claim in claims {
        if claim.claim_type != ClaimType::OpenQuestion {
            for evid_id in &claim.evidence_ids {
                if !evidence_ids.contains(evid_id.as_str()) {
                    errors.push(format!(
                        "Claim {} references missing evidence {}",
                        claim.id, evid_id
                    ));
                }
            }
            if claim.evidence_ids.is_empty() {
                warnings.push(format!(
                    "Claim {} ({}) has no evidence references",
                    claim.id,
                    claim.claim_type.as_str()
                ));
            }
        }
    }

    // Rule 3: Every contradiction must reference existing claims
    for contra in contradictions {
        for claim_id in &contra.claim_ids {
            if !claim_ids.contains(claim_id.as_str()) {
                errors.push(format!(
                    "Contradiction {} references missing claim {}",
                    contra.id, claim_id
                ));
            }
        }
    }

    // Rule 4: Sources should not be empty (warning)
    if sources.is_empty() {
        warnings.push("No sources were collected".to_string());
    }

    // Rule 5: Claims should not be empty (warning)
    if claims.is_empty() {
        warnings.push("No claims were constructed".to_string());
    }

    // Rule 6: High-severity contradictions should be flagged
    for contra in contradictions {
        if contra.severity == ContradictionSeverity::High {
            warnings.push(format!(
                "High-severity contradiction {}: {}",
                contra.id, contra.description
            ));
        }
    }

    VerificationResult {
        passed: errors.is_empty(),
        errors,
        warnings,
    }
}

/// Semantic citation verification using the LLM.
///
/// For each non-OpenQuestion claim, checks whether the cited evidence actually
/// supports the claim text. Returns per-claim results with support status and
/// suggested confidence adjustments.
pub async fn verify_semantic(
    provider: &dyn Provider,
    model: &str,
    question: &str,
    claims: &[ClaimRecord],
    evidence: &[EvidenceSpan],
    sources: &[SourceRecord],
) -> Vec<SemanticCheckResult> {
    if claims.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    // Process claims in batches of 5 to avoid overwhelming the model
    for chunk in claims.chunks(5) {
        let batch_results =
            verify_claim_batch(provider, model, question, chunk, evidence, sources).await;
        results.extend(batch_results);
    }

    results
}

async fn verify_claim_batch(
    provider: &dyn Provider,
    model: &str,
    question: &str,
    claims: &[ClaimRecord],
    evidence: &[EvidenceSpan],
    sources: &[SourceRecord],
) -> Vec<SemanticCheckResult> {
    // Build compact context for each claim
    let claim_entries: Vec<serde_json::Value> = claims
        .iter()
        .map(|c| {
            let cited_evidence: Vec<serde_json::Value> = c
                .evidence_ids
                .iter()
                .filter_map(|eid| {
                    evidence.iter().find(|e| &e.id == eid).map(|e| {
                        let source = sources.iter().find(|s| s.id == e.source_id);
                        let src = source.and_then(|s| s.title.as_deref()).unwrap_or("unknown");
                        serde_json::json!({
                            "evidence_id": e.id,
                            "source": src,
                            "text": truncate_verify(&e.text, 800),
                        })
                    })
                })
                .collect();

            serde_json::json!({
                "claim_id": c.id,
                "claim_text": c.text,
                "claim_type": c.claim_type.as_str(),
                "confidence": format!("{:?}", c.confidence).to_lowercase(),
                "cited_evidence": cited_evidence,
            })
        })
        .collect();

    let claims_json = serde_json::to_string_pretty(&claim_entries).unwrap_or_default();

    let prompt = templates::VERIFICATION_PROMPT;
    let user_msg = format!(
        "Research question: {}\n\nClaims to verify:\n{}\n\n{}",
        question, claims_json, prompt
    );

    let json_val = match llm::call_llm_json(provider, model, None, &user_msg, Some(4096)).await {
        Ok(v) => v,
        Err(_) => {
            return claims
                .iter()
                .map(|c| SemanticCheckResult {
                    claim_id: c.id.clone(),
                    support_status: "unverifiable".to_string(),
                    explanation: "LLM verification call failed".to_string(),
                    suggested_confidence: None,
                })
                .collect();
        }
    };

    let items: Vec<serde_json::Value> = serde_json::from_value(json_val).unwrap_or_default();

    let mut results: Vec<SemanticCheckResult> = items
        .into_iter()
        .filter_map(|item| {
            Some(SemanticCheckResult {
                claim_id: item.get("claim_id")?.as_str()?.to_string(),
                support_status: item
                    .get("support_status")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unverifiable")
                    .to_string(),
                explanation: item
                    .get("explanation")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                suggested_confidence: item
                    .get("suggested_confidence")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            })
        })
        .collect();

    // Add entries for claims not covered by the model response
    for claim in claims {
        if !results.iter().any(|r| r.claim_id == claim.id) {
            results.push(SemanticCheckResult {
                claim_id: claim.id.clone(),
                support_status: "unverifiable".to_string(),
                explanation: "Not covered by model verification".to_string(),
                suggested_confidence: None,
            });
        }
    }

    results
}

fn truncate_verify(s: &str, max_chars: usize) -> String {
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

    fn make_request() -> ResearchRequest {
        ResearchRequest {
            id: "req-1".to_string(),
            question: "test".to_string(),
            mode: ResearchMode::NarrowAnswer,
            audience: ResearchAudience::Human,
            depth: ResearchDepth::Low,
            output_profiles: vec![],
            constraints: vec![],
            sources: vec![],
            existing_context_refs: vec![],
            budget: ResearchBudget {
                max_sources: 5,
                max_chunks_per_source: 5,
                max_evidence_spans: 10,
                max_model_calls: 0,
                max_output_tokens: None,
                allow_network: false,
            },
            created_at: Utc::now(),
        }
    }

    fn make_source(id: &str) -> SourceRecord {
        SourceRecord {
            id: id.to_string(),
            run_id: "req-1".to_string(),
            uri: "test.rs".to_string(),
            title: None,
            source_type: SourceType::LocalFile,
            source_quality: SourceQuality::SourceCode,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: None,
            locator: SourceLocator::TextSpan {
                label: "root".to_string(),
            },
            notes: vec![],
        }
    }

    fn make_evidence(id: &str, source_id: &str) -> EvidenceSpan {
        EvidenceSpan {
            id: id.to_string(),
            run_id: "req-1".to_string(),
            source_id: source_id.to_string(),
            locator: SourceLocator::TextSpan {
                label: "test".to_string(),
            },
            text: "evidence".to_string(),
            summary: None,
            extracted_at: Utc::now(),
        }
    }

    fn make_claim(id: &str, evidence_ids: Vec<String>, claim_type: ClaimType) -> ClaimRecord {
        ClaimRecord {
            id: id.to_string(),
            run_id: "req-1".to_string(),
            text: "claim".to_string(),
            claim_type,
            confidence: Confidence::Medium,
            evidence_ids,
            caveats: vec![],
            applies_to: vec![],
        }
    }

    #[test]
    fn verify_passes_with_valid_data() {
        let request = make_request();
        let sources = vec![make_source("s1")];
        let evidence = vec![make_evidence("e1", "s1")];
        let claims = vec![make_claim("c1", vec!["e1".to_string()], ClaimType::Fact)];
        let result = verify_structural(&request, &sources, &evidence, &claims, &[]);
        assert!(result.passed);
        assert!(result.errors.is_empty());
    }

    #[test]
    fn verify_rejects_missing_source() {
        let request = make_request();
        let sources = vec![];
        let evidence = vec![make_evidence("e1", "missing-source")];
        let claims = vec![];
        let result = verify_structural(&request, &sources, &evidence, &claims, &[]);
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("missing source")));
    }

    #[test]
    fn verify_rejects_missing_evidence_in_claim() {
        let request = make_request();
        let sources = vec![make_source("s1")];
        let evidence = vec![];
        let claims = vec![make_claim(
            "c1",
            vec!["missing-ev".to_string()],
            ClaimType::Fact,
        )];
        let result = verify_structural(&request, &sources, &evidence, &claims, &[]);
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("missing evidence")));
    }

    #[test]
    fn verify_allows_empty_evidence_for_open_question() {
        let request = make_request();
        let sources = vec![make_source("s1")];
        let evidence = vec![];
        let claims = vec![make_claim("c1", vec![], ClaimType::OpenQuestion)];
        let result = verify_structural(&request, &sources, &evidence, &claims, &[]);
        assert!(result.passed);
    }

    #[test]
    fn verify_rejects_missing_claim_in_contradiction() {
        let request = make_request();
        let sources = vec![make_source("s1")];
        let evidence = vec![make_evidence("e1", "s1")];
        let claims = vec![make_claim("c1", vec!["e1".to_string()], ClaimType::Fact)];
        let contras = vec![ContradictionRecord {
            id: "x1".to_string(),
            run_id: "req-1".to_string(),
            description: "conflict".to_string(),
            claim_ids: vec!["missing-claim".to_string()],
            severity: ContradictionSeverity::High,
        }];
        let result = verify_structural(&request, &sources, &evidence, &claims, &contras);
        assert!(!result.passed);
        assert!(result.errors.iter().any(|e| e.contains("missing claim")));
    }

    #[test]
    fn verify_warns_on_empty_sources() {
        let request = make_request();
        let result = verify_structural(&request, &[], &[], &[], &[]);
        assert!(result.warnings.iter().any(|w| w.contains("No sources")));
    }

    #[test]
    fn verify_warns_on_empty_claims() {
        let request = make_request();
        let sources = vec![make_source("s1")];
        let result = verify_structural(&request, &sources, &[], &[], &[]);
        assert!(result.warnings.iter().any(|w| w.contains("No claims")));
    }

    #[test]
    fn verify_warns_on_high_severity_contradiction() {
        let request = make_request();
        let sources = vec![make_source("s1")];
        let evidence = vec![make_evidence("e1", "s1")];
        let claims = vec![make_claim("c1", vec!["e1".to_string()], ClaimType::Fact)];
        let contras = vec![ContradictionRecord {
            id: "x1".to_string(),
            run_id: "req-1".to_string(),
            description: "major conflict".to_string(),
            claim_ids: vec!["c1".to_string()],
            severity: ContradictionSeverity::High,
        }];
        let result = verify_structural(&request, &sources, &evidence, &claims, &contras);
        assert!(result.warnings.iter().any(|w| w.contains("High-severity")));
    }
}
