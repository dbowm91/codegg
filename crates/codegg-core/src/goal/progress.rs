//! Host-observed Goal continuation progress assessment (M001).
//!
//! Autonomous continuation must not depend on budget alone. Each continuation
//! cycle classifies the canonical state available at the boundary as
//! `Progress`, `VerifiedWait`, or `NoProgress`.
//!
//! The assessment reuses the `ProgressSignal` / `RecoveryController` vocabulary
//! from the turn-local recovery path (`NewEvidence`, `StateChanged`,
//! `ChildAdvanced`, graduated `Nudge -> Correct -> Replan -> Stall`) without
//! creating a second general recovery controller. The durable layer here is a
//! small typed disposition plus a deterministic fingerprint over bounded
//! host-observable metadata only.
//!
//! Fingerprint inputs are deliberately narrow:
//!
//! - goal status string (e.g. `active`);
//! - per-status todo counts (pending / in_progress / blocked / completed /
//!   cancelled);
//! - open-question count (blocker-report signal, not progress);
//! - sorted host-owned execution records (`id`, `source`, `status`).
//!
//! Free-form model reasoning, progress-summary text, file contents, command
//! output, and transcript text are never hashed. A prose-only
//! `goal_update_progress` that changes no todo status, no execution status,
//! and no open-question count therefore leaves the fingerprint unchanged and
//! cannot be classified as `Progress`.
//!
//! `VerifiedWait` is produced only from a host-owned live execution record
//! carrying this goal's provenance label. Model prose naming a job id can
//! never manufacture a wait handle because the assembler only accepts store
//! records.
//!
//! The consecutive no-progress counter itself is intentionally run-local (kept
//! in the continuation loop, not persisted). A daemon restart conservatively
//! restarts the counter; it never marks a goal complete.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::verification::{GoalExecutionEvidence, HostEvidenceStatus};

/// Consecutive `NoProgress` cycles before the owning Goal must be handed to
/// the user. Small by design so stagnation stops well before the 32-cycle
/// emergency continuation cap.
pub const MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER: u8 = 3;

/// Maximum bounded job identity accepted into a wait handle. Host job ids are
/// UUIDs; the bound only guards against pathological store values.
pub const MAX_WAIT_HANDLE_ID_CHARS: usize = 128;

/// Bounded reason a continuation cycle observed no host progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalNoProgressReason {
    /// No todo, execution, or blocker-count change was observed.
    NoStateChange,
    /// Open questions grew with no accompanying host progress. The model
    /// reported a blocker through the structured channel; the runtime still
    /// treats it as no progress for continuation purposes.
    BlockerReported,
    /// Canonical evidence could not be loaded. Never interpreted as progress;
    /// the caller applies the bounded replan / awaiting-user path.
    EvidenceLoadFailed,
}

impl GoalNoProgressReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoStateChange => "no_state_change",
            Self::BlockerReported => "blocker_reported",
            Self::EvidenceLoadFailed => "evidence_load_failed",
        }
    }
}

/// Canonical live handle that justifies a verified wait.
///
/// Only two host-owned sources qualify in M001: a goal-labelled supervised
/// test job or a goal-labelled delegated run, both in `InProgress` state. The
/// id is the durable store identity, never model prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitHandleRef {
    TestJob { job_id: String },
    DelegatedRun { job_id: String },
}

impl WaitHandleRef {
    pub fn bounded(job_id: String, source: &str) -> Option<Self> {
        let id: String = job_id.chars().take(MAX_WAIT_HANDLE_ID_CHARS).collect();
        if id.trim().is_empty() {
            return None;
        }
        match source {
            "test" => Some(Self::TestJob { job_id: id }),
            "delegated_run" => Some(Self::DelegatedRun { job_id: id }),
            _ => None,
        }
    }

    pub fn job_id(&self) -> &str {
        match self {
            Self::TestJob { job_id } | Self::DelegatedRun { job_id } => job_id,
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::TestJob { .. } => "test",
            Self::DelegatedRun { .. } => "delegated_run",
        }
    }
}

/// Typed continuation assessment for one boundary evaluation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalProgressDisposition {
    Progress {
        fingerprint: String,
    },
    VerifiedWait {
        handle: WaitHandleRef,
        fingerprint: String,
    },
    NoProgress {
        fingerprint: String,
        reason: GoalNoProgressReason,
    },
}

