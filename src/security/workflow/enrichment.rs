use serde::{Deserialize, Serialize};
use std::time::Duration;

use super::context::*;
use super::evidence::*;
use super::report::*;
use super::types::*;

/// Result of a single LSP securityContext enrichment pass for one target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityContextEnrichmentResult {
    /// The target that was enriched.
    pub target: SecurityReviewTarget,
    /// Escalation level used.
    pub level: SecurityContextEscalationLevel,
    /// The request payload sent.
    pub request: serde_json::Value,
    /// Response from the executor (None on failure).
    pub response: Option<serde_json::Value>,
    /// Prompts derived from risk markers in the response.
    pub prompts: Vec<SecurityReviewPrompt>,
    /// Structured evidence derived from the response.
    pub evidence: Vec<StructuredSecurityEvidence>,
    /// Notes about failures, truncation, or timeouts.
    pub notes: Vec<String>,
}

/// Run the LSP securityContext enrichment pass.
///
/// Takes the deterministic stage-1 output and escalation plans, executes
/// bounded securityContext requests through the executor, and converts
/// responses into enriched prompts and structured evidence.
///
/// Failures and timeouts are recorded as notes — they never fail the
/// whole review.
pub async fn run_security_context_enrichment<E: SecurityContextExecutor>(
    output: &SecurityReviewOutput,
    executor: &E,
    options: &SecurityReviewWorkflowOptions,
) -> Vec<SecurityContextEnrichmentResult> {
    let plans = plan_security_context_escalations(output);

    // Filter to non-None plans
    let mut eligible: Vec<SecurityContextEscalationPlan> = plans
        .into_iter()
        .filter(|p| p.level != SecurityContextEscalationLevel::None)
        .collect();

    // Sort by priority: CallDepth2 first, then CallDepth1, then Basic
    eligible.sort_by_key(|b| std::cmp::Reverse(b.level));

    // Apply caps
    eligible.truncate(options.max_lsp_enriched_targets);
    let max_requests = options.max_lsp_requests;
    let timeout = Duration::from_millis(options.lsp_request_timeout_ms);

    let mut results = Vec::new();
    let mut request_count = 0usize;

    for plan in eligible {
        if request_count >= max_requests {
            break;
        }

        let request = match &plan.request {
            Some(req) => req.clone(),
            None => continue,
        };

        let response =
            match tokio::time::timeout(timeout, executor.security_context(request.clone())).await {
                Ok(Ok(val)) => Some(val),
                Ok(Err(e)) => {
                    // Executor returned an error
                    let result = SecurityContextEnrichmentResult {
                        target: plan.target.clone(),
                        level: plan.level,
                        request,
                        response: None,
                        prompts: Vec::new(),
                        evidence: Vec::new(),
                        notes: vec![format!("executor error: {}", e)],
                    };
                    results.push(result);
                    request_count += 1;
                    continue;
                }
                Err(_) => {
                    // Timeout
                    let result = SecurityContextEnrichmentResult {
                        target: plan.target.clone(),
                        level: plan.level,
                        request,
                        response: None,
                        prompts: Vec::new(),
                        evidence: Vec::new(),
                        notes: vec![format!(
                            "securityContext request timed out after {}ms",
                            options.lsp_request_timeout_ms
                        )],
                    };
                    results.push(result);
                    request_count += 1;
                    continue;
                }
            };

        request_count += 1;

        let resp_ref = response.as_ref().unwrap();
        let prompts = prompts_from_security_context(&plan.target, resp_ref);
        let enriched_evidence = evidence_from_security_context(&plan.target, resp_ref);

        let mut notes = Vec::new();
        let truncated = resp_ref
            .get("truncated")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if truncated {
            notes.push("securityContext response was truncated".to_string());
        }

        results.push(SecurityContextEnrichmentResult {
            target: plan.target,
            level: plan.level,
            request,
            response,
            prompts,
            evidence: enriched_evidence,
            notes,
        });
    }

    results
}

/// Merge enrichment results into stage-1 output, producing enriched
/// prompts and extra evidence for a second synthesis pass.
pub fn merge_enrichment_results(
    output: &SecurityReviewOutput,
    enrichment_results: &[SecurityContextEnrichmentResult],
) -> (
    Vec<SecurityReviewPrompt>,
    Vec<StructuredSecurityEvidence>,
    Vec<String>,
) {
    let mut enriched_prompts: Vec<SecurityReviewPrompt> = Vec::new();
    let mut extra_evidence: Vec<StructuredSecurityEvidence> = Vec::new();
    let mut enrichment_notes: Vec<String> = Vec::new();

    for result in enrichment_results {
        enriched_prompts.extend(result.prompts.clone());
        extra_evidence.extend(result.evidence.clone());
        enrichment_notes.extend(result.notes.clone());
    }

    // Merge enriched prompts with original prompts (avoid duplicates by
    // deduplicating on (file_path, line, title))
    let mut all_prompts = output.review_prompts.clone();
    for ep in &enriched_prompts {
        let is_dup = all_prompts
            .iter()
            .any(|p| p.file_path == ep.file_path && p.line == ep.line && p.title == ep.title);
        if !is_dup {
            all_prompts.push(ep.clone());
        }
    }

    // Sort prompts for deterministic output
    all_prompts.sort_by(|a, b| {
        a.file_path
            .cmp(&b.file_path)
            .then(a.line.cmp(&b.line))
            .then(a.title.cmp(&b.title))
    });

    (all_prompts, extra_evidence, enrichment_notes)
}
