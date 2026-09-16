//! Host-evidence snapshot for WorkPlan completion arbitration (long-horizon M003).
//!
//! The snapshot is a bounded, pure value assembled by the application layer
//! from canonical stores (jobs, runs, artifacts). The assessment layer only
//! interprets it; it never queries stores directly. A missing entry is
//! `Unavailable` — never satisfied.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::model::{WorkEvidenceKind, WorkItem};

/// Deterministic host-evidence status for one evidence ref.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostEvidenceStatus {
    Passed,
    Failed,
    InProgress,
    Unavailable,
}

impl HostEvidenceStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::InProgress => "in_progress",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Bounded snapshot of canonical evidence resolutions.
///
/// Keys are `"<kind>:<ref_id>"` with the closed kind string from
/// [`WorkEvidenceKind::as_str`]. Values are the deterministic host status.
/// Absence means `Unavailable`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkPlanEvidenceSnapshot {
    entries: HashMap<String, HostEvidenceStatus>,
}

impl WorkPlanEvidenceSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn with_entry(
        mut self,
        kind: WorkEvidenceKind,
        ref_id: impl Into<String>,
        status: HostEvidenceStatus,
    ) -> Self {
        self.entries
            .insert(evidence_key(kind, ref_id.into().as_str()), status);
        self
    }

    pub fn insert(&mut self, kind: WorkEvidenceKind, ref_id: &str, status: HostEvidenceStatus) {
        self.entries.insert(evidence_key(kind, ref_id), status);
    }

    pub fn lookup(&self, kind: WorkEvidenceKind, ref_id: &str) -> HostEvidenceStatus {
        self.entries
            .get(&evidence_key(kind, ref_id))
            .copied()
            .unwrap_or(HostEvidenceStatus::Unavailable)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn evidence_key(kind: WorkEvidenceKind, ref_id: &str) -> String {
    format!("{}:{ref_id}", kind.as_str())
}

/// True when the item has at least one host signal that deterministically
/// satisfies it: a `Satisfied` acceptance or a `Passed` evidence ref.
///
/// Owner run/job provenance alone never satisfies: it is provenance only and
/// grants no completion authority.
pub fn item_is_satisfied(item: &WorkItem, snapshot: &WorkPlanEvidenceSnapshot) -> bool {
    if item
        .acceptance
        .iter()
        .any(|c| c.disposition == super::model::WorkAcceptanceDisposition::Satisfied)
    {
        return true;
    }
    item.evidence.iter().any(|evidence| {
        snapshot.lookup(evidence.kind, evidence.ref_id.as_str()) == HostEvidenceStatus::Passed
    })
}

/// True when any evidence ref resolves to `Failed`.
pub fn item_has_failed_evidence(item: &WorkItem, snapshot: &WorkPlanEvidenceSnapshot) -> bool {
    item.evidence.iter().any(|evidence| {
        snapshot.lookup(evidence.kind, evidence.ref_id.as_str()) == HostEvidenceStatus::Failed
    })
}

/// True when any evidence ref resolves to `InProgress`.
pub fn item_has_in_progress_evidence(item: &WorkItem, snapshot: &WorkPlanEvidenceSnapshot) -> bool {
    item.evidence.iter().any(|evidence| {
        snapshot.lookup(evidence.kind, evidence.ref_id.as_str()) == HostEvidenceStatus::InProgress
    })
}

/// True when any evidence ref is dangling (`Unavailable`).
pub fn item_has_unavailable_evidence(item: &WorkItem, snapshot: &WorkPlanEvidenceSnapshot) -> bool {
    item.evidence.iter().any(|evidence| {
        snapshot.lookup(evidence.kind, evidence.ref_id.as_str()) == HostEvidenceStatus::Unavailable
    })
}

/// True when the item's only remaining criteria require human/semantic
/// judgment: at least one `RequiresUserJudgment`, no `Unmet`, no `Failed`,
/// no `InProgress`, and no dangling evidence.
pub fn item_is_user_judgment_only(item: &WorkItem, snapshot: &WorkPlanEvidenceSnapshot) -> bool {
    use super::model::WorkAcceptanceDisposition as Disposition;
    let has_judgment = item
        .acceptance
        .iter()
        .any(|c| c.disposition == Disposition::RequiresUserJudgment);
    if !has_judgment {
        return false;
    }
    let has_unmet = item
        .acceptance
        .iter()
        .any(|c| c.disposition == Disposition::Unmet);
    if has_unmet {
        return false;
    }
    if item_has_failed_evidence(item, snapshot)
        || item_has_in_progress_evidence(item, snapshot)
        || item_has_unavailable_evidence(item, snapshot)
    {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work_plan::model::{
        WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceRef, WorkItemId, WorkItemStatus,
        WorkPlanId,
    };
    use chrono::Utc;

    fn item_fixture() -> WorkItem {
        let now = Utc::now();
        WorkItem {
            id: WorkItemId("wi_1".to_string()),
            plan_id: WorkPlanId("wp_1".to_string()),
            revision: 0,
            position: 0,
            parent_item_id: None,
            dependencies: vec![],
            status: WorkItemStatus::Actionable,
            description: "do work".to_string(),
            acceptance: vec![],
            evidence: vec![],
            owner_run_id: None,
            owner_job_id: None,
            attempts: 0,
            blocker: None,
            next_action: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn missing_entry_is_unavailable_never_satisfied() {
        let item = WorkItem {
            evidence: vec![WorkEvidenceRef {
                kind: WorkEvidenceKind::TestJob,
                ref_id: "job-1".to_string(),
                detail: None,
            }],
            ..item_fixture()
        };
        let snapshot = WorkPlanEvidenceSnapshot::empty();
        assert_eq!(
            snapshot.lookup(WorkEvidenceKind::TestJob, "job-1"),
            HostEvidenceStatus::Unavailable
        );
        assert!(!item_is_satisfied(&item, &snapshot));
        assert!(item_has_unavailable_evidence(&item, &snapshot));
    }

    #[test]
    fn owner_alone_never_satisfies() {
        let item = WorkItem {
            owner_run_id: Some("run-1".to_string()),
            ..item_fixture()
        };
        assert!(!item_is_satisfied(
            &item,
            &WorkPlanEvidenceSnapshot::empty()
        ));
    }

    #[test]
    fn satisfied_acceptance_or_passed_evidence_satisfies() {
        let satisfied = WorkItem {
            acceptance: vec![WorkAcceptance {
                description: "c".to_string(),
                disposition: WorkAcceptanceDisposition::Satisfied,
                note: None,
            }],
            ..item_fixture()
        };
        assert!(item_is_satisfied(
            &satisfied,
            &WorkPlanEvidenceSnapshot::empty()
        ));

        let evidenced = WorkItem {
            evidence: vec![WorkEvidenceRef {
                kind: WorkEvidenceKind::TestJob,
                ref_id: "job-1".to_string(),
                detail: None,
            }],
            ..item_fixture()
        };
        let snapshot = WorkPlanEvidenceSnapshot::empty().with_entry(
            WorkEvidenceKind::TestJob,
            "job-1",
            HostEvidenceStatus::Passed,
        );
        assert!(item_is_satisfied(&evidenced, &snapshot));
    }

    #[test]
    fn user_judgment_only_requires_no_deterministic_remainder() {
        let judgment = WorkItem {
            acceptance: vec![WorkAcceptance {
                description: "owner signs off".to_string(),
                disposition: WorkAcceptanceDisposition::RequiresUserJudgment,
                note: None,
            }],
            ..item_fixture()
        };
        assert!(item_is_user_judgment_only(
            &judgment,
            &WorkPlanEvidenceSnapshot::empty()
        ));

        let mixed = WorkItem {
            acceptance: vec![
                WorkAcceptance {
                    description: "tests".to_string(),
                    disposition: WorkAcceptanceDisposition::Unmet,
                    note: None,
                },
                WorkAcceptance {
                    description: "sign off".to_string(),
                    disposition: WorkAcceptanceDisposition::RequiresUserJudgment,
                    note: None,
                },
            ],
            ..item_fixture()
        };
        assert!(!item_is_user_judgment_only(
            &mixed,
            &WorkPlanEvidenceSnapshot::empty()
        ));
    }
}