impl GoalProgressDisposition {
    /// Bounded operator-facing reason code. Never contains free-form text.
    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::Progress { .. } => "progress",
            Self::VerifiedWait { .. } => "verified_wait",
            Self::NoProgress { reason, .. } => match reason {
                GoalNoProgressReason::NoStateChange => "no_progress",
                GoalNoProgressReason::BlockerReported => "blocker_reported",
                GoalNoProgressReason::EvidenceLoadFailed => "evidence_load_failed",
            },
        }
    }

    pub fn fingerprint(&self) -> &str {
        match self {
            Self::Progress { fingerprint }
            | Self::VerifiedWait { fingerprint, .. }
            | Self::NoProgress { fingerprint, .. } => fingerprint,
        }
    }

    pub fn is_progress(&self) -> bool {
        matches!(self, Self::Progress { .. })
    }
}

/// Bounded host-observable snapshot assembled at a continuation boundary.
///
/// `goal_revision` is carried for compare-and-set staleness checks but is
/// explicitly excluded from the progress fingerprint: a prose-only progress
/// update bumps the revision without changing any host signal and must not
/// read as progress. The same applies to `todo_revision`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalContinuationEvidence {
    pub goal_id: String,
    pub goal_revision: i64,
    pub goal_status: String,
    pub todo_revision: u64,
    pub todo_pending: u32,
    pub todo_in_progress: u32,
    pub todo_blocked: u32,
    pub todo_completed: u32,
    pub todo_cancelled: u32,
    /// Count of structured open questions, not their text. A growing count
    /// signals a blocker report without persisting reasoning.
    pub open_questions: u32,
    pub executions: Vec<GoalExecutionEvidence>,
}

impl GoalContinuationEvidence {
    /// Empty evidence for a goal with no todos and no executions. Useful as a
    /// stagnation baseline in tests.
    pub fn empty(goal_id: impl Into<String>, revision: i64) -> Self {
        Self {
            goal_id: goal_id.into(),
            goal_revision: revision,
            goal_status: "active".to_string(),
            todo_revision: 0,
            todo_pending: 0,
            todo_in_progress: 0,
            todo_blocked: 0,
            todo_completed: 0,
            todo_cancelled: 0,
            open_questions: 0,
            executions: Vec::new(),
        }
    }
}

