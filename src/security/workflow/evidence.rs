use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::types::*;

// ---------------------------------------------------------------------------
// Evidence-based finding synthesis
// ---------------------------------------------------------------------------

/// Compute the start of a 5-line bucket containing `line`.
pub(crate) fn line_bucket_start(line: u32) -> u32 {
    line / 5 * 5
}

/// Check whether `evidence_line` falls within the group window.
/// The group spans `[group_bucket, group_bucket + bucket_width - 1]`
/// with an additional `radius` on each side.
pub(crate) fn line_within_group_window(
    evidence_line: u32,
    group_bucket: u32,
    bucket_width: u32,
    radius: u32,
) -> bool {
    let bucket_end = group_bucket + bucket_width.saturating_sub(1);
    let window_start = group_bucket.saturating_sub(radius);
    let window_end = bucket_end.saturating_add(radius);
    evidence_line >= window_start && evidence_line <= window_end
}

pub(crate) fn evidence_matches_group(
    evidence: &StructuredSecurityEvidence,
    file_path: &Path,
    line_bucket: Option<u32>,
) -> bool {
    let Some(ef) = &evidence.file_path else {
        return false;
    };
    if ef != file_path {
        return false;
    }
    match (line_bucket, evidence.line) {
        (Some(bucket), Some(line)) => line_within_group_window(line, bucket, 5, 5),
        (Some(_), None) => true,
        (None, _) => true,
    }
}

/// Explicit gate: marker-only evidence is never finding-eligible.
pub fn marker_only_is_finding_eligible(_evidence: &[StructuredSecurityEvidence]) -> bool {
    false
}

/// Determine whether a set of structured evidence is eligible to produce
/// a finding.  Requires at least two meaningful evidence dimensions or
/// explicit code reasoning.
pub fn is_finding_eligible(evidence: &[StructuredSecurityEvidence]) -> bool {
    let has_marker = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::RiskMarker);
    let has_changed_hunk = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::ChangedHunk);
    let has_preflight_fail = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::Preflight);
    let has_call_path = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::CallPath);
    let has_diagnostic = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::Diagnostic);
    let has_reasoning = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::CodeReasoning);

    (has_marker
        && (has_changed_hunk
            || has_preflight_fail
            || has_call_path
            || has_diagnostic
            || has_reasoning))
        || (has_preflight_fail && has_changed_hunk)
        || (has_reasoning && has_changed_hunk)
}

/// Convert a [`SecurityReviewTarget`] into structured evidence.
pub fn evidence_from_target(target: &SecurityReviewTarget) -> StructuredSecurityEvidence {
    let kind = match target.reason {
        SecurityTargetReason::ChangedHunk => SecurityEvidenceKind::ChangedHunk,
        SecurityTargetReason::UnsafeCode
        | SecurityTargetReason::ProcessExecution
        | SecurityTargetReason::NetworkBoundary
        | SecurityTargetReason::AuthOrSecretHandling => SecurityEvidenceKind::CodeReasoning,
        _ => SecurityEvidenceKind::CodeReasoning,
    };
    StructuredSecurityEvidence {
        kind,
        file_path: Some(target.file_path.clone()),
        line: target.line,
        summary: format!("target {:?} preset={}", target.reason, target.preset),
        detail: None,
    }
}

/// Convert a [`SecurityReviewPrompt`] into structured evidence.
///
/// Prompts from `source: securityContext.risk_marker` emit `RiskMarker`
/// evidence.  Prompts from `source: changed_hunk` emit `ChangedHunk`
/// evidence.
pub fn evidence_from_review_prompt(
    prompt: &SecurityReviewPrompt,
) -> Vec<StructuredSecurityEvidence> {
    let mut evidence = Vec::new();

    let has_marker_source = prompt
        .evidence
        .iter()
        .any(|e| e == "source: securityContext.risk_marker");
    let has_hunk_source = prompt.evidence.iter().any(|e| e == "source: changed_hunk");

    if has_marker_source {
        evidence.push(StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::RiskMarker,
            file_path: Some(prompt.file_path.clone()),
            line: prompt.line,
            summary: prompt.title.clone(),
            detail: Some(prompt.rationale.clone()),
        });
    } else if has_hunk_source {
        evidence.push(StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::ChangedHunk,
            file_path: Some(prompt.file_path.clone()),
            line: prompt.line,
            summary: prompt.title.clone(),
            detail: Some(prompt.rationale.clone()),
        });
    }

    let truncated = prompt.evidence.iter().any(|e| e.contains("truncated"));
    if truncated {
        evidence.push(StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::TruncationNotice,
            file_path: Some(prompt.file_path.clone()),
            line: prompt.line,
            summary: "context was truncated".to_string(),
            detail: None,
        });
    }

    evidence
}

