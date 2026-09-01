//! Host-owned finalization for specialized agent runtimes.
//!
//! This module deliberately does not stream providers, execute tools, or
//! admit work.  It prepares bounded evidence, coordinates bounded evidence
//! children through the existing sub-agent pool, and validates the ordinary
//! loop's public output before completion is published.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use crate::agent::r#loop::AgentLoopTerminalOutput;
use crate::agent::worker::{SubAgentPool, SubAgentRequest, SubAgentResult};
use crate::error::AppError;
use crate::research::runtime::{
    self, BoundedResearchPlan, EvidenceRecord, ResearchEvidenceReport, ResearchReport,
};
use crate::security::runtime::{
    self as security_runtime, SecurityEvidenceBundle, SecurityReviewReport,
};

pub const MAX_CHILD_TIMEOUT: Duration = Duration::from_secs(120);
pub const MAX_CHILD_TOOL_CALLS: usize = 24;

#[derive(Debug, Clone)]
pub enum PreparedSpecializedRuntime {
    Security {
        bundle: SecurityEvidenceBundle,
    },
    Research {
        plan: BoundedResearchPlan,
        ledger: ResearchEvidenceLedger,
    },
}

#[derive(Debug, Clone, Default)]
pub struct ResearchEvidenceLedger {
    pub reports: Vec<ResearchEvidenceReport>,
    pub sources: Vec<crate::research::types::SourceRecord>,
    pub evidence: Vec<EvidenceRecord>,
    pub claims: Vec<runtime::ClaimRecord>,
    pub limitations: Vec<String>,
    pub conflicts: Vec<runtime::ClaimConflict>,
}

impl ResearchEvidenceLedger {
    pub fn prompt_context(&self) -> String {
        let mut out = String::from("\n\n## Host-validated research evidence ledger\n");
        out.push_str(&format!(
            "- Sources: {}\n- Evidence: {}\n- Claims: {}\n",
            self.sources.len(),
            self.evidence.len(),
            self.claims.len()
        ));
        for claim in self.claims.iter().take(runtime::MAX_CLAIMS) {
            out.push_str(&format!(
                "- Claim {}: {}\n",
                claim.id,
                runtime::bounded_text(&claim.text)
            ));
        }
        for limitation in self.limitations.iter().take(16) {
            out.push_str(&format!(
                "- Limitation: {}\n",
                runtime::bounded_text(limitation)
            ));
        }
        out.push_str("Use only these evidence IDs and source IDs. Do not invent citations.\n");
        out
    }
}

pub async fn coordinate_research(
    plan: &BoundedResearchPlan,
    pool: Option<&Arc<SubAgentPool>>,
    session_id: &str,
    workspace: &std::path::Path,
    parent_model: &str,
) -> Result<ResearchEvidenceLedger, AppError> {
    let Some(pool) = pool else {
        return Ok(ResearchEvidenceLedger {
            limitations: vec!["sub-agent pool unavailable".into()],
            ..Default::default()
        });
    };

    let mut reports = Vec::new();
    for task in plan.tasks.iter().take(runtime::MAX_CHILD_TASKS) {
        let task_id = pool
            .task_store()
            .lock()
            .await
            .create_task(
                format!("research evidence: {}", task.id),
                child_prompt(task),
                "general".into(),
                Some(session_id.into()),
                vec![
                    "apply_patch".into(),
                    "write_file".into(),
                    "edit".into(),
                    "task".into(),
                ],
                vec![workspace.to_string_lossy().into_owned()],
            )
            .await;
        let request = SubAgentRequest {
            task_id,
            run_id: None,
            prompt: child_prompt(task),
            agent: "general".into(),
            parent_id: Some(session_id.into()),
            parent_run_id: None,
            denied_tools: vec![
                "apply_patch".into(),
                "write_file".into(),
                "edit".into(),
                "task".into(),
            ],
            allowed_paths: vec![workspace.to_string_lossy().into_owned()],
            description: format!("research evidence: {}", task.id),
            depth: 1,
            max_tool_calls: Some(MAX_CHILD_TOOL_CALLS),
            parent_model: Some(parent_model.into()),
            workspace_root: Some(workspace.to_path_buf()),
        };
        let result =
            tokio::time::timeout(MAX_CHILD_TIMEOUT, pool.spawner().send_and_wait(request)).await;
        match result {
            Ok(Ok(SubAgentResult {
                success: true,
                result,
                ..
            })) => match parse_evidence_report(&result, &task.id) {
                Ok(report) => reports.push(report),
                Err(error) => reports.push(failed_report(&task.id, error)),
            },
            Ok(Ok(SubAgentResult { result, .. })) => reports.push(failed_report(&task.id, result)),
            Ok(Err(error)) => reports.push(failed_report(&task.id, error)),
            Err(_) => reports.push(failed_report(&task.id, "child timeout".into())),
        }
    }
    Ok(aggregate_research(plan, reports))
}