/// Deterministic bounded fingerprint over host-observable metadata only.
///
/// Identical canonical state yields an identical fingerprint. New todo status
/// distribution, new execution records, or execution status transitions change
/// it. Free-form prose alone does not.
pub fn goal_continuation_fingerprint(evidence: &GoalContinuationEvidence) -> String {
    let mut executions: Vec<(&str, &str, &str)> = evidence
        .executions
        .iter()
        .map(|e| {
            (
                e.id.as_str(),
                e.source.as_str(),
                match e.status {
                    HostEvidenceStatus::Passed => "passed",
                    HostEvidenceStatus::Failed => "failed",
                    HostEvidenceStatus::InProgress => "in_progress",
                    HostEvidenceStatus::Unavailable => "unavailable",
                },
            )
        })
        .collect();
    executions.sort();
    let mut canonical = String::with_capacity(256 + executions.len() * 64);
    canonical.push_str("goal_status=");
    canonical.push_str(&bounded_status(&evidence.goal_status));
    canonical.push_str("|todo=");
    canonical.push_str(&format!(
        "pending:{}|in_progress:{}|blocked:{}|completed:{}|cancelled:{}",
        evidence.todo_pending,
        evidence.todo_in_progress,
        evidence.todo_blocked,
        evidence.todo_completed,
        evidence.todo_cancelled
    ));
    canonical.push_str("|open_questions=");
    canonical.push_str(&evidence.open_questions.to_string());
    canonical.push_str("|exec=");
    for (i, (id, source, status)) in executions.iter().enumerate() {
        if i > 0 {
            canonical.push(';');
        }
        let bounded_id: String = id.chars().take(MAX_WAIT_HANDLE_ID_CHARS).collect();
        let bounded_source: String = source.chars().take(32).collect();
        canonical.push_str(&bounded_id);
        canonical.push(':');
        canonical.push_str(&bounded_source);
        canonical.push(':');
        canonical.push_str(status);
    }
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn bounded_status(status: &str) -> String {
    status.chars().take(32).collect()
}

/// Classify the current boundary against the previous boundary.
///
/// - Any todo status-distribution change or any execution set/status change
///   is authoritative progress (`NewEvidence` / `StateChanged` /
///   `ChildAdvanced` in `ProgressSignal` vocabulary).
/// - Otherwise a live goal-owned `InProgress` execution is a verified wait.
/// - Otherwise a growing open-question count is a structured blocker report
///   (`NoProgress` with `BlockerReported`).
/// - Otherwise the turn produced no host-observable state change.
pub fn assess_goal_continuation(
    previous: Option<&GoalContinuationEvidence>,
    current: &GoalContinuationEvidence,
) -> GoalProgressDisposition {
    let fingerprint = goal_continuation_fingerprint(current);
    let Some(previous) = previous else {
        return GoalProgressDisposition::Progress { fingerprint };
    };
    if has_host_progress(previous, current) {
        return GoalProgressDisposition::Progress { fingerprint };
    }
    if let Some(handle) = live_wait_handle(current) {
        return GoalProgressDisposition::VerifiedWait {
            handle,
            fingerprint,
        };
    }
    if current.open_questions > previous.open_questions {
        return GoalProgressDisposition::NoProgress {
            fingerprint,
            reason: GoalNoProgressReason::BlockerReported,
        };
    }
    GoalProgressDisposition::NoProgress {
        fingerprint,
        reason: GoalNoProgressReason::NoStateChange,
    }
}

/// Disposition for a boundary where canonical evidence could not be loaded.
/// Never progress; the caller routes through the bounded replan path.
pub fn disposition_for_evidence_failure(
    previous_fingerprint: Option<&str>,
) -> GoalProgressDisposition {
    GoalProgressDisposition::NoProgress {
        fingerprint: previous_fingerprint.unwrap_or("unavailable").to_string(),
        reason: GoalNoProgressReason::EvidenceLoadFailed,
    }
}

fn has_host_progress(
    previous: &GoalContinuationEvidence,
    current: &GoalContinuationEvidence,
) -> bool {
    if previous.todo_pending != current.todo_pending
        || previous.todo_in_progress != current.todo_in_progress
        || previous.todo_blocked != current.todo_blocked
        || previous.todo_completed != current.todo_completed
        || previous.todo_cancelled != current.todo_cancelled
    {
        return true;
    }
    if executions_changed(&previous.executions, &current.executions) {
        return true;
    }
    false
}

fn executions_changed(
    previous: &[GoalExecutionEvidence],
    current: &[GoalExecutionEvidence],
) -> bool {
    if previous.len() != current.len() {
        return true;
    }
    fn key(e: &GoalExecutionEvidence) -> (String, String, String) {
        (
            e.id.clone(),
            e.source.clone(),
            match e.status {
                HostEvidenceStatus::Passed => "passed".to_string(),
                HostEvidenceStatus::Failed => "failed".to_string(),
                HostEvidenceStatus::InProgress => "in_progress".to_string(),
                HostEvidenceStatus::Unavailable => "unavailable".to_string(),
            },
        )
    }
    let mut prev: Vec<(String, String, String)> = previous.iter().map(key).collect();
    let mut curr: Vec<(String, String, String)> = current.iter().map(key).collect();
    prev.sort();
    curr.sort();
    prev != curr
}

/// First live goal-owned execution, if any. Callers must only supply
/// host-store records carrying this goal's provenance label.
pub fn live_wait_handle(evidence: &GoalContinuationEvidence) -> Option<WaitHandleRef> {
    let mut sorted: Vec<&GoalExecutionEvidence> = evidence
        .executions
        .iter()
        .filter(|e| e.status == HostEvidenceStatus::InProgress)
        .collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    sorted
        .first()
        .and_then(|e| WaitHandleRef::bounded(e.id.clone(), e.source.as_str()))
}

/// Small stagnation policy adapting the turn-local graduated recovery
/// vocabulary (`Nudge -> Correct -> Replan -> Stall`) to continuation scope.
///
/// Step 1 nudges the model to consult host evidence, step 2 issues an
/// explicit replan instruction, and step 3 escalates the owning Goal to the
/// existing `AwaitingUser` state. No new `Blocked` goal status is introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalStagnationStep {
    ContinueWithNudge,
    ContinueWithReplan,
    EscalateToAwaitingUser,
}

pub fn stagnation_step(consecutive_no_progress: u8) -> GoalStagnationStep {
    match consecutive_no_progress {
        0 => GoalStagnationStep::ContinueWithNudge,
        1 => GoalStagnationStep::ContinueWithNudge,
        2 => GoalStagnationStep::ContinueWithReplan,
        _ => GoalStagnationStep::EscalateToAwaitingUser,
    }
}