// ---------------------------------------------------------------------------
// Evidence-based finding synthesis
// ---------------------------------------------------------------------------

/// Synthesize evidence-based findings from targets, review prompts, and
/// preflight results.  Groups evidence by file and nearby line, applies the
/// eligibility gate, and emits findings only for eligible groups.  Ineligible
/// prompts are preserved as review prompts.
pub fn synthesize_evidence_based_findings(
    targets: &[SecurityReviewTarget],
    prompts: &[SecurityReviewPrompt],
    preflight: &[SecurityPreflightResult],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>) {
    let mut findings = Vec::new();
    let remaining_prompts: Vec<SecurityReviewPrompt> = prompts.to_vec();

    // Collect structured preflight evidence (file-scoped)
    let preflight_evidence: Vec<StructuredSecurityEvidence> = preflight
        .iter()
        .filter(|p| p.status == PreflightStatus::Fail)
        .flat_map(|p| {
            if !p.structured_evidence.is_empty() {
                p.structured_evidence
                    .iter()
                    .map(move |se| StructuredSecurityEvidence {
                        kind: SecurityEvidenceKind::Preflight,
                        file_path: Some(se.file_path.clone()),
                        line: se.line,
                        summary: format!("{}: {}", p.check_name, se.summary),
                        detail: se.detail.clone().or_else(|| Some(p.notes.join("; "))),
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        })
        .collect();

    // Group prompts by file path and line bucket
    #[derive(Hash, PartialEq, Eq, Clone)]
    struct GroupKey {
        file_path: PathBuf,
        line_bucket: Option<u32>,
    }

    let mut groups: std::collections::HashMap<GroupKey, Vec<usize>> =
        std::collections::HashMap::new();

    for (idx, prompt) in remaining_prompts.iter().enumerate() {
        let line_bucket = prompt.line.map(line_bucket_start);
        let key = GroupKey {
            file_path: prompt.file_path.clone(),
            line_bucket,
        };
        groups.entry(key).or_default().push(idx);
    }

    // Process each group
    let mut indices_to_remove: HashSet<usize> = HashSet::new();

    for (key, indices) in &groups {
        let mut group_evidence: Vec<StructuredSecurityEvidence> = Vec::new();

        // Add target evidence for this file
        for target in targets {
            if target.file_path == key.file_path {
                group_evidence.push(evidence_from_target(target));
            }
        }

        // Add preflight evidence for this file
        for pe in &preflight_evidence {
            if evidence_matches_group(pe, &key.file_path, key.line_bucket) {
                group_evidence.push(pe.clone());
            }
        }

        // Add prompt evidence
        for &idx in indices {
            let prompt_evidence = evidence_from_review_prompt(&remaining_prompts[idx]);
            group_evidence.extend(prompt_evidence);
        }

        // Check if truncated
        let truncated = group_evidence
            .iter()
            .any(|e| e.kind == SecurityEvidenceKind::TruncationNotice);

        if is_finding_eligible(&group_evidence) {
            let category = remaining_prompts[indices[0]].category.clone();
            let (severity, confidence) =
                classify_finding(category.as_deref(), &group_evidence, truncated);
            let title = finding_title(category.as_deref(), &group_evidence);
            let reasoning = finding_reasoning(&group_evidence);
            let recommendation = finding_recommendation(category.as_deref());
            let tests = finding_tests(category.as_deref());

            findings.push(SecurityReviewFinding {
                severity,
                confidence,
                title,
                file_path: key.file_path.clone(),
                line: remaining_prompts[indices[0]].line,
                category,
                evidence: group_evidence,
                reasoning,
                recommendation,
                tests,
            });

            for &idx in indices {
                indices_to_remove.insert(idx);
            }
        }
    }

    // Remove prompts that became findings
    let remaining: Vec<SecurityReviewPrompt> = remaining_prompts
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !indices_to_remove.contains(i))
        .map(|(_, p)| p)
        .collect();

    (findings, remaining)
}

// ---------------------------------------------------------------------------
// Enriched context to structured evidence
// ---------------------------------------------------------------------------

/// Convert `securityContext` JSON response into structured evidence.
///
/// Extracts risk markers, diagnostics, call graph summaries, and
/// truncation notices. Evidence is always file-scoped and compact —
/// no large raw JSON payloads are included.
pub fn evidence_from_security_context(
    target: &SecurityReviewTarget,
    context_json: &serde_json::Value,
) -> Vec<StructuredSecurityEvidence> {
    let mut evidence = Vec::new();

    // Risk markers
    if let Some(serde_json::Value::Array(markers)) = context_json.get("risk_markers") {
        for marker in markers {
            let category = marker
                .get("category")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let label = marker
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let matched_text = marker
                .get("matched_text")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let file_path = marker
                .get("file")
                .or_else(|| marker.get("file_path"))
                .and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| target.file_path.clone());

            let line = marker
                .get("line")
                .and_then(|v| v.as_u64())
                .map(|l| l as u32)
                .or(target.line);

            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::RiskMarker,
                file_path: Some(file_path),
                line,
                summary: format!("{}: {}", category, label),
                detail: Some(matched_text.to_string()),
            });
        }
    }

    // Diagnostics
    if let Some(serde_json::Value::Array(diags)) = context_json.get("security_relevant_diagnostics")
    {
        for diag in diags {
            let message = diag
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("diagnostic");
            let line = diag
                .get("line")
                .and_then(|v| v.as_u64())
                .map(|l| l as u32)
                .or(target.line);

            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::Diagnostic,
                file_path: Some(target.file_path.clone()),
                line,
                summary: format!("LSP diagnostic: {}", message),
                detail: None,
            });
        }
    }

    // Call expansion / call graph summary
    if let Some(call_expansion) = context_json.get("call_expansion") {
        let node_count = call_expansion
            .get("nodes")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let edge_count = call_expansion
            .get("edges")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        let depth = call_expansion
            .get("depth")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        if node_count > 0 || edge_count > 0 {
            evidence.push(StructuredSecurityEvidence {
                kind: SecurityEvidenceKind::CallPath,
                file_path: Some(target.file_path.clone()),
                line: target.line,
                summary: format!(
                    "securityContext call expansion returned {} nodes and {} edges at depth {}",
                    node_count, edge_count, depth
                ),
                detail: None,
            });
        }
    }

    // Truncation notice
    let truncated = context_json
        .get("truncated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let limits_truncated = context_json
        .get("limits")
        .and_then(|l| l.get("call_expansion_truncated"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if truncated || limits_truncated {
        evidence.push(StructuredSecurityEvidence {
            kind: SecurityEvidenceKind::TruncationNotice,
            file_path: Some(target.file_path.clone()),
            line: target.line,
            summary: "securityContext response was truncated".to_string(),
            detail: None,
        });
    }

    evidence
}

// ---------------------------------------------------------------------------
// Enriched finding synthesis (with extra evidence)
// ---------------------------------------------------------------------------

/// Synthesize findings with additional enriched evidence from LSP
/// securityContext responses.
///
/// Combines the original targets, prompts, and preflight results with
/// extra evidence from the enrichment pass. The eligibility gate and
/// classification logic remain identical to the base synthesis.
pub fn synthesize_evidence_based_findings_with_extra_evidence(
    targets: &[SecurityReviewTarget],
    prompts: &[SecurityReviewPrompt],
    preflight: &[SecurityPreflightResult],
    extra_evidence: &[StructuredSecurityEvidence],
) -> (Vec<SecurityReviewFinding>, Vec<SecurityReviewPrompt>) {
    // Combine base prompts with any enriched prompts
    let mut all_prompts = prompts.to_vec();

    // Convert extra evidence into prompts for synthesis grouping
    for ev in extra_evidence {
        if ev.kind == SecurityEvidenceKind::RiskMarker {
            let file_path = ev
                .file_path
                .clone()
                .unwrap_or_else(|| target_file_path(targets, &ev.file_path));
            all_prompts.push(SecurityReviewPrompt {
                file_path,
                line: ev.line,
                preset: String::new(),
                category: ev.summary.split(':').next().map(|s| s.to_string()),
                title: ev.summary.clone(),
                rationale: ev.detail.clone().unwrap_or_default(),
                evidence: vec![
                    "source: securityContext.risk_marker".to_string(),
                    ev.summary.clone(),
                ],
            });
        }
    }

    let (mut findings, remaining_prompts) =
        synthesize_evidence_based_findings(targets, &all_prompts, preflight);

    // For findings that were created, inject any matching extra evidence
    // (CallPath, Diagnostic, TruncationNotice) into their evidence lists
    // and re-classify
    for finding in &mut findings {
        let matching_extra: Vec<StructuredSecurityEvidence> = extra_evidence
            .iter()
            .filter(|ev| {
                ev.file_path.as_deref() == Some(&finding.file_path)
                    && matches!(
                        ev.kind,
                        SecurityEvidenceKind::CallPath
                            | SecurityEvidenceKind::Diagnostic
                            | SecurityEvidenceKind::TruncationNotice
                    )
            })
            .cloned()
            .collect();

        if !matching_extra.is_empty() {
            finding.evidence.extend(matching_extra);
            let truncated = finding
                .evidence
                .iter()
                .any(|e| e.kind == SecurityEvidenceKind::TruncationNotice);
            let (severity, confidence) =
                classify_finding(finding.category.as_deref(), &finding.evidence, truncated);
            finding.severity = severity;
            finding.confidence = confidence;
        }
    }

    (findings, remaining_prompts)
}

fn target_file_path(targets: &[SecurityReviewTarget], fallback: &Option<PathBuf>) -> PathBuf {
    targets
        .first()
        .map(|t| t.file_path.clone())
        .or_else(|| fallback.clone())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Severity and confidence classification
// ---------------------------------------------------------------------------

/// Deterministically classify the severity and confidence of a finding
/// based on its category, evidence, and truncation state.
pub fn classify_finding(
    category: Option<&str>,
    evidence: &[StructuredSecurityEvidence],
    truncated: bool,
) -> (SecuritySeverity, SecurityConfidence) {
    let has_marker = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::RiskMarker);
    let has_changed_hunk = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::ChangedHunk);
    let has_preflight = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::Preflight);
    let has_call_path = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::CallPath);
    let has_diagnostic = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::Diagnostic);
    let has_reasoning = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::CodeReasoning);

    // Base severity from category
    let base_severity = match category {
        Some("auth") | Some("secret") | Some("crypto") => SecuritySeverity::Medium,
        Some("unsafe") | Some("process") | Some("sql") => SecuritySeverity::Medium,
        Some("filesystem") | Some("path") => SecuritySeverity::Medium,
        _ => SecuritySeverity::Low,
    };

    // Adjust severity based on evidence strength
    let severity = if has_call_path && has_reasoning {
        // Call path + reasoning pushes toward higher severity
        match base_severity {
            SecuritySeverity::Low => SecuritySeverity::Medium,
            other => other,
        }
    } else if has_preflight && has_changed_hunk {
        // Content preflight + changed hunk is meaningful
        match base_severity {
            SecuritySeverity::Low => SecuritySeverity::Medium,
            other => other,
        }
    } else {
        base_severity
    };

    // Base confidence
    let mut confidence = if has_preflight && has_changed_hunk && has_marker {
        SecurityConfidence::High
    } else if has_marker && (has_changed_hunk || has_call_path || has_diagnostic || has_reasoning) {
        SecurityConfidence::Medium
    } else {
        SecurityConfidence::Low
    };

    // Truncation reduces confidence by one level
    if truncated && confidence != SecurityConfidence::Low {
        confidence = match confidence {
            SecurityConfidence::High => SecurityConfidence::Medium,
            SecurityConfidence::Medium => SecurityConfidence::Low,
            SecurityConfidence::Low => SecurityConfidence::Low,
        };
    }

    // No Critical by default in this pass
    let severity = match severity {
        SecuritySeverity::Critical => SecuritySeverity::High,
        other => other,
    };

    (severity, confidence)
}

