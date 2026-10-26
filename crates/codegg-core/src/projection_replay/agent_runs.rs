//! Pure adapters from durable delegated-run authority to the frontend
//! projection contract.
//!
//! These functions intentionally take already-loaded records. They do not
//! read storage, send controls, allocate worktrees, or infer provenance from
//! display IDs. Callers load the authoritative records and publish the
//! resulting bounded DTO/event through `ProjectionReplayService`.

use codegg_protocol::projection::dto::{
    AgentRunGroupSummaryProjection, AgentRunSummary, AgentRunTranscriptBoundExceeded,
    AgentRunTranscriptEntryProjection, AgentRunTranscriptKind, AgentRunTranscriptPageDto,
    AgentRunTranscriptPageStatus, AgentRunTranscriptProjection, AgentRunTranscriptRole,
    AgentRunTranscriptSourceState, AgentRunTranscriptTruncation, WorktreeSummaryProjection,
};

use super::super::agent_run::{AgentRunRecord, AgentRunStatus, AgentTaskRecord};
use super::super::agent_run_group::AgentRunGroupSummary;
#[cfg(test)]
use super::super::agent_run_group::{
    AgentRunGroupOwner, AgentRunGroupRecord, RunGroupStatus, RunJoinPolicy,
};
use super::super::agent_run_transcript::{
    AgentRunTranscriptError, AgentRunTranscriptRead, AgentRunTranscriptStore, TranscriptKind,
    TranscriptRole,
};
use crate::identity::AgentRunId;
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
        project_id: task.project_id.to_string(),
        repository_id: task.repository_id.as_ref().map(ToString::to_string),
        workspace_id: task.workspace_id.to_string(),
        job_id: run.job_id.as_ref().map(ToString::to_string),
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
        // The transcript is not read here: this function takes already-loaded
        // records and has no store. `agent_run_summary_with_transcript` is
        // the boundary read that supplies it.
        transcript: None,
    };
    summary.normalise();
    summary
}

/// Populate a run summary's transcript field from the durable transcript
/// store.
///
/// This is the **boundary read**: the delegated tab reads the transcript when
/// it opens, not on a timer, and the read is bounded at the store and
/// honoured here. [`agent_run_summary`] keeps its signature — the callers
/// that build a summary for a lifecycle event keep compiling against it —
/// and this sibling is the additive path that carries the body.
///
/// The body was redacted when it was written; nothing here redacts again.
/// A read that fails leaves the field **absent** rather than reporting the
/// run as having no transcript: the two are different facts.
pub async fn agent_run_summary_with_transcript(
    mut summary: AgentRunSummary,
    store: &dyn AgentRunTranscriptStore,
    run_id: &AgentRunId,
) -> AgentRunSummary {
    let page = match store
        .read(run_id, 0, AgentRunTranscriptProjection::MAX_ROWS)
        .await
    {
        Ok(AgentRunTranscriptRead::Absent) => {
            summary.transcript = Some(AgentRunTranscriptProjection::Absent);
            return summary;
        }
        Ok(AgentRunTranscriptRead::Present(page)) => page,
        Err(error) => {
            tracing::warn!(%error, %run_id, "delegated run transcript read failed");
            return summary;
        }
    };

    // The page is bounded by the row request; the byte bound is applied here
    // so the payload that crosses is inside the contract. Rows that do not
    // fit are **not** dropped silently — the payload states the point below.
    let mut entries: Vec<AgentRunTranscriptEntryProjection> = Vec::new();
    let mut bytes = 0usize;
    let mut cut_by_bytes = false;
    for entry in &page.entries {
        let len = entry.redacted_body.len();
        if bytes + len > AgentRunTranscriptProjection::MAX_BYTES {
            cut_by_bytes = true;
            break;
        }
        bytes += len;
        entries.push(AgentRunTranscriptEntryProjection {
            ordinal: entry.ordinal,
            role: match entry.role {
                TranscriptRole::Orchestrator => AgentRunTranscriptRole::Orchestrator,
                TranscriptRole::Internal => AgentRunTranscriptRole::Internal,
            },
            kind: match entry.kind {
                TranscriptKind::ModelMessage => AgentRunTranscriptKind::ModelMessage,
                TranscriptKind::ToolCall => AgentRunTranscriptKind::ToolCall,
                TranscriptKind::ToolResult => AgentRunTranscriptKind::ToolResult,
                TranscriptKind::TerminalNote => AgentRunTranscriptKind::TerminalNote,
            },
            body: entry.redacted_body.clone(),
            byte_len: entry.byte_len,
            created_at: entry.created_at,
        });
    }

    let omitted_entries = (page.total_entries - entries.len() as i64).max(0);
    let next_ordinal = entries.last().map(|entry| entry.ordinal + 1).unwrap_or(0);
    let truncation =
        (cut_by_bytes || page.has_more || page.truncated).then_some(AgentRunTranscriptTruncation {
            next_ordinal,
            omitted_entries,
            recording_stopped: page.truncated,
        });

    match AgentRunTranscriptProjection::checked(entries, truncation) {
        Ok(payload) => summary.transcript = Some(payload),
        Err(error) => {
            // Refused, not shortened: a payload over the declared bound is
            // not shipped, and the client degrades to the run inspector.
            tracing::warn!(%error, %run_id, "refused an over-bound delegated run transcript");
        }
    }
    summary
}

