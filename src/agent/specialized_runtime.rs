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

    // Evidence children are independent read-only gathers over disjoint
    // scopes, so they are fanned out concurrently instead of run one after the
    // other. Running them sequentially made coordination cost the *sum* of the
    // children: with `MAX_CHILD_TASKS` children each allowed the full
    // `MAX_CHILD_TIMEOUT`, a turn could sit silent for three times that before
    // the agent loop even started, which read as "research is broken" rather
    // than "research is working". Fanned out, coordination costs the *slowest*
    // child instead, and all children are visible at once because the worker
    // publishes `SubagentStarted`/`SubagentProgress` for each.
    //
    // Each child keeps its own `MAX_CHILD_TIMEOUT` budget and still contributes
    // exactly one report, so a hung or failing child degrades to a failed
    // report instead of stalling the others. Results come back in plan order,
    // so ledger aggregation stays deterministic and identical to the previous
    // sequential behavior.
    let pool = Arc::clone(pool);
    let children: Vec<_> = plan
        .tasks
        .iter()
        .take(runtime::MAX_CHILD_TASKS)
        .map(|task| {
            let pool = Arc::clone(&pool);
            let session_id = session_id.to_string();
            let workspace = workspace.to_path_buf();
            let parent_model = parent_model.to_string();
            let child = async move {
                run_research_child(&pool, task, &session_id, &workspace, &parent_model).await
            };
            move || child
        })
        .collect();
    let reports = fan_out_children(children).await;
    Ok(aggregate_research(plan, reports))
}

/// Run bounded, independent child jobs concurrently and collect one result per
/// job in submission order.
///
/// This is the seam that keeps coordination cost at the *slowest* child rather
/// than the sum of all of them. It deliberately does not sequence the jobs and
/// does not apply a shared deadline: each child owns its own timeout, so a hung
/// child degrades to a failed report instead of stalling its siblings, and the
/// returned vector stays dense and ordered for deterministic aggregation.
async fn fan_out_children<T, F, Fut>(jobs: Vec<F>) -> Vec<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
{
    futures_util::future::join_all(jobs.into_iter().map(|job| job())).await
}

