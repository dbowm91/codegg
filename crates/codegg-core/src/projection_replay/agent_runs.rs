//! Pure adapters from durable delegated-run authority to the frontend
//! projection contract.
//!
//! These functions intentionally take already-loaded records. They do not
//! read storage, send controls, allocate worktrees, or infer provenance from
//! display IDs. Callers load the authoritative records and publish the
//! resulting bounded DTO/event through `ProjectionReplayService`.

use codegg_protocol::projection::dto::{
    AgentRunGroupSummaryProjection, AgentRunSummary, WorktreeSummaryProjection,
};

use super::super::agent_run::{AgentRunRecord, AgentRunStatus, AgentTaskRecord};
use super::super::agent_run_group::AgentRunGroupSummary;
#[cfg(test)]
use super::super::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupRecord, RunGroupStatus, RunJoinPolicy,
};
use crate::run_result::AgentRunResult;
use crate::worktree_service::{WorktreeHealth, WorktreeRecord};

/// Convert authoritative records into the bounded run summary used by all
/// frontend consumers.
pub fn agent_run_summary(
    task: &AgentTaskRecord,
    run: &AgentRunRecord,
    worktree: Option<&WorktreeRecord>,
    result: Option<&AgentRunResult>,
    group_id: Option<&str>,
) -> AgentRunSummary {
    let validation_summary = result.and_then(|result| {
        if result.validation.is_empty() {
            None
        } else {
            Some(
                result
                    .validation
                    .iter()
                    .map(|item| format!("{}: {:?}", item.kind, item.status).to_lowercase())
                    .collect::<Vec<_>>()
                    .join(", "),
            )
        }
    });
    let terminal_summary = result
        .map(|result| result.summary.clone())
        .or_else(|| run.failure_message.clone());
    let attention_required = run.failure_class.is_some()
        || matches!(
            run.status,
            AgentRunStatus::Interrupted | AgentRunStatus::Cancelled
        )
        || worktree.is_some_and(|worktree| {
            matches!(
                worktree.health,
                WorktreeHealth::Dirty
                    | WorktreeHealth::Conflicted
                    | WorktreeHealth::Missing
                    | WorktreeHealth::GitError
            )
        });

    let mut summary = AgentRunSummary {
        run_id: run.run_id.to_string(),
        task_id: run.task_id.to_string(),
        parent_run_id: run.parent_run_id.as_ref().map(ToString::to_string),
        agent: run.agent_name.clone(),
        status: run.status.as_str().into(),
        depth: run.depth,
        worktree_id: run.worktree_id.as_ref().map(ToString::to_string),
        branch: worktree.and_then(|worktree| worktree.branch.clone()),
        base_commit: worktree
            .map(|worktree| worktree.base_commit.clone())
            .or_else(|| result.and_then(|result| result.base_commit.clone())),
        result_commit: result.and_then(|result| result.result_commit.clone()),
        validation_summary,
        group_id: group_id.map(str::to_owned),
        attention_required,
        terminal_summary,
        control_status: if run.cancellation_requested {
            "cancellation_requested"
        } else if run.status == AgentRunStatus::Cancelling {
            "cancelling"
        } else {
            "none"
        }
        .into(),
        progress: None,
        failure_class: run.failure_class.clone(),
        updated_at: run.updated_at,
    };
    // The task is deliberately read here to make it explicit that the
    // projection adapter is fed by the task/run authority. Its description
    // is not projected because prompts and full task bodies are disclosure-
    // sensitive and can be unbounded.
    let _ = task;
    summary.normalise();
    summary
}

/// Convert a managed worktree record without exposing its filesystem path.
pub fn worktree_summary(worktree: &WorktreeRecord) -> WorktreeSummaryProjection {
    let mut summary = WorktreeSummaryProjection {
        worktree_id: worktree.worktree_id.to_string(),
        owner_run_id: worktree.owner_run_id.as_ref().map(ToString::to_string),
        branch: worktree.branch.clone(),
        base_commit: Some(worktree.base_commit.clone()),
        state: worktree.state.as_str().into(),
        health: worktree.health.as_str().into(),
        dirty: matches!(
            worktree.health,
            WorktreeHealth::Dirty | WorktreeHealth::Conflicted
        ),
        conflicted: worktree.health == WorktreeHealth::Conflicted,
        retained_for_attention: !worktree.health.cleanup_safe(),
        updated_at: worktree.updated_at,
    };
    summary.normalise();
    summary
}

/// Convert a recomputed authoritative group summary. Member details stay in
/// the run-summary collection and the group carries only bounded IDs/counts.
pub fn run_group_summary(
    summary: &AgentRunGroupSummary,
    updated_at: i64,
) -> AgentRunGroupSummaryProjection {
    let (owner_kind, owner_session_id, owner_turn_id) = match &summary.group.owner {
        super::super::agent_run_group::AgentRunGroupOwner::Turn {
            session_id,
            turn_id,
        } => ("turn", Some(session_id.clone()), Some(turn_id.clone())),
        super::super::agent_run_group::AgentRunGroupOwner::Run { .. } => ("run", None, None),
    };
    let mut projection = AgentRunGroupSummaryProjection {
        group_id: summary.group.group_id.to_string(),
        owner_run_id: summary.group.owner_run_id.to_string(),
        owner_kind: owner_kind.into(),
        owner_session_id,
        owner_turn_id,
        status: summary.group.status.as_str().into(),
        join_policy: summary.group.join_policy.as_str().into(),
        member_run_ids: summary
            .group
            .member_run_ids
            .iter()
            .map(ToString::to_string)
            .collect(),
        successful: summary.successful,
        failed: summary.failed,
        active: summary.active,
        winner_run_id: summary
            .group
            .winner_run_id
            .as_ref()
            .map(ToString::to_string),
        cancel_remaining_on_satisfaction: summary.group.cancel_remaining_on_satisfaction,
        updated_at,
    };
    projection.normalise();
    projection
}