fn child_prompt(task: &runtime::ResearchTask) -> String {
    format!(
        "You are a read-only research evidence child. Role: {:?}. Objective: {}. Scope: {}. Return ONLY one JSON ResearchEvidenceReport with task_id {:?}, sources, evidence, claims, and limitations. Never mutate, delegate, or provide a final answer.",
        task.role, task.objective, task.scope, task.id
    )
}

fn parse_evidence_report(text: &str, task_id: &str) -> Result<ResearchEvidenceReport, String> {
    if text.len() > 256 * 1024 {
        return Err("child report exceeds size limit".into());
    }
    let report: ResearchEvidenceReport = serde_json::from_str(text.trim())
        .map_err(|error| format!("malformed typed evidence report: {error}"))?;
    if report.task_id != task_id {
        return Err("child task_id does not match planned task".into());
    }
    Ok(report)
}

fn failed_report(task_id: &str, error: String) -> ResearchEvidenceReport {
    ResearchEvidenceReport {
        task_id: task_id.into(),
        sources: Vec::new(),
        evidence: Vec::new(),
        claims: Vec::new(),
        limitations: vec![runtime::bounded_text(&error)],
    }
}

pub fn aggregate_research(
    plan: &BoundedResearchPlan,
    reports: Vec<ResearchEvidenceReport>,
) -> ResearchEvidenceLedger {
    let mut ledger = ResearchEvidenceLedger {
        reports,
        ..Default::default()
    };
    ledger.sources = runtime::deduplicate_sources(
        ledger
            .reports
            .iter()
            .flat_map(|r| r.sources.clone())
            .take(plan.max_sources),
    );
    let source_ids: BTreeSet<_> = ledger.sources.iter().map(|s| s.id.clone()).collect();
    let mut evidence_ids = BTreeSet::new();
    for report in &ledger.reports {
        for evidence in report.evidence.iter().take(plan.max_evidence) {
            if source_ids.contains(&evidence.source_id) && evidence_ids.insert(evidence.id.clone())
            {
                ledger.evidence.push(evidence.clone());
            }
        }
        for claim in report.claims.iter().take(runtime::MAX_CLAIMS) {
            if claim
                .evidence_ids
                .iter()
                .all(|id| evidence_ids.contains(id))
            {
                ledger.claims.push(claim.clone());
            }
        }
        ledger
            .limitations
            .extend(report.limitations.iter().map(|s| runtime::bounded_text(s)));
    }
    ledger.claims.truncate(runtime::MAX_CLAIMS);
    ledger.evidence.truncate(plan.max_evidence);
    ledger.limitations.truncate(32);
    let mut by_text: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for claim in &ledger.claims {
        by_text
            .entry(claim.text.trim().to_ascii_lowercase())
            .or_default()
            .push(claim.id.clone());
    }
    ledger.conflicts = by_text
        .into_values()
        .filter(|ids| ids.len() > 1)
        .map(|claim_ids| runtime::ClaimConflict {
            claim_ids,
            explanation: "duplicate or conflicting normalized claims require review".into(),
        })
        .collect();
    ledger
}

