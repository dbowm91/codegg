//! Host-side runtime contract for the `security_review` agent kind.
//!
//! The ordinary agent loop remains the owner of providers, tools, permissions,
//! cancellation, and scheduling.  This module only prepares bounded evidence
//! before the loop starts and provides conservative validation helpers for
//! structured model output.

use std::collections::BTreeSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::workflow::{
    run_security_review_workflow, SecurityReviewFinding, SecurityReviewOutput,
    SecurityReviewPrompt, SecurityReviewTarget, SecurityReviewWorkflowOptions,
};

pub const MAX_TARGETS: usize = 64;
pub const MAX_PREFLIGHT_RESULTS: usize = 32;
pub const MAX_EVIDENCE_PER_CHECK: usize = 24;
pub const MAX_TEXT: usize = 320;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityReviewInput {
    pub workspace_root: PathBuf,
    pub base: Option<String>,
    pub active_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityEvidenceRecord {
    pub check_name: String,
    pub status: String,
    pub file_path: Option<PathBuf>,
    pub line: Option<u32>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityEvidenceGap {
    pub source: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityEvidenceBundle {
    pub scope: SecurityReviewInput,
    pub targets: Vec<SecurityReviewTarget>,
    pub evidence: Vec<SecurityEvidenceRecord>,
    pub evidence_gaps: Vec<SecurityEvidenceGap>,
    pub coverage: Vec<String>,
    pub fingerprint: String,
}

impl SecurityEvidenceBundle {
    pub fn prompt_context(&self) -> String {
        let mut out = String::from("\n\n## Host-prepared security review evidence\n".to_string());
        out.push_str(&format!("- Bundle fingerprint: {}\n", self.fingerprint));
        out.push_str(&format!("- Targets examined: {}\n", self.targets.len()));
        out.push_str("- Risk markers and diagnostics are review prompts, not findings.\n");
        if !self.coverage.is_empty() {
            out.push_str(&format!("- Coverage: {}\n", self.coverage.join(", ")));
        }
        for record in self.evidence.iter().take(MAX_EVIDENCE_PER_CHECK) {
            out.push_str(&format!(
                "- [{}] {}{}: {}\n",
                record.status,
                record.check_name,
                record
                    .file_path
                    .as_ref()
                    .map(|p| format!(" ({})", p.display()))
                    .unwrap_or_default(),
                record.summary
            ));
        }
        for gap in &self.evidence_gaps {
            out.push_str(&format!(
                "- Evidence gap ({}): {}\n",
                gap.source, gap.reason
            ));
        }
        out.push_str(
            "\nReport only evidence-backed findings. Keep marker-only items in review_prompts.\n",
        );
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SecurityReviewReport {
    pub findings: Vec<SecurityReviewFinding>,
    pub review_prompts: Vec<SecurityReviewPrompt>,
    pub evidence_gaps: Vec<SecurityEvidenceGap>,
    pub coverage: Vec<String>,
    pub overall_confidence: String,
}

/// Provider-neutral schema requested for security-review synthesis. Local
/// validation remains authoritative because providers may ignore structured
/// output requests.
pub fn report_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["findings", "review_prompts", "evidence_gaps", "coverage", "overall_confidence"],
        "properties": {
            "findings": {"type": "array", "items": {"type": "object"}},
            "review_prompts": {"type": "array", "items": {"type": "object"}},
            "evidence_gaps": {"type": "array", "items": {"type": "object"}},
            "coverage": {"type": "array", "items": {"type": "string"}},
            "overall_confidence": {"type": "string"}
        }
    })
}

/// Prepare the deterministic, read-only stage of a security-review turn.
pub async fn prepare_security_review(
    input: SecurityReviewInput,
) -> Result<(SecurityEvidenceBundle, SecurityReviewOutput), String> {
    let options = SecurityReviewWorkflowOptions {
        max_findings: 50,
        max_prompts: 100,
        enable_lsp_enrichment: false,
        enable_hunk_source_context: false,
        ..Default::default()
    };
    let (mut output, _) =
        run_security_review_workflow(&input.workspace_root, input.base.as_deref(), options, None)
            .await?;

    output.targets.truncate(MAX_TARGETS);
    let mut evidence = Vec::new();
    for result in output.preflight_results.iter().take(MAX_PREFLIGHT_RESULTS) {
        for item in result
            .structured_evidence
            .iter()
            .take(MAX_EVIDENCE_PER_CHECK)
        {
            evidence.push(SecurityEvidenceRecord {
                check_name: result.check_name.clone(),
                status: format!("{:?}", result.status).to_lowercase(),
                file_path: Some(item.file_path.clone()),
                line: item.line,
                summary: bounded(&item.summary),
            });
        }
        if result.structured_evidence.is_empty() {
            evidence.push(SecurityEvidenceRecord {
                check_name: result.check_name.clone(),
                status: format!("{:?}", result.status).to_lowercase(),
                file_path: None,
                line: None,
                summary: bounded(
                    result
                        .notes
                        .first()
                        .map(String::as_str)
                        .unwrap_or("check completed"),
                ),
            });
        }
    }

    let mut gaps = Vec::new();
    if output.hunks.is_empty() && output.targets.is_empty() {
        gaps.push(SecurityEvidenceGap {
            source: "scope".to_string(),
            reason: "no changed files or hunks were available".to_string(),
        });
    }
    if output
        .notes
        .iter()
        .any(|note| note.contains("unavailable") || note.contains("failed"))
    {
        gaps.push(SecurityEvidenceGap {
            source: "optional-evidence".to_string(),
            reason: "one or more optional evidence collectors were unavailable".to_string(),
        });
    }

    let coverage = vec![
        "changed diff targets".to_string(),
        "filename and bounded content preflight".to_string(),
        "conservative marker-versus-finding synthesis".to_string(),
    ];
    let fingerprint = fingerprint(&input, &output, &evidence, &gaps);
    Ok((
        SecurityEvidenceBundle {
            scope: input,
            targets: output.targets.clone(),
            evidence,
            evidence_gaps: gaps,
            coverage,
            fingerprint,
        },
        output,
    ))
}

/// Validate a model-produced report against host evidence.  A finding must
/// point at a target file and carry at least one evidence-bearing location.
/// Invalid findings are returned as review prompts by the caller rather than
/// being treated as confirmed vulnerabilities.
pub fn validate_report(
    mut report: SecurityReviewReport,
    bundle: &SecurityEvidenceBundle,
) -> (SecurityReviewReport, Vec<String>) {
    let allowed: BTreeSet<PathBuf> = bundle.targets.iter().map(|t| t.file_path.clone()).collect();
    let mut accepted = Vec::new();
    let mut rejected = Vec::new();
    for finding in std::mem::take(&mut report.findings) {
        let has_location = finding.evidence.iter().any(|e| e.file_path.is_some());
        if allowed.contains(&finding.file_path)
            && has_location
            && !finding.reasoning.trim().is_empty()
        {
            accepted.push(finding);
        } else {
            rejected.push(format!(
                "rejected unsupported finding at {}",
                finding.file_path.display()
            ));
        }
    }
    let mut result = report;
    result.findings = accepted;
    result
        .evidence_gaps
        .extend(rejected.iter().cloned().map(|reason| SecurityEvidenceGap {
            source: "model-output".to_string(),
            reason,
        }));
    (result, rejected)
}

fn bounded(value: &str) -> String {
    value.chars().take(MAX_TEXT).collect()
}

fn fingerprint(
    input: &SecurityReviewInput,
    output: &SecurityReviewOutput,
    evidence: &[SecurityEvidenceRecord],
    gaps: &[SecurityEvidenceGap],
) -> String {
    let payload = serde_json::to_vec(&(input, &output.targets, &output.hunks, evidence, gaps))
        .unwrap_or_default();
    let digest = Sha256::digest(payload);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_context_is_bounded_and_does_not_include_details() {
        let bundle = SecurityEvidenceBundle {
            scope: SecurityReviewInput {
                workspace_root: PathBuf::from("/repo"),
                base: None,
                active_file: None,
            },
            targets: Vec::new(),
            evidence: vec![SecurityEvidenceRecord {
                check_name: "secret_content_scan".into(),
                status: "warn".into(),
                file_path: Some(PathBuf::from("src/lib.rs")),
                line: Some(4),
                summary: "secret-like assignment".into(),
            }],
            evidence_gaps: vec![],
            coverage: vec!["preflight".into()],
            fingerprint: "abc".into(),
        };
        let context = bundle.prompt_context();
        assert!(context.contains("secret-like assignment"));
        assert!(!context.contains("detail"));
        assert!(context.contains("review_prompts"));
    }

    #[test]
    fn fingerprint_is_stable_for_same_input() {
        let input = SecurityReviewInput {
            workspace_root: PathBuf::from("/repo"),
            base: Some("HEAD~1".into()),
            active_file: None,
        };
        let output = SecurityReviewOutput {
            targets: vec![],
            findings: vec![],
            review_prompts: vec![],
            preflight_results: vec![],
            notes: vec![],
            hunks: vec![],
        };
        let a = fingerprint(&input, &output, &[], &[]);
        let b = fingerprint(&input, &output, &[], &[]);
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
    }
}