// ---------------------------------------------------------------------------
// Finding text generation
// ---------------------------------------------------------------------------

/// Generate a finding title from category and evidence.
pub(crate) fn finding_title(
    category: Option<&str>,
    evidence: &[StructuredSecurityEvidence],
) -> String {
    let cat = category.unwrap_or("unknown");
    let has_marker = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::RiskMarker);
    let has_preflight = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::Preflight);

    if has_preflight {
        format!("Evidence-based finding: {} (preflight confirmed)", cat)
    } else if has_marker {
        format!(
            "Evidence-based finding: {} (marker + supporting evidence)",
            cat
        )
    } else {
        format!("Evidence-based finding: {}", cat)
    }
}

/// Generate reasoning text from evidence.
pub(crate) fn finding_reasoning(evidence: &[StructuredSecurityEvidence]) -> String {
    let mut parts = Vec::new();

    let marker_count = evidence
        .iter()
        .filter(|e| e.kind == SecurityEvidenceKind::RiskMarker)
        .count();
    if marker_count > 0 {
        parts.push(format!("{} risk marker(s) present", marker_count));
    }

    let hunk_count = evidence
        .iter()
        .filter(|e| e.kind == SecurityEvidenceKind::ChangedHunk)
        .count();
    if hunk_count > 0 {
        parts.push(format!("{} changed hunk(s) in scope", hunk_count));
    }

    let preflight_count = evidence
        .iter()
        .filter(|e| e.kind == SecurityEvidenceKind::Preflight)
        .count();
    if preflight_count > 0 {
        parts.push(format!("{} content preflight(s) failed", preflight_count));
    }

    if evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::CallPath)
    {
        parts.push("reachable from public/auth boundary".to_string());
    }

    if evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::Diagnostic)
    {
        parts.push("LSP diagnostics support concern".to_string());
    }

    if evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::CodeReasoning)
    {
        parts.push("code analysis indicates potential risk".to_string());
    }

    let truncated = evidence
        .iter()
        .any(|e| e.kind == SecurityEvidenceKind::TruncationNotice);
    if truncated {
        parts.push("context was truncated, confidence reduced".to_string());
    }

    if parts.is_empty() {
        "insufficient evidence for detailed reasoning".to_string()
    } else {
        parts.join("; ")
    }
}