pub fn finalize(
    prepared: &PreparedSpecializedRuntime,
    terminal: &AgentLoopTerminalOutput,
) -> Result<(), AppError> {
    if terminal.public_text.len() > 512 * 1024 {
        return Err(AppError::Other(anyhow::anyhow!(
            "specialized output exceeds size limit"
        )));
    }
    match prepared {
        PreparedSpecializedRuntime::Security { bundle } => {
            let report: SecurityReviewReport = serde_json::from_str(terminal.public_text.trim())
                .map_err(|error| {
                    AppError::Other(anyhow::anyhow!(
                        "security report validation failed: malformed JSON: {error}"
                    ))
                })?;
            if report.findings.len() > security_runtime::MAX_PREFLIGHT_RESULTS
                || report.review_prompts.len() > security_runtime::MAX_PREFLIGHT_RESULTS * 4
                || report.evidence_gaps.len() > security_runtime::MAX_PREFLIGHT_RESULTS
                || report.coverage.len() > security_runtime::MAX_PREFLIGHT_RESULTS
                || report.overall_confidence.len() > security_runtime::MAX_TEXT
            {
                return Err(AppError::Other(anyhow::anyhow!(
                    "security report validation failed: report bounds exceeded"
                )));
            }
            let (_validated, rejected) = security_runtime::validate_report(report, bundle);
            tracing::info!(target: "specialized_runtime", findings_rejected = rejected.len(), "security report finalized");
            Ok(())
        }
        PreparedSpecializedRuntime::Research { plan, ledger } => {
            if plan.kind != runtime::RequestKind::QuickLookup
                && (ledger.evidence.is_empty()
                    || (plan.kind == runtime::RequestKind::MultiSource && ledger.sources.len() < 2))
            {
                return Err(AppError::Other(anyhow::anyhow!(
                    "research finalization failed: minimum evidence policy not met"
                )));
            }
            let report: ResearchReport = serde_json::from_str(terminal.public_text.trim())
                .map_err(|error| {
                    AppError::Other(anyhow::anyhow!(
                        "research report validation failed: malformed JSON: {error}"
                    ))
                })?;
            runtime::validate_report(&report).map_err(|errors| {
                AppError::Other(anyhow::anyhow!(
                    "research report validation failed: {}",
                    errors.join("; ")
                ))
            })?;
            let source_ids: BTreeSet<_> = ledger.sources.iter().map(|s| s.id.as_str()).collect();
            if report
                .evidence
                .iter()
                .any(|e| !source_ids.contains(e.source_id.as_str()))
                && !ledger.sources.is_empty()
            {
                return Err(AppError::Other(anyhow::anyhow!(
                    "research report cites source outside validated ledger"
                )));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn security_prepared() -> PreparedSpecializedRuntime {
        PreparedSpecializedRuntime::Security {
            bundle: SecurityEvidenceBundle {
                scope: crate::security::runtime::SecurityReviewInput {
                    workspace_root: PathBuf::from("/repo"),
                    base: None,
                    active_file: None,
                },
                targets: Vec::new(),
                evidence: Vec::new(),
                evidence_gaps: Vec::new(),
                coverage: Vec::new(),
                fingerprint: "fixture".into(),
            },
        }
    }

    fn terminal(text: &str) -> AgentLoopTerminalOutput {
        AgentLoopTerminalOutput {
            public_text: text.into(),
            stop_reason: "stop".into(),
            usage: None,
            tool_event_count: 0,
        }
    }

    #[test]
    fn malformed_security_output_is_not_success() {
        assert!(finalize(&security_prepared(), &terminal("not json")).is_err());
    }

    #[test]
    fn empty_quick_lookup_report_can_be_explicitly_limited() {
        let prepared = PreparedSpecializedRuntime::Research {
            plan: runtime::build_plan(runtime::RuntimeResearchRequest {
                question: "What is Tokio?".into(),
                scope: None,
            }),
            ledger: ResearchEvidenceLedger {
                limitations: vec!["no approved source backend".into()],
                ..Default::default()
            },
        };
        let report = serde_json::json!({
            "question": "What is Tokio?", "claims": [], "sources": [],
            "evidence": [], "conflicts": [], "unresolved_questions": [],
            "limitations": ["no approved source backend"]
        });
        assert!(finalize(&prepared, &terminal(&report.to_string())).is_ok());
    }

    #[test]
    fn research_fabricated_citation_is_rejected() {
        let prepared = PreparedSpecializedRuntime::Research {
            plan: runtime::build_plan(runtime::RuntimeResearchRequest {
                question: "What is Tokio?".into(),
                scope: None,
            }),
            ledger: ResearchEvidenceLedger::default(),
        };
        let report = serde_json::json!({
            "question": "What is Tokio?", "claims": [], "sources": [],
            "evidence": [{"id":"e1","source_id":"missing","claim_fragment":"x","relation":"supports","excerpt":"x","confidence":"low"}],
            "conflicts": [], "unresolved_questions": [], "limitations": []
        });
        assert!(finalize(&prepared, &terminal(&report.to_string())).is_err());
    }
}