/// Why a transcript page read did not produce a page.
///
/// The two causes are deliberately distinct. A **storage failure** is not
/// "the run has no transcript": a caller that renders one as the other
/// asserts that a child never spoke. A **bound refusal** is not a shortened
/// page either: it is a page the contract could not carry.
#[derive(Debug, thiserror::Error)]
pub enum AgentRunTranscriptPageError {
    /// The store could not be read.
    #[error("transcript storage failure: {0}")]
    Storage(#[from] AgentRunTranscriptError),
    /// The page was over the declared bound and was refused rather than
    /// silently shortened.
    #[error("transcript page is over the declared bound: {0}")]
    BoundExceeded(#[from] AgentRunTranscriptBoundExceeded),
}

/// Read one bounded page of a delegated run's recorded transcript.
///
/// This is the **tail read**. It is addressed by the store's durable cursor
/// and answers with the authoritative cursor to pass back, so a client can
/// walk past the initial preview without re-fetching the whole session.
///
/// Three server-side clamps are applied here, in this order:
///
/// 1. the cursor is clamped to a non-negative value, so a negative
///    `from_ordinal` reads from the beginning rather than from a negative
///    offset the store would have to interpret;
/// 2. the requested row count is clamped to the declared page row ceiling,
///    independently of the store's own larger read limit;
/// 3. the byte ceiling is applied to the **redacted** bodies — after the
///    store's own write-time redaction and before serialization.
///
/// The cut is always at a row boundary and always states where it stopped:
/// the page's `next_ordinal` is the first row it did **not** carry, and
/// `has_more` is true whenever the store holds rows beyond the page. A
/// full last page therefore does not read as exhausted history — the
/// store's own `recording_stopped` is reported separately, and a page cut
/// short by the byte ceiling is never silently "complete".
pub async fn agent_run_transcript_page(
    store: &dyn AgentRunTranscriptStore,
    run_id: &AgentRunId,
    from_ordinal: i64,
    limit: usize,
) -> Result<AgentRunTranscriptPageDto, AgentRunTranscriptPageError> {
    let from_ordinal = from_ordinal.max(0);
    let limit = limit.min(AgentRunTranscriptPageDto::MAX_ROWS);

    let page = match store.read(run_id, from_ordinal, limit).await? {
        AgentRunTranscriptRead::Absent => {
            return Ok(AgentRunTranscriptPageDto {
                run_id: run_id.to_string(),
                status: AgentRunTranscriptPageStatus::Absent,
                entries: Vec::new(),
                next_ordinal: from_ordinal,
                has_more: false,
                source: AgentRunTranscriptSourceState {
                    recording_stopped: false,
                    total_entries: 0,
                },
            });
        }
        AgentRunTranscriptRead::Present(page) => page,
    };

    // The byte bound is applied to the redacted body, which is exactly what
    // will cross the wire. Rows that do not fit are not dropped silently:
    // the cut point becomes the page's cursor.
    //
    // The store's own per-row bound (`MAX_TRANSCRIPT_ROW_BYTES`, 8 KiB) is
    // not larger than this page's byte ceiling, so the first row of a page
    // always fits and a cut page is never empty. That matters: an empty
    // page with a cursor that does not advance would strand a client that
    // pages by cursor.
    let mut entries: Vec<AgentRunTranscriptEntryProjection> = Vec::new();
    let mut bytes = 0usize;
    let mut cut_by_bytes = false;
    for entry in &page.entries {
        let len = entry.redacted_body.len();
        if bytes + len > AgentRunTranscriptPageDto::MAX_BYTES {
            cut_by_bytes = true;
            break;
        }
        bytes += len;
        entries.push(AgentRunTranscriptEntryProjection {
            ordinal: entry.ordinal,
            role: match entry.role {
                TranscriptRole::Orchestrator => AgentRunTranscriptRole::Orchestrator,
                TranscriptRole::Internal => AgentRunTranscriptRole::Internal,
            },
            kind: match entry.kind {
                TranscriptKind::ModelMessage => AgentRunTranscriptKind::ModelMessage,
                TranscriptKind::ToolCall => AgentRunTranscriptKind::ToolCall,
                TranscriptKind::ToolResult => AgentRunTranscriptKind::ToolResult,
                TranscriptKind::TerminalNote => AgentRunTranscriptKind::TerminalNote,
            },
            body: entry.redacted_body.clone(),
            byte_len: entry.byte_len,
            created_at: entry.created_at,
        });
    }

    // The cursor is the store's own ordinal, not a count of rows returned:
    // a page cut short by the byte ceiling still advances to the first row
    // it did not carry.
    let next_ordinal = entries
        .last()
        .map(|entry| entry.ordinal + 1)
        .unwrap_or(from_ordinal);
    let has_more = page.has_more || cut_by_bytes;
    let status = if page.truncated {
        AgentRunTranscriptPageStatus::RecordingStopped
    } else {
        AgentRunTranscriptPageStatus::Present
    };

    let dto = AgentRunTranscriptPageDto {
        run_id: run_id.to_string(),
        status,
        entries,
        next_ordinal,
        has_more,
        source: AgentRunTranscriptSourceState {
            recording_stopped: page.truncated,
            total_entries: page.total_entries,
        },
    };
    Ok(AgentRunTranscriptPageDto::checked(dto)?)
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
        workstream_claims: Vec::new(),
        workstream_dependencies: Vec::new(),
        workstream_claims_updated_at: None,
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
    use super::super::super::agent_run_transcript::{
        InMemoryAgentRunTranscriptStore, TranscriptKind, TranscriptRole,
    };
    use super::*;
    use crate::identity::{
        AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, RepositoryId, WorktreeId,
    };
    use crate::workspace::WorkspaceId;

    fn summary_for(run_id: &AgentRunId) -> AgentRunSummary {
        AgentRunSummary {
            run_id: run_id.to_string(),
            task_id: "task-1".into(),
            agent: "explore".into(),
            status: "completed".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn the_adapter_carries_the_recorded_transcript_in_order() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let run_id = AgentRunId::new();
        store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "first",
            )
            .await
            .expect("append the first row");
        store
            .append(
                &run_id,
                TranscriptRole::Internal,
                TranscriptKind::ToolCall,
                "second",
            )
            .await
            .expect("append the second row");

        let summary =
            agent_run_summary_with_transcript(summary_for(&run_id), &store, &run_id).await;
        let transcript = summary.transcript.expect("a transcript is attached");
        match transcript {
            AgentRunTranscriptProjection::Complete { entries } => {
                assert_eq!(entries.len(), 2);
                assert_eq!(entries[0].body, "first");
                assert_eq!(entries[1].body, "second");
                assert!(entries[0].ordinal < entries[1].ordinal);
                assert_eq!(entries[0].role, AgentRunTranscriptRole::Orchestrator);
                assert_eq!(entries[1].role, AgentRunTranscriptRole::Internal);
                assert_eq!(entries[1].kind, AgentRunTranscriptKind::ToolCall);
            }
            other => panic!("expected a complete transcript, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_run_with_no_transcript_is_absent_rather_than_empty() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let run_id = AgentRunId::new();
        let summary =
            agent_run_summary_with_transcript(summary_for(&run_id), &store, &run_id).await;
        assert_eq!(
            summary.transcript,
            Some(AgentRunTranscriptProjection::Absent),
            "a run that recorded nothing must say so, not render as an empty conversation"
        );
    }

    #[tokio::test]
    async fn a_long_run_is_bounded_and_states_its_truncation_point() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let run_id = AgentRunId::new();
        // More rows than the payload may carry, so the read is a page and not
        // the whole transcript.
        let total = AgentRunTranscriptProjection::MAX_ROWS + 12;
        for index in 0..total {
            store
                .append(
                    &run_id,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ModelMessage,
                    &format!("row {index}"),
                )
                .await
                .expect("append a row");
        }

        let summary =
            agent_run_summary_with_transcript(summary_for(&run_id), &store, &run_id).await;
        let transcript = summary.transcript.expect("a transcript is attached");
        assert!(
            transcript.body_bytes() <= AgentRunTranscriptProjection::MAX_BYTES,
            "the payload must stay inside the declared bound"
        );
        match &transcript {
            AgentRunTranscriptProjection::Truncated {
                entries,
                truncation,
            } => {
                assert!(entries.len() <= AgentRunTranscriptProjection::MAX_ROWS);
                assert_eq!(truncation.omitted_entries, 12);
                assert_eq!(
                    truncation.next_ordinal,
                    entries.last().expect("a carried row").ordinal + 1,
                    "the truncation point is the ordinal the next row would carry"
                );
                assert!(!truncation.recording_stopped);
            }
            other => panic!("expected a truncated transcript, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_transcript_over_the_byte_bound_is_cut_at_a_stated_point() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let run_id = AgentRunId::new();
        // Rows that fit the row bound but not the byte bound.
        for index in 0..8 {
            store
                .append(
                    &run_id,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ToolResult,
                    &format!("{index}{}", "x".repeat(2 * 1024)),
                )
                .await
                .expect("append a row");
        }

        let summary =
            agent_run_summary_with_transcript(summary_for(&run_id), &store, &run_id).await;
        let transcript = summary.transcript.expect("a transcript is attached");
        assert!(
            transcript.body_bytes() <= AgentRunTranscriptProjection::MAX_BYTES,
            "the payload must stay inside the declared bound"
        );
        match &transcript {
            AgentRunTranscriptProjection::Truncated {
                entries,
                truncation,
            } => {
                assert!(!entries.is_empty(), "the first row must survive the cut");
                assert_eq!(truncation.omitted_entries, 8 - entries.len() as i64);
            }
            other => panic!("expected a truncated transcript, got {other:?}"),
        }
    }

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
            shared_scratch: Vec::new(),
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
        assert_eq!(summary.project_id, task.project_id.to_string());
        assert_eq!(
            summary.repository_id,
            task.repository_id.as_ref().map(ToString::to_string)
        );
        assert_eq!(summary.workspace_id, task.workspace_id.to_string());
        assert_eq!(summary.job_id, None);
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

    /// M002: a page read answers with the store's own cursor.
    #[tokio::test]
    async fn a_page_reads_from_the_cursor_and_answers_with_the_next_cursor() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let run_id = AgentRunId::new();
        for index in 0..5 {
            store
                .append(
                    &run_id,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ModelMessage,
                    &format!("row-{index}"),
                )
                .await
                .expect("append a row");
        }

        let page = agent_run_transcript_page(&store, &run_id, 0, 3)
            .await
            .expect("the page read succeeds");
        assert_eq!(page.entries.len(), 3);
        assert_eq!(page.entries[0].ordinal, 0);
        assert_eq!(
            page.next_ordinal, 3,
            "the cursor is the store's own ordinal"
        );
        assert!(page.has_more, "the store holds rows beyond the page");

        let second = agent_run_transcript_page(&store, &run_id, page.next_ordinal, 3)
            .await
            .expect("the page read succeeds");
        assert_eq!(second.entries.len(), 2);
        assert_eq!(second.entries[0].ordinal, 3);
        assert_eq!(second.next_ordinal, 5);
        assert!(!second.has_more, "the last page does not claim more rows");
        assert!(
            !second.source.recording_stopped,
            "a run that recorded to the end did not stop recording"
        );
    }

    /// M002: a negative cursor reads from the beginning.
    #[tokio::test]
    async fn a_negative_cursor_is_clamped_to_the_beginning() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let run_id = AgentRunId::new();
        store
            .append(
                &run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                "first",
            )
            .await
            .expect("append a row");

        let page = agent_run_transcript_page(&store, &run_id, -9, 8)
            .await
            .expect("the page read succeeds");
        assert_eq!(
            page.entries.first().map(|entry| entry.ordinal),
            Some(0),
            "a negative cursor is clamped to zero"
        );
    }

    /// M002: the byte ceiling cuts the page at a row boundary and the
    /// cursor still says where it stopped.
    #[tokio::test]
    async fn a_page_is_cut_at_the_byte_ceiling_and_still_advances_the_cursor() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let run_id = AgentRunId::new();
        // Each row is well under the ceiling, but together they exceed it.
        let body = "x".repeat(2_000);
        for _ in 0..10 {
            store
                .append(
                    &run_id,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ModelMessage,
                    &body,
                )
                .await
                .expect("append a row");
        }

        let page = agent_run_transcript_page(&store, &run_id, 0, 64)
            .await
            .expect("the page read succeeds");
        assert!(
            page.body_bytes() <= AgentRunTranscriptPageDto::MAX_BYTES,
            "the page must stay inside the declared byte ceiling, got {}",
            page.body_bytes()
        );
        assert!(
            page.entries.len() < 10,
            "the page must be cut short by the byte ceiling"
        );
        assert!(page.has_more, "a cut page still has rows behind it");
        assert_eq!(
            page.next_ordinal,
            page.entries.len() as i64,
            "the cursor names the first row the page did not carry"
        );
    }

    /// M002: a run that recorded nothing is absent, and a run whose
    /// recording stopped says so separately from `has_more`.
    #[tokio::test]
    async fn an_absent_run_and_a_stopped_recording_are_different_answers() {
        let store = InMemoryAgentRunTranscriptStore::new();
        let empty = AgentRunId::new();
        let page = agent_run_transcript_page(&store, &empty, 0, 8)
            .await
            .expect("the page read succeeds");
        assert_eq!(page.status, AgentRunTranscriptPageStatus::Absent);
        assert!(page.entries.is_empty());
        assert!(!page.has_more);
        assert!(!page.source.recording_stopped);
        assert_eq!(page.source.total_entries, 0);

        // A run whose recording hit the store's per-run bound reports the
        // truncation as its own fact, not as "there is more".
        let truncated = AgentRunId::new();
        let huge = "y".repeat(64 * 1024);
        for _ in 0..40 {
            let _ = store
                .append(
                    &truncated,
                    TranscriptRole::Orchestrator,
                    TranscriptKind::ModelMessage,
                    &huge,
                )
                .await
                .expect("append a row");
        }
        let page = agent_run_transcript_page(&store, &truncated, 0, 64)
            .await
            .expect("the page read succeeds");
        assert_eq!(
            page.status,
            AgentRunTranscriptPageStatus::RecordingStopped,
            "a stopped recording is its own status"
        );
        assert!(page.source.recording_stopped);
    }
}