pub fn stagnation_step_str(step: GoalStagnationStep) -> &'static str {
    match step {
        GoalStagnationStep::ContinueWithNudge => "nudge",
        GoalStagnationStep::ContinueWithReplan => "replan",
        GoalStagnationStep::EscalateToAwaitingUser => "awaiting_user_no_progress",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence_with_todo(completed: u32, pending: u32) -> GoalContinuationEvidence {
        GoalContinuationEvidence {
            goal_id: "goal-1".into(),
            goal_revision: 0,
            goal_status: "active".into(),
            todo_revision: 0,
            todo_pending: pending,
            todo_in_progress: 0,
            todo_blocked: 0,
            todo_completed: completed,
            todo_cancelled: 0,
            open_questions: 0,
            executions: Vec::new(),
        }
    }

    fn execution(id: &str, source: &str, status: HostEvidenceStatus) -> GoalExecutionEvidence {
        GoalExecutionEvidence {
            id: id.into(),
            source: source.into(),
            status,
        }
    }

    #[test]
    fn identical_state_produces_identical_fingerprint() {
        let a = evidence_with_todo(1, 2);
        let b = evidence_with_todo(1, 2);
        assert_eq!(
            goal_continuation_fingerprint(&a),
            goal_continuation_fingerprint(&b)
        );
    }

    #[test]
    fn todo_completion_changes_fingerprint_and_is_progress() {
        let before = evidence_with_todo(0, 2);
        let mut after = evidence_with_todo(1, 1);
        after.todo_revision = 99;
        assert_ne!(
            goal_continuation_fingerprint(&before),
            goal_continuation_fingerprint(&after)
        );
        let disposition = assess_goal_continuation(Some(&before), &after);
        assert!(disposition.is_progress());
        assert_eq!(disposition.reason_code(), "progress");
    }

    #[test]
    fn prose_only_revision_bump_is_not_progress() {
        let mut before = evidence_with_todo(0, 1);
        before.goal_revision = 3;
        before.todo_revision = 4;
        let mut after = before.clone();
        after.goal_revision = 4;
        after.todo_revision = 5;
        assert_eq!(
            goal_continuation_fingerprint(&before),
            goal_continuation_fingerprint(&after)
        );
        let disposition = assess_goal_continuation(Some(&before), &after);
        assert!(matches!(
            disposition,
            GoalProgressDisposition::NoProgress { .. }
        ));
    }

    #[test]
    fn live_execution_is_verified_wait_not_progress() {
        let before = evidence_with_todo(0, 1);
        let mut waiting = evidence_with_todo(0, 1);
        waiting.executions = vec![execution("job-1", "test", HostEvidenceStatus::InProgress)];
        // New live job appearing is host state change, so first observation
        // reads as progress (new evidence arrived).
        let first = assess_goal_continuation(Some(&before), &waiting);
        assert!(first.is_progress());
        // The same live job persisting with no other change is a wait.
        let second = assess_goal_continuation(Some(&waiting), &waiting);
        assert_eq!(second.reason_code(), "verified_wait");
        match &second {
            GoalProgressDisposition::VerifiedWait { handle, .. } => {
                assert_eq!(handle.job_id(), "job-1");
            }
            other => panic!("expected verified wait, got {other:?}"),
        }
    }

    #[test]
    fn terminal_execution_transition_is_progress() {
        let mut running = evidence_with_todo(0, 1);
        running.executions = vec![execution("job-1", "test", HostEvidenceStatus::InProgress)];
        let mut failed = running.clone();
        failed.executions = vec![execution("job-1", "test", HostEvidenceStatus::Failed)];
        let disposition = assess_goal_continuation(Some(&running), &failed);
        assert!(disposition.is_progress());
    }

    #[test]
    fn narration_without_state_change_is_no_progress() {
        let state = evidence_with_todo(0, 1);
        let disposition = assess_goal_continuation(Some(&state), &state);
        assert!(matches!(
            disposition,
            GoalProgressDisposition::NoProgress {
                reason: GoalNoProgressReason::NoStateChange,
                ..
            }
        ));
    }

    #[test]
    fn blocker_report_is_no_progress_with_blocker_reason() {
        let before = evidence_with_todo(0, 1);
        let mut after = before.clone();
        after.open_questions = 2;
        let disposition = assess_goal_continuation(Some(&before), &after);
        assert!(matches!(
            disposition,
            GoalProgressDisposition::NoProgress {
                reason: GoalNoProgressReason::BlockerReported,
                ..
            }
        ));
    }

    #[test]
    fn unknown_source_never_manufactures_wait() {
        let mut evidence = evidence_with_todo(0, 1);
        evidence.executions = vec![execution(
            "job-1",
            "model_prose",
            HostEvidenceStatus::InProgress,
        )];
        assert!(live_wait_handle(&evidence).is_none());
    }

    #[test]
    fn fingerprint_contains_no_freeform_text() {
        let evidence = evidence_with_todo(0, 1);
        let fingerprint = goal_continuation_fingerprint(&evidence);
        assert!(fingerprint.starts_with("sha256:"));
        assert!(!fingerprint.contains("progress"));
    }

    #[test]
    fn stagnation_escalates_on_third_consecutive_no_progress() {
        assert_eq!(stagnation_step(1), GoalStagnationStep::ContinueWithNudge);
        assert_eq!(stagnation_step(2), GoalStagnationStep::ContinueWithReplan);
        assert_eq!(
            stagnation_step(MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER),
            GoalStagnationStep::EscalateToAwaitingUser
        );
        assert_eq!(
            stagnation_step_str(GoalStagnationStep::EscalateToAwaitingUser),
            "awaiting_user_no_progress"
        );
    }
}
