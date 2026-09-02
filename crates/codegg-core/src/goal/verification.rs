//! Host-owned, deterministic verification for goal completion requests.
//!
//! A completion request is a claim from the working model.  This module keeps
//! that claim separate from the evidence owned by CodeGG and produces a
//! bounded verdict that the application layer may apply through `GoalStore`.
//! The verifier is intentionally stateless and has no tool or workspace
//! mutation authority.

use serde::{Deserialize, Serialize};

use super::model::{CompletionRequest, Goal};

pub const MAX_PROPOSAL_TEXT_CHARS: usize = 2_000;
pub const MAX_PROPOSAL_ITEMS: usize = 32;
pub const MAX_PROPOSAL_ITEM_CHARS: usize = 256;
pub const MAX_VERDICT_ITEMS: usize = 16;
pub const MAX_VERDICT_TEXT_CHARS: usize = 512;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalCompletionProposal {
    /// Model-supplied explanation. It is retained only as bounded context and
    /// never treated as proof of completion.
    pub evidence: String,
    pub files_changed: Vec<String>,
    pub tests_run: Vec<String>,
    pub remaining_risks: Vec<String>,
}

impl GoalCompletionProposal {
    pub fn from_request(request: CompletionRequest) -> Result<Self, String> {
        let evidence = bounded_text(request.evidence, MAX_PROPOSAL_TEXT_CHARS);
        if evidence.trim().is_empty() {
            return Err("evidence is required to request completion".to_string());
        }

        Ok(Self {
            evidence,
            files_changed: bounded_items(request.files_changed),
            tests_run: bounded_items(request.tests_run),
            remaining_risks: bounded_items(request.remaining_risks),
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostEvidenceStatus {
    Passed,
    Failed,
    InProgress,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalExecutionEvidence {
    /// Stable host-owned record identity, never derived from model text.
    pub id: String,
    /// A bounded source label such as `test` or `delegated_run`.
    pub source: String,
    pub status: HostEvidenceStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalTodoEvidence {
    pub content: String,
    pub status: String,
}

/// Bounded evidence assembled by the application layer from authoritative
/// stores. The verifier accepts values, not store handles or tools.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GoalEvidenceContext {
    pub executions: Vec<GoalExecutionEvidence>,
    pub todos: Vec<GoalTodoEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GoalVerificationVerdict {
    Met {
        summary: String,
    },
    NotMet {
        unmet_criteria: Vec<String>,
        evidence_gaps: Vec<String>,
        next_action: String,
    },
    AwaitingUser {
        reason: String,
    },
}

impl GoalVerificationVerdict {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Met { .. } => "met",
            Self::NotMet { .. } => "not_met",
            Self::AwaitingUser { .. } => "awaiting_user",
        }
    }
}

/// Stateless deterministic verifier. It only interprets structured host
/// evidence and bounded goal/proposal data.
#[derive(Debug, Default, Clone, Copy)]
pub struct GoalVerificationService;

impl GoalVerificationService {
    pub fn verify(
        &self,
        goal: &Goal,
        proposal: &GoalCompletionProposal,
        evidence: &GoalEvidenceContext,
    ) -> GoalVerificationVerdict {
        if !goal.is_active() {
            return GoalVerificationVerdict::AwaitingUser {
                reason: format!(
                    "goal is no longer active (status: {})",
                    goal.status_as_str()
                ),
            };
        }

        let test_runs: Vec<&GoalExecutionEvidence> = evidence
            .executions
            .iter()
            .filter(|execution| execution.source == "test")
            .collect();
        let delegated_runs: Vec<&GoalExecutionEvidence> = evidence
            .executions
            .iter()
            .filter(|execution| execution.source == "delegated_run")
            .collect();

        let mut unmet = Vec::new();
        let mut gaps = Vec::new();

        if let Some(failed) = test_runs
            .iter()
            .find(|run| run.status == HostEvidenceStatus::Failed)
        {
            unmet.push(format!("host-recorded test {} failed", failed.id));
        }
        if let Some(in_progress) = test_runs
            .iter()
            .find(|run| run.status == HostEvidenceStatus::InProgress)
        {
            unmet.push(format!(
                "host-recorded test {} is still in progress",
                in_progress.id
            ));
        }

        if let Some(failed) = delegated_runs
            .iter()
            .find(|run| run.status == HostEvidenceStatus::Failed)
        {
            unmet.push(format!("delegated run {} failed", failed.id));
        }
        if let Some(in_progress) = delegated_runs
            .iter()
            .find(|run| run.status == HostEvidenceStatus::InProgress)
        {
            unmet.push(format!(
                "delegated run {} is still in progress",
                in_progress.id
            ));
        }

        let unfinished_todos: Vec<&GoalTodoEvidence> = evidence
            .todos
            .iter()
            .filter(|todo| matches!(todo.status.as_str(), "pending" | "in_progress" | "blocked"))
            .collect();
        for todo in unfinished_todos.iter().take(MAX_VERDICT_ITEMS) {
            unmet.push(format!("todo remains unfinished: {}", todo.content));
        }

        let has_passed_test = test_runs
            .iter()
            .any(|run| run.status == HostEvidenceStatus::Passed);
        if !proposal.tests_run.is_empty() && test_runs.is_empty() {
            gaps.push("no host-recorded test run matches the completion request".to_string());
        } else if proposal.tests_run.is_empty() && proposal.remaining_risks.is_empty() {
            gaps.push("the completion request contains no test claim or explicit risk".to_string());
        }

        let mut unavailable_criteria = Vec::new();
        for criterion in goal.completion_criteria.iter().take(MAX_VERDICT_ITEMS) {
            let normalized = criterion.to_ascii_lowercase();
            if normalized.contains("test")
                || normalized.contains("pass")
                || normalized.contains("green")
            {
                if !has_passed_test {
                    unavailable_criteria
                        .push(bounded_text(criterion.clone(), MAX_VERDICT_TEXT_CHARS));
                }
            } else if normalized.contains("todo")
                || normalized.contains("task")
                || normalized.contains("remaining")
            {
                if !unfinished_todos.is_empty() {
                    unavailable_criteria
                        .push(bounded_text(criterion.clone(), MAX_VERDICT_TEXT_CHARS));
                }
            } else {
                unavailable_criteria.push(bounded_text(criterion.clone(), MAX_VERDICT_TEXT_CHARS));
            }
        }

        if !unavailable_criteria.is_empty() {
            if proposal.remaining_risks.is_empty() {
                return GoalVerificationVerdict::AwaitingUser {
                    reason: format!(
                        "completion criteria require user-verifiable evidence: {}",
                        unavailable_criteria.join("; ")
                    ),
                };
            }
            gaps.extend(unavailable_criteria);
        }

        if !proposal.remaining_risks.is_empty() {
            return GoalVerificationVerdict::AwaitingUser {
                reason: "the completion request reports remaining risks; user review is required"
                    .to_string(),
            };
        }

        if !unmet.is_empty() || !gaps.is_empty() {
            unmet.truncate(MAX_VERDICT_ITEMS);
            gaps.truncate(MAX_VERDICT_ITEMS);
            return GoalVerificationVerdict::NotMet {
                next_action: bounded_text(
                    if unmet.is_empty() {
                        "establish the missing host-owned evidence, then request completion again"
                            .to_string()
                    } else {
                        "resolve the failed or unfinished host-owned work, then request completion again"
                            .to_string()
                    },
                    MAX_VERDICT_TEXT_CHARS,
                ),
                unmet_criteria: unmet,
                evidence_gaps: gaps,
            };
        }

        if !proposal.tests_run.is_empty() && !has_passed_test {
            return GoalVerificationVerdict::NotMet {
                unmet_criteria: vec!["a passing host-recorded test is required".to_string()],
                evidence_gaps: vec!["claimed tests are not authoritative evidence".to_string()],
                next_action: "run the required test through the supervised test tool, then request completion again".to_string(),
            };
        }

        GoalVerificationVerdict::Met {
            summary: format!(
                "host verification accepted ({} passing test record(s), {} delegated run record(s))",
                test_runs
                    .iter()
                    .filter(|run| run.status == HostEvidenceStatus::Passed)
                    .count(),
                delegated_runs
                    .iter()
                    .filter(|run| run.status == HostEvidenceStatus::Passed)
                    .count()
            ),
        }
    }
}

fn bounded_items(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| bounded_text(item, MAX_PROPOSAL_ITEM_CHARS))
        .filter(|item| !item.trim().is_empty())
        .take(MAX_PROPOSAL_ITEMS)
        .collect()
}

fn bounded_text(value: String, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goal::model::{GoalBudget, GoalStatus, GoalUsage};
    use chrono::Utc;

    fn goal(criteria: Vec<&str>) -> Goal {
        Goal {
            id: "goal-1".into(),
            revision: 0,
            session_id: "session-1".into(),
            project_id: "/tmp/project".into(),
            title: "Test".into(),
            objective: "Do the work".into(),
            status: GoalStatus::Active,
            plan_path: None,
            checkpoint_path: None,
            current_phase: None,
            progress_summary: String::new(),
            next_action: None,
            completion_criteria: criteria.into_iter().map(str::to_string).collect(),
            open_questions: Vec::new(),
            budget: GoalBudget::default(),
            usage: GoalUsage::default(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            started_at: None,
            completed_at: None,
        }
    }

    fn proposal() -> GoalCompletionProposal {
        GoalCompletionProposal::from_request(CompletionRequest {
            evidence: "done".into(),
            files_changed: vec!["src/lib.rs".into()],
            tests_run: vec!["cargo test".into()],
            remaining_risks: Vec::new(),
        })
        .unwrap()
    }

    #[test]
    fn failed_host_test_overrides_model_claim() {
        let evidence = GoalEvidenceContext {
            executions: vec![GoalExecutionEvidence {
                id: "job-1".into(),
                source: "test".into(),
                status: HostEvidenceStatus::Failed,
            }],
            todos: Vec::new(),
        };
        let verdict = GoalVerificationService.verify(&goal(Vec::new()), &proposal(), &evidence);
        assert!(matches!(verdict, GoalVerificationVerdict::NotMet { .. }));
    }

    #[test]
    fn claims_without_host_evidence_do_not_complete() {
        let verdict = GoalVerificationService.verify(
            &goal(Vec::new()),
            &proposal(),
            &GoalEvidenceContext::default(),
        );
        assert!(matches!(verdict, GoalVerificationVerdict::NotMet { .. }));
    }

    #[test]
    fn unsupported_criterion_requires_user() {
        let evidence = GoalEvidenceContext {
            executions: vec![GoalExecutionEvidence {
                id: "job-1".into(),
                source: "test".into(),
                status: HostEvidenceStatus::Passed,
            }],
            todos: Vec::new(),
        };
        let verdict = GoalVerificationService.verify(
            &goal(vec!["Product owner signs off"]),
            &proposal(),
            &evidence,
        );
        assert!(matches!(
            verdict,
            GoalVerificationVerdict::AwaitingUser { .. }
        ));
    }

    #[test]
    fn passing_host_test_meets_empty_criteria() {
        let evidence = GoalEvidenceContext {
            executions: vec![GoalExecutionEvidence {
                id: "job-1".into(),
                source: "test".into(),
                status: HostEvidenceStatus::Passed,
            }],
            todos: Vec::new(),
        };
        let verdict = GoalVerificationService.verify(&goal(Vec::new()), &proposal(), &evidence);
        assert!(matches!(verdict, GoalVerificationVerdict::Met { .. }));
    }

    #[test]
    fn proposal_is_bounded_and_requires_evidence() {
        assert!(GoalCompletionProposal::from_request(CompletionRequest {
            evidence: "   ".into(),
            files_changed: Vec::new(),
            tests_run: Vec::new(),
            remaining_risks: Vec::new(),
        })
        .is_err());

        let proposal = GoalCompletionProposal::from_request(CompletionRequest {
            evidence: "x".repeat(MAX_PROPOSAL_TEXT_CHARS + 10),
            files_changed: (0..MAX_PROPOSAL_ITEMS + 10)
                .map(|index| format!("file-{index}"))
                .collect(),
            tests_run: Vec::new(),
            remaining_risks: Vec::new(),
        })
        .unwrap();
        assert_eq!(proposal.evidence.chars().count(), MAX_PROPOSAL_TEXT_CHARS);
        assert_eq!(proposal.files_changed.len(), MAX_PROPOSAL_ITEMS);
    }
}