/// Generate a defensive recommendation for a category.
pub(crate) fn finding_recommendation(category: Option<&str>) -> String {
    match category {
        Some("auth") => {
            "Add validation and negative tests; enforce issuer/audience/expiry checks; \
             never trust client-supplied tokens without verification"
                .to_string()
        }
        Some("secret") => {
            "Remove hardcoded secret; load from secret store or environment variable; \
             add secret-scan regression test"
                .to_string()
        }
        Some("unsafe") => "Document invariant and safety requirements; add boundary checks; \
             reduce unsafe scope to minimum necessary"
            .to_string(),
        Some("process") => "Avoid shell interpolation; pass arguments separately to Command; \
             add test for malicious input handling"
            .to_string(),
        Some("filesystem") | Some("path") => "Canonicalize paths and enforce root directory; \
             add traversal tests for edge cases"
            .to_string(),
        Some("sql") => "Use parameterized queries; never construct SQL with format!(); \
             add injection regression test"
            .to_string(),
        Some("crypto") => "Use modern cryptographic primitives; avoid weak hash algorithms, \
             ECB mode, and hardcoded keys"
            .to_string(),
        _ => "Review for security implications; add appropriate defensive tests; \
             follow principle of least privilege"
            .to_string(),
    }
}

/// Generate suggested test names for a category.
pub(crate) fn finding_tests(category: Option<&str>) -> Vec<String> {
    match category {
        Some("auth") => vec![
            "test_auth_rejects_invalid_token".to_string(),
            "test_auth_enforces_issuer_check".to_string(),
            "test_auth_enforces_expiry_check".to_string(),
        ],
        Some("secret") => vec![
            "test_no_hardcoded_secrets_in_source".to_string(),
            "test_secret_loaded_from_env".to_string(),
        ],
        Some("unsafe") => vec![
            "test_unsafe_invariant_documented".to_string(),
            "test_unsafe_boundary_checks".to_string(),
        ],
        Some("process") => vec![
            "test_command_no_shell_interpolation".to_string(),
            "test_command_handles_malicious_args".to_string(),
        ],
        Some("filesystem") | Some("path") => vec![
            "test_path_canonicalized_within_root".to_string(),
            "test_path_rejects_traversal".to_string(),
        ],
        Some("sql") => vec![
            "test_query_uses_parameterized_statement".to_string(),
            "test_sql_injection_rejected".to_string(),
        ],
        Some("crypto") => vec![
            "test_uses_modern_crypto_primitive".to_string(),
            "test_no_hardcoded_key".to_string(),
        ],
        _ => vec!["test_security_review_defensive".to_string()],
    }
}
