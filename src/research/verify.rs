use std::collections::HashSet;

use crate::research::types::*;

#[derive(Debug)]
pub struct VerificationResult {
    pub passed: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
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

    fn make_claim(
        id: &str,
        evidence_ids: Vec<String>,
        claim_type: ClaimType,
    ) -> ClaimRecord {
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
        let claims = vec![make_claim(
            "c1",
            vec!["e1".to_string()],
            ClaimType::Fact,
        )];
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
        assert!(result
            .errors
            .iter()
            .any(|e| e.contains("missing evidence")));
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
        let claims = vec![make_claim(
            "c1",
            vec!["e1".to_string()],
            ClaimType::Fact,
        )];
        let contras = vec![ContradictionRecord {
            id: "x1".to_string(),
            run_id: "req-1".to_string(),
            description: "conflict".to_string(),
            claim_ids: vec!["missing-claim".to_string()],
            severity: ContradictionSeverity::High,
        }];
        let result =
            verify_structural(&request, &sources, &evidence, &claims, &contras);
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
        let claims = vec![make_claim(
            "c1",
            vec!["e1".to_string()],
            ClaimType::Fact,
        )];
        let contras = vec![ContradictionRecord {
            id: "x1".to_string(),
            run_id: "req-1".to_string(),
            description: "major conflict".to_string(),
            claim_ids: vec!["c1".to_string()],
            severity: ContradictionSeverity::High,
        }];
        let result =
            verify_structural(&request, &sources, &evidence, &claims, &contras);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("High-severity")));
    }
}