/// Small helper for an authoritative run to emit a full replacement event.
pub fn run_upsert_event(
    summary: AgentRunSummary,
) -> codegg_protocol::projection::event::ProjectionEvent {
    codegg_protocol::projection::event::ProjectionEvent::AgentRunUpserted { run: summary }
}

#[cfg(test)]
mod tests {
    use super::super::super::agent_run::{AgentRunBudget, AgentRunTerminal};
    use super::*;
    use crate::identity::{
        AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, RepositoryId, WorktreeId,
    };
    use crate::workspace::WorkspaceId;

    #[test]
    fn summary_uses_typed_authority_and_marks_dirty_worktree_for_attention() {
        let task_id = AgentTaskId::new();
        let run_id = AgentRunId::new();
        let worktree_id = WorktreeId::new();
        let task = AgentTaskRecord {
            task_id: task_id.clone(),
            root_task_id: task_id.clone(),
            parent_task_id: None,
            originating_session_id: "session".into(),
            originating_turn_id: None,
            project_id: ProjectId::new(),
            repository_id: Some(RepositoryId::new()),
            workspace_id: WorkspaceId::new(),
            requested_agent: "worker".into(),
            delegation_key: "key".into(),
            request_fingerprint: "key".into(),
            description: "sensitive prompt omitted".into(),
            status: super::super::super::agent_run::AgentTaskStatus::Failed,
            created_at: 1,
            updated_at: 2,
        };
        let run = AgentRunRecord {
            run_id,
            task_id,
            root_run_id: AgentRunId::new(),
            parent_run_id: None,
            depth: 1,
            workspace_id: task.workspace_id.clone(),
            worktree_id: Some(worktree_id.clone()),
            node_id: None,
            job_id: None,
            attempt_id: None,
            agent_name: "worker".into(),
            agent_digest: None,
            provider: "provider".into(),
            model: "model".into(),
            authority_digest: "digest".into(),
            budget: AgentRunBudget::default(),
            status: AgentRunStatus::Failed,
            terminal: Some(AgentRunTerminal::Failed),
            result_ref: None,
            failure_class: Some("executor".into()),
            failure_message: Some("failed".into()),
            cancellation_requested: false,
            created_at: 1,
            started_at: Some(2),
            finished_at: Some(3),
            updated_at: 3,
        };
        let worktree = WorktreeRecord {
            worktree_id,
            project_id: task.project_id.clone(),
            repository_id: task.repository_id.clone().unwrap(),
            workspace_id: task.workspace_id.clone(),
            node_id: None,
            repository_root: "/repo".into(),
            path: "/repo/.codegg/worktrees/w".into(),
            branch: Some("codegg/run".into()),
            base_commit: "abc".into(),
            managed: true,
            state: crate::worktree_service::ManagedWorktreeState::Orphaned,
            health: WorktreeHealth::Dirty,
            lease_generation: 1,
            owner_run_id: Some(run.run_id.clone()),
            created_at: 1,
            updated_at: 3,
        };
        let summary = agent_run_summary(&task, &run, Some(&worktree), None, None);
        assert!(summary.attention_required);
        assert_eq!(summary.depth, 1);
        assert_eq!(summary.result_commit, None);
        let encoded = serde_json::to_string(&summary).unwrap();
        assert!(!encoded.contains("sensitive prompt"));
        assert!(!encoded.contains("/repo/.codegg"));
    }

    #[test]
    fn group_projection_preserves_turn_owner_without_disclosing_inputs() {
        let owner_run_id = AgentRunId::new();
        let group = AgentRunGroupRecord {
            group_id: AgentRunGroupId::new(),
            root_run_id: owner_run_id.clone(),
            owner_run_id: owner_run_id.clone(),
            owner: AgentRunGroupOwner::Turn {
                session_id: "session".into(),
                turn_id: "turn".into(),
            },
            member_run_ids: vec![owner_run_id.clone()],
            join_policy: RunJoinPolicy::All,
            cancel_remaining_on_satisfaction: false,
            status: RunGroupStatus::Pending,
            created_at: 1,
            completed_at: None,
            winner_run_id: None,
            idempotency_key: "call".into(),
        };
        let projection = run_group_summary(
            &AgentRunGroupSummary {
                group,
                members: vec![],
                successful: 0,
                failed: 0,
                active: 1,
                timed_out: false,
            },
            2,
        );
        assert_eq!(projection.owner_kind, "turn");
        assert_eq!(projection.owner_session_id.as_deref(), Some("session"));
        assert_eq!(projection.owner_turn_id.as_deref(), Some("turn"));
    }
}