/// Dispatch one bounded evidence child and convert any outcome — success,
/// malformed report, dispatch error, or timeout — into exactly one report so
/// the caller can fan out siblings without one child stalling the others.
async fn run_research_child(
    pool: &SubAgentPool,
    task: &runtime::ResearchTask,
    session_id: &str,
    workspace: &std::path::Path,
    parent_model: &str,
) -> ResearchEvidenceReport {
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
        workspace_id: None,
        workspace_locks: None,
        // M005: research children inherit the parent WorkspaceWrite
        // ceiling by default; read-only evidence work narrows further
        // in the worker.
        parent_sandbox_profile: None,
        sandbox_profile: None,
    };
    let result =
        tokio::time::timeout(MAX_CHILD_TIMEOUT, pool.spawner().send_and_wait(request)).await;
    match result {
        Ok(Ok(SubAgentResult {
            success: true,
            result,
            ..
        })) => match parse_evidence_report(&result, &task.id) {
            Ok(report) => report,
            Err(error) => failed_report(&task.id, error),
        },
        Ok(Ok(SubAgentResult { result, .. })) => failed_report(&task.id, result),
        Ok(Err(error)) => failed_report(&task.id, error),
        Err(_) => failed_report(&task.id, "child timeout".into()),
    }
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
            // Evidence sufficiency is a *coverage quality* gate, not an
            // integrity gate. It used to hard-fail the turn, which discarded a
            // model answer the user had already streamed into the transcript
            // and reported the whole run as failed — the dominant "research is
            // broken" symptom. Reachability made it worse: `QuickLookup` is the
            // tolerant branch, but the caller always supplies a workspace
            // scope, so every research turn hit the hard gate whenever no
            // child produced schema-conforming evidence.
            //
            // The completed answer is real work and is now preserved; the gap
            // is recorded so it stays observable. Integrity checks below
            // (malformed JSON, out-of-ledger citations, bounds) still fail the
            // turn, because those mean the contract itself was violated.
            if plan.kind != runtime::RequestKind::QuickLookup
                && (ledger.evidence.is_empty()
                    || (plan.kind == runtime::RequestKind::MultiSource && ledger.sources.len() < 2))
            {
                tracing::warn!(
                    target: "specialized_runtime",
                    plan_kind = ?plan.kind,
                    evidence = ledger.evidence.len(),
                    sources = ledger.sources.len(),
                    "research completed below the minimum evidence policy; \
                     preserving the answer and recording the coverage gap"
                );
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

    // --- research child fan-out ------------------------------------------------
    //
    // The stall this guards against: children used to be dispatched in a
    // sequential `for` loop that blocked on `send_and_wait` before starting the
    // next, so `MAX_CHILD_TASKS` children each allowed the full
    // `MAX_CHILD_TIMEOUT` stalled the turn for the *sum* of them before the
    // agent loop could begin. These tests pin the fan-out contract that
    // replaced it.

    #[tokio::test]
    async fn fan_out_children_is_bounded_by_the_slowest_child_not_their_sum() {
        let each = Duration::from_millis(150);
        let jobs: Vec<_> = (0..runtime::MAX_CHILD_TASKS)
            .map(|i| {
                move || async move {
                    tokio::time::sleep(each).await;
                    i
                }
            })
            .collect();

        let started = std::time::Instant::now();
        let results = fan_out_children(jobs).await;
        let elapsed = started.elapsed();

        assert_eq!(results.len(), runtime::MAX_CHILD_TASKS);
        // Concurrent: bounded by `each`. Sequential would be `each * n`
        // (450ms for the default 3 children), so the 300ms ceiling separates
        // the two implementations with room for scheduler jitter.
        assert!(
            elapsed < Duration::from_millis(300),
            "fan-out took {elapsed:?}; children appear to have run sequentially \
             (expected under 300ms for {} concurrent children)",
            runtime::MAX_CHILD_TASKS
        );
    }

    #[tokio::test]
    async fn fan_out_children_preserves_plan_order_despite_reversed_completion() {
        // Descending delays make the *last* child finish first, so a result
        // vector ordered by completion would come back reversed.
        let delays = [
            Duration::from_millis(150),
            Duration::from_millis(60),
            Duration::from_millis(10),
        ];
        let jobs: Vec<_> = delays
            .iter()
            .enumerate()
            .map(|(i, delay)| {
                let delay = *delay;
                move || async move {
                    tokio::time::sleep(delay).await;
                    i
                }
            })
            .collect();

        assert_eq!(fan_out_children(jobs).await, vec![0, 1, 2]);
    }

    #[tokio::test]
    async fn a_hung_child_does_not_stall_or_hide_its_siblings() {
        // Mirrors the per-child `MAX_CHILD_TIMEOUT` contract: one child that
        // outlives its own budget degrades to a failed report, while the
        // siblings still return and still occupy their ordered slots.
        let budget = Duration::from_millis(200);
        let jobs: Vec<_> = [
            (0u8, Duration::from_millis(10)),
            (1u8, Duration::from_secs(30)),
            (2u8, Duration::from_millis(10)),
        ]
        .into_iter()
        .map(|(id, delay)| {
            move || async move {
                match tokio::time::timeout(budget, async move {
                    tokio::time::sleep(delay).await;
                    Ok(id)
                })
                .await
                {
                    Ok(result) => result,
                    Err(_) => Err("child timeout"),
                }
            }
        })
        .collect();

        let started = std::time::Instant::now();
        let results = fan_out_children(jobs).await;

        assert_eq!(
            results,
            vec![Ok(0), Err("child timeout"), Ok(2)],
            "a timed-out child must yield its own slot and leave siblings intact"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the hung child must not extend the fan-out beyond its own budget \
             (took {:?}, budget {:?}, child sleep 30s)",
            started.elapsed(),
            budget
        );
    }
}
