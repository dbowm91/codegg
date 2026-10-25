//! C004: a delegated run can be continued.
//!
//! A delegated run that failed with a continuable failure may be continued by
//! the principal that spawned it: one continuation per terminal failure, a
//! new attempt of the same run, under the parent's own authority and ceiling,
//! and no provider entry before the recorded transcript has been read back and
//! its redaction re-verified.
//!
//! Every refusal here is a typed reason, and every refusal is decided before
//! any provider entry: the tool answers with the reason rather than starting
//! anything.

use std::sync::Arc;

use codegg::agent::run_control::RunControlService;
use codegg::scheduler::{JobScheduler, JobSubmissionService, ResolvedSchedulerConfig};
use codegg::tool::task::TaskTool;
use codegg::tool::Tool;
use codegg_core::agent_run::{
    AgentRunBudget, AgentRunStore, AgentRunTerminalOutcome, NewAgentRun, NewAgentTask,
    SqliteAgentRunStore,
};
use codegg_core::agent_run_attempt::{
    AgentRunAttemptEvent, AgentRunAttemptKind, AgentRunAttemptStore, ContinuationState,
    DelegatedFailureClass, NewAgentRunAttempt, SqliteAgentRunAttemptStore,
};
use codegg_core::agent_run_transcript::{
    AgentRunTranscriptStore, SqliteAgentRunTranscriptStore, TranscriptKind, TranscriptRole,
};
use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobPayload, JobStore};
use codegg_core::workspace::WorkspaceId;
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};
use sqlx::SqlitePool;

use super::common;

const SESSION: &str = "sess-c004-continuation";
const OTHER_SESSION: &str = "sess-c004-other";

/// A delegated run's own failure class, as the continuability decision reads
/// it.
fn failure_class_name(class: DelegatedFailureClass) -> &'static str {
    class.as_str()
}

/// The durable state a continuation request is decided against.
struct Fixture {
    pool: SqlitePool,
    runs: Arc<dyn AgentRunStore>,
    control: Arc<RunControlService>,
    transcript: Arc<dyn AgentRunTranscriptStore>,
    attempts: Arc<dyn AgentRunAttemptStore>,
    jobs: Arc<dyn JobStore>,
    workspace_id: WorkspaceId,
    services: Arc<WorkspaceServiceRegistry>,
}

impl Fixture {
    async fn new() -> Self {
        let pool = common::pool::isolated_pool().await;
        let runs: Arc<dyn AgentRunStore> = Arc::new(SqliteAgentRunStore::new(pool.clone()));
        let control: Arc<RunControlService> = RunControlService::in_memory(runs.clone());
        let transcript: Arc<dyn AgentRunTranscriptStore> =
            Arc::new(SqliteAgentRunTranscriptStore::new(pool.clone()));
        let attempts: Arc<dyn AgentRunAttemptStore> =
            Arc::new(SqliteAgentRunAttemptStore::new(pool.clone()));
        let jobs: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
        let root = std::env::current_dir().expect("workspace root");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.as_path())
            .await
            .expect("register the workspace");
        let workspace_id = workspace.id.clone();
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        Self {
            pool,
            runs,
            control,
            transcript,
            attempts,
            jobs,
            workspace_id,
            services,
        }
    }

    /// The ordinary submission path a continuation dispatches through.
    async fn submission(&self) -> Arc<JobSubmissionService> {
        let services = self.services.clone();
        let scheduler = JobScheduler::new(
            self.jobs.clone(),
            services.clone(),
            ResolvedSchedulerConfig::default(),
            DaemonGeneration::new_unchecked("gen-c004"),
        );
        JobSubmissionService::new(
            self.jobs.clone(),
            scheduler,
            services.clone(),
            DaemonGeneration::new_unchecked("gen-c004"),
        )
    }

    /// A delegated run that failed with `failure_class`, carrying the attempt
    /// record of its first attempt.
    async fn failed_run(
        &self,
        session: &str,
        failure_class: Option<&str>,
        parent: Option<(AgentRunId, AgentTaskId)>,
    ) -> (AgentRunId, AgentTaskId) {
        let task_id = AgentTaskId::new();
        let run_id = AgentRunId::new();
        let submission = self
            .runs
            .create_or_get(
                NewAgentTask {
                    task_id,
                    parent_task_id: parent.as_ref().map(|(_, task_id)| task_id.clone()),
                    originating_session_id: session.into(),
                    originating_turn_id: Some("turn-1".into()),
                    project_id: ProjectId::new(),
                    repository_id: None,
                    workspace_id: self.workspace_id.clone(),
                    requested_agent: "general".into(),
                    delegation_key: format!("delegation-{}", run_id.as_str()),
                    request_fingerprint: format!("fingerprint-{}", run_id.as_str()),
                    description: "a delegated task".into(),
                },
                NewAgentRun {
                    run_id: run_id.clone(),
                    parent_run_id: parent.as_ref().map(|(run_id, _)| run_id.clone()),
                    // A nested delegation is one level deeper than its parent.
                    depth: if parent.is_some() { 2 } else { 1 },
                    workspace_id: self.workspace_id.clone(),
                    agent_name: "general".into(),
                    agent_digest: None,
                    provider: "test".into(),
                    model: "test/model".into(),
                    authority_digest: "authority".into(),
                    budget: AgentRunBudget::default(),
                    shared_scratch: Vec::new(),
                },
            )
            .await
            .expect("create the delegated run");
        self.runs
            .finish(
                &submission.run.run_id,
                AgentRunTerminalOutcome::Failed,
                None,
                failure_class.map(str::to_string),
                Some("the child failed".into()),
            )
            .await
            .expect("fail the delegated run");
        self.record_original_attempt(&submission.run.run_id, session)
            .await;
        (submission.run.run_id, submission.task.task_id)
    }

    /// The run's first attempt, with the ceiling it ran under.
    async fn record_original_attempt(&self, run_id: &AgentRunId, session: &str) {
        let payload = JobPayload::SubagentRun {
            prompt: "do the delegated work".into(),
            agent: "general".into(),
            model: Some("test/model".into()),
            parent_id: Some(session.into()),
            denied_tools: vec!["shell".into()],
            allowed_paths: vec!["/tmp/c004-root".into()],
            max_tool_calls: None,
            task_id: AgentTaskId::new(),
            run_id: run_id.clone(),
            delegation_key: format!("delegation-{}", run_id.as_str()),
            base_commit: None,
            placement: Default::default(),
        };
        self.attempts
            .append_attempt(NewAgentRunAttempt {
                run_id: run_id.clone(),
                attempt_ordinal: 1,
                event: AgentRunAttemptEvent::Attempt,
                attempt_kind: AgentRunAttemptKind::Original,
                continued_from_ordinal: None,
                principal: session.into(),
                principal_turn_id: "turn-1".into(),
                authority_digest: "authority".into(),
                transcript_digest: None,
                failure_class: None,
                ceiling_json: Some(serde_json::to_string(&payload).expect("serialize the ceiling")),
                outcome: None,
                detail: None,
            })
            .await
            .expect("record the run's first attempt");
    }

    /// A delegated run that reached a terminal outcome other than a failure.
    async fn completed_run(&self, session: &str) -> AgentRunId {
        self.terminal_run(session, AgentRunTerminalOutcome::Completed, None)
            .await
    }

    /// A delegated run that was cancelled.
    async fn cancelled_run(&self, session: &str) -> AgentRunId {
        self.terminal_run(session, AgentRunTerminalOutcome::Cancelled, None)
            .await
    }

    /// A delegated run that reached `outcome`.
    async fn terminal_run(
        &self,
        session: &str,
        outcome: AgentRunTerminalOutcome,
        failure_class: Option<&str>,
    ) -> AgentRunId {
        let task_id = AgentTaskId::new();
        let run_id = AgentRunId::new();
        let submission = self
            .runs
            .create_or_get(
                NewAgentTask {
                    task_id,
                    parent_task_id: None,
                    originating_session_id: session.into(),
                    originating_turn_id: Some("turn-1".into()),
                    project_id: ProjectId::new(),
                    repository_id: None,
                    workspace_id: self.workspace_id.clone(),
                    requested_agent: "general".into(),
                    delegation_key: format!("delegation-{}", run_id.as_str()),
                    request_fingerprint: format!("fingerprint-{}", run_id.as_str()),
                    description: "a delegated task".into(),
                },
                NewAgentRun {
                    run_id: run_id.clone(),
                    parent_run_id: None,
                    depth: 1,
                    workspace_id: self.workspace_id.clone(),
                    agent_name: "general".into(),
                    agent_digest: None,
                    provider: "test".into(),
                    model: "test/model".into(),
                    authority_digest: "authority".into(),
                    budget: AgentRunBudget::default(),
                    shared_scratch: Vec::new(),
                },
            )
            .await
            .expect("create the delegated run");
        if outcome == AgentRunTerminalOutcome::Completed {
            for status in [
                codegg_core::agent_run::AgentRunStatus::Queued,
                codegg_core::agent_run::AgentRunStatus::Preparing,
                codegg_core::agent_run::AgentRunStatus::Running,
            ] {
                self.runs
                    .transition(&submission.run.run_id, status)
                    .await
                    .expect("advance the delegated run");
            }
        }
        self.runs
            .finish(
                &submission.run.run_id,
                outcome,
                None,
                failure_class.map(str::to_string),
                None,
            )
            .await
            .expect("terminate the delegated run");
        self.record_original_attempt(&submission.run.run_id, session)
            .await;
        submission.run.run_id
    }

    /// A recorded transcript for the run.
    async fn record_transcript(&self, run_id: &AgentRunId, body: &str) {
        self.transcript
            .append(
                run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                body,
            )
            .await
            .expect("record the transcript");
    }

    /// A tool that asks as `session`.
    fn tool(&self, session: &str) -> TaskTool {
        self.tool_with_submission(session, None)
    }

    /// A tool that asks as `session` and dispatches through `submission`.
    fn tool_with_submission(
        &self,
        session: &str,
        submission: Option<Arc<JobSubmissionService>>,
    ) -> TaskTool {
        let mut tool = TaskTool::new(
            Arc::new(tokio::sync::Mutex::new(codegg::tool::task::TaskStore::new())),
            None,
            Some(session.to_string()),
            Vec::new(),
        )
        .with_agent_run_store_opt(Some(self.runs.clone()))
        .with_run_control_opt(Some(self.control.clone()))
        .with_transcript_store(Some(self.transcript.clone()))
        .with_turn_owner(session.to_string(), "turn-2".to_string());
        if let Some(submission) = submission {
            tool =
                tool.with_submission(submission, std::env::current_dir().expect("workspace root"));
        }
        tool
    }

    /// The tool's answer for a continuation request.
    ///
    /// A dispatch failure is part of the answer: the fixture has no
    /// submission path, and the claim is taken before the dispatch, so the
    /// refusal and the claim are both observable.
    async fn continue_run(&self, session: &str, run_id: &AgentRunId) -> String {
        match self
            .tool(session)
            .execute(serde_json::json!({
                "action": "continue",
                "run_id": run_id.as_str(),
            }))
            .await
        {
            Ok(answer) => answer,
            Err(error) => format!("dispatch failed: {error}"),
        }
    }
}

/// The typed reason a refusal carries.
fn refusal_code(answer: &str) -> String {
    answer
        .lines()
        .find_map(|line| line.strip_prefix("Reason: "))
        .map(|reason| reason.split(':').next().unwrap_or_default().to_string())
        .unwrap_or_else(|| panic!("the answer carries no typed reason: {answer}"))
}

#[tokio::test]
async fn a_continuation_is_refused_for_a_wrong_principal() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;

    let answer = fixture.continue_run(OTHER_SESSION, &run_id).await;

    assert_eq!(
        refusal_code(&answer),
        "wrong_principal",
        "only the principal that spawned the run may continue it: {answer}"
    );
    assert!(
        answer.contains("Continuation refused"),
        "the answer states that the continuation was refused: {answer}"
    );
}

#[tokio::test]
async fn a_continuation_is_refused_for_a_run_with_no_live_parent() {
    let fixture = Fixture::new().await;
    // A nested delegation whose parent run record is gone: the parent was
    // there when the child was created, and is not there now.
    let (parent, parent_task) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    let (run_id, _) = fixture
        .failed_run(
            SESSION,
            Some("provider_rate_limit"),
            Some((parent.clone(), parent_task)),
        )
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;
    sqlx::query("DELETE FROM agent_run WHERE run_id = ?")
        .bind(parent.as_str())
        .execute(&fixture.pool)
        .await
        .expect("remove the parent run");

    let answer = fixture.continue_run(SESSION, &run_id).await;

    assert_eq!(
        refusal_code(&answer),
        "no_live_parent",
        "a run whose parent is gone has nobody holding its authority: {answer}"
    );
}

#[tokio::test]
async fn a_continuation_is_refused_for_a_cancelled_run() {
    let fixture = Fixture::new().await;
    let run_id = fixture.cancelled_run(SESSION).await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;

    let answer = fixture.continue_run(SESSION, &run_id).await;

    assert_eq!(
        refusal_code(&answer),
        "run_cancelled",
        "a cancellation is a decision, not a failure: {answer}"
    );
}

#[tokio::test]
async fn a_continuation_is_refused_for_a_superseded_run() {
    let fixture = Fixture::new().await;
    // The run reached a different terminal outcome, so the failure the caller
    // wants to continue is stale.
    let run_id = fixture.completed_run(SESSION).await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;

    let answer = fixture.continue_run(SESSION, &run_id).await;

    assert_eq!(
        refusal_code(&answer),
        "run_superseded",
        "a run whose terminal outcome was superseded has no stale failure to continue: {answer}"
    );
}

#[tokio::test]
async fn a_continuation_is_refused_for_an_expired_authorization() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;
    // The failure is older than the authorization's own bound.
    let expired = chrono::Utc::now().timestamp_millis()
        - codegg_core::agent_run_attempt::CONTINUATION_AUTHORIZATION_TTL_MS
        - 60_000;
    sqlx::query("UPDATE agent_run SET finished_at = ? WHERE run_id = ?")
        .bind(expired)
        .bind(run_id.as_str())
        .execute(&fixture.pool)
        .await
        .expect("age the delegated run's failure");

    let answer = fixture.continue_run(SESSION, &run_id).await;

    assert_eq!(
        refusal_code(&answer),
        "authorization_expired",
        "an expired authorization is refused: {answer}"
    );
}

#[tokio::test]
async fn a_continuation_is_refused_for_each_failure_class_that_is_not_continuable() {
    let not_continuable = [
        DelegatedFailureClass::ProviderAuthentication,
        DelegatedFailureClass::Cancelled,
        DelegatedFailureClass::Unclassified,
    ];
    for class in not_continuable {
        let fixture = Fixture::new().await;
        let (run_id, _) = fixture
            .failed_run(SESSION, Some(failure_class_name(class)), None)
            .await;
        fixture
            .record_transcript(&run_id, "the child's recorded turn")
            .await;

        let answer = fixture.continue_run(SESSION, &run_id).await;

        assert_eq!(
            refusal_code(&answer),
            "not_continuable",
            "{} is not continuable under the recorded decision: {answer}",
            failure_class_name(class)
        );
        assert!(
            answer.contains(failure_class_name(class)),
            "the refusal names the class it refused: {answer}"
        );
    }
}

#[tokio::test]
async fn a_continuation_is_refused_for_a_run_with_no_recorded_transcript() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;

    let answer = fixture.continue_run(SESSION, &run_id).await;

    assert_eq!(
        refusal_code(&answer),
        "transcript_absent",
        "a run with no recorded transcript must not be continued from nothing: {answer}"
    );
}

#[tokio::test]
async fn a_continuation_is_refused_when_a_stored_transcript_row_fails_re_verification() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    // A row that was never redacted: written around the store, so the
    // re-verification has something to refuse.
    let secret = "api_key=fixture-api-key-value";
    sqlx::query(
        "INSERT INTO agent_run_transcript (run_id, ordinal, role, kind, redacted_body, \
         byte_len, created_at) VALUES (?, 0, 'orchestrator', 'model_message', ?, ?, ?)",
    )
    .bind(run_id.as_str())
    .bind(secret)
    .bind(secret.len() as i64)
    .bind(chrono::Utc::now().timestamp_millis())
    .execute(&fixture.pool)
    .await
    .expect("write an unredacted transcript row");
    sqlx::query(
        "INSERT INTO agent_run_transcript_state (run_id, next_ordinal, total_bytes, truncated) \
         VALUES (?, 1, ?, 0)",
    )
    .bind(run_id.as_str())
    .bind(secret.len() as i64)
    .execute(&fixture.pool)
    .await
    .expect("write the transcript aggregate");

    let answer = fixture.continue_run(SESSION, &run_id).await;

    assert_eq!(
        refusal_code(&answer),
        "transcript_unverified",
        "a stored row that fails re-verification is refused rather than seeded: {answer}"
    );
    assert!(
        !answer.contains(secret),
        "the refusal must not echo the unredacted row: {answer}"
    );
}

#[tokio::test]
async fn a_duplicate_continuation_is_refused_and_never_replayed() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;

    let submission = fixture.submission().await;
    let first = fixture
        .tool_with_submission(SESSION, Some(submission.clone()))
        .execute(serde_json::json!({
            "action": "continue",
            "run_id": run_id.as_str(),
        }))
        .await
        .expect("the first continuation is dispatched");
    assert!(
        first.contains("Continuation accepted"),
        "the first request takes the continuation: {first}"
    );

    // The second request is refused, not replayed.
    let second = fixture
        .tool_with_submission(SESSION, Some(submission))
        .execute(serde_json::json!({
            "action": "continue",
            "run_id": run_id.as_str(),
        }))
        .await
        .expect("the duplicate request is answered");
    assert_eq!(
        refusal_code(&second),
        "already_continued",
        "a duplicate request is refused, never replayed: {second}"
    );
    let attempts = fixture
        .attempts
        .attempts(&run_id)
        .await
        .expect("read the attempts");
    assert_eq!(
        attempts
            .iter()
            .filter(|row| row.event == AgentRunAttemptEvent::Attempt)
            .count(),
        2,
        "the duplicate must not create a second attempt: {attempts:?}"
    );
}

#[tokio::test]
async fn a_crash_between_the_claim_and_the_dispatch_needs_attention_and_is_never_replayed() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;

    // The claim is taken and the daemon dies before the dispatch.
    let claimed = fixture
        .attempts
        .claim_continuation(codegg_core::agent_run_attempt::NewContinuationClaim {
            run_id: run_id.clone(),
            attempt_ordinal: 2,
            attempt_kind: AgentRunAttemptKind::Continuation,
            continued_from_ordinal: Some(1),
            principal: SESSION.into(),
            principal_turn_id: "turn-1".into(),
            authority_digest: "authority".into(),
            transcript_digest: None,
            failure_class: DelegatedFailureClass::ProviderRateLimited,
        })
        .await
        .expect("claim the continuation");
    assert!(
        matches!(
            claimed,
            codegg_core::agent_run_attempt::ContinuationClaimOutcome::Claimed(_)
        ),
        "the claim is taken"
    );
    let claimed = fixture
        .attempts
        .continuation(&run_id)
        .await
        .expect("read the continuation")
        .expect("the claim");
    assert_eq!(
        claimed.state,
        ContinuationState::Claimed,
        "the crash left the continuation claimed and not dispatched"
    );

    // The next reconciliation surfaces it as needing attention.
    let report = fixture
        .control
        .reconcile_continuations(0)
        .await
        .expect("reconcile the continuations");
    assert_eq!(
        report.needs_attention,
        vec![run_id.clone()],
        "the stale claim is surfaced, not replayed"
    );
    let after = fixture
        .attempts
        .continuation(&run_id)
        .await
        .expect("read the continuation")
        .expect("the claim");
    assert_eq!(
        after.state,
        ContinuationState::NeedsAttention,
        "the claim needs attention rather than being replayed"
    );
    let attempts = fixture
        .attempts
        .attempts(&run_id)
        .await
        .expect("read the attempts");
    assert_eq!(
        attempts
            .iter()
            .filter(|row| row.event == AgentRunAttemptEvent::Attempt)
            .count(),
        1,
        "the reconciliation must not create a second attempt: {attempts:?}"
    );
}

#[tokio::test]
async fn the_delegated_job_is_still_no_retry_and_non_idempotent() {
    // The delegated job's own admission is unchanged by a continuation: a
    // continuation is an explicitly authorized new attempt, not the scheduler
    // retrying a `no_retry` job.
    let job = codegg_core::jobs::JobRecord {
        job_id: codegg_core::jobs::JobId::new_unchecked("job-c004-regression"),
        workspace_id: WorkspaceId::new_unchecked("ws-c004"),
        session_id: Some(SESSION.into()),
        turn_id: Some("turn-1".into()),
        kind: codegg_core::jobs::JobKind::Subagent,
        source: codegg_core::jobs::JobSource::AgentDelegated,
        priority: codegg_core::jobs::JobPriority::Interactive,
        payload: JobPayload::SubagentRun {
            prompt: "do the delegated work".into(),
            agent: "general".into(),
            model: None,
            parent_id: Some(SESSION.into()),
            denied_tools: vec![],
            allowed_paths: vec![],
            max_tool_calls: None,
            task_id: AgentTaskId::new(),
            run_id: AgentRunId::new(),
            delegation_key: "delegation-c004".into(),
            base_commit: None,
            placement: Default::default(),
        },
        resource_request: codegg_core::jobs::ResourceRequest::for_kind(
            codegg_core::jobs::JobKind::Subagent,
        ),
        state: codegg_core::jobs::JobState::Queued,
        current_attempt_id: None,
        attempt_count: 0,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        terminal_at: None,
        cancel_requested_at: None,
        cancel_reason: None,
        labels: Default::default(),
        not_before: None,
        deadline: None,
        schedule_id: None,
        depends_on: vec![],
        parent_job_id: None,
        parent_attempt_id: None,
        parent_call_id: None,
        parent_program_id: None,
        parent_instruction_sequence: None,
        relation_kind: None,
        target: Default::default(),
        timeout: None,
        retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
        idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
    };
    assert_eq!(
        job.retry_policy,
        codegg_core::jobs::RetryPolicy::no_retry(),
        "the delegated job does not retry itself"
    );
    assert_eq!(
        job.idempotency,
        codegg_core::jobs::IdempotencyClass::NonIdempotent,
        "the delegated job is not idempotent"
    );
}

#[tokio::test]
async fn the_continuation_authorization_records_who_continued_which_run_under_which_authority() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;
    let _ = fixture
        .attempts
        .claim_continuation(codegg_core::agent_run_attempt::NewContinuationClaim {
            run_id: run_id.clone(),
            attempt_ordinal: 2,
            attempt_kind: AgentRunAttemptKind::Continuation,
            continued_from_ordinal: Some(1),
            principal: SESSION.into(),
            principal_turn_id: "turn-1".into(),
            authority_digest: "authority".into(),
            transcript_digest: None,
            failure_class: DelegatedFailureClass::ProviderRateLimited,
        })
        .await
        .expect("claim the continuation");

    let claim = fixture
        .attempts
        .continuation(&run_id)
        .await
        .expect("read the continuation")
        .expect("the claim");
    assert_eq!(claim.run_id, run_id, "the claim names the run it continues");
    assert_eq!(
        claim.principal, SESSION,
        "the claim names the principal that continued the run"
    );
    assert_eq!(
        claim.authority_digest, "authority",
        "the claim names the parent's authority, carried through unchanged"
    );
    assert_eq!(
        claim.failure_class,
        DelegatedFailureClass::ProviderRateLimited,
        "the claim names the failure class it continues from"
    );
    assert_eq!(
        claim.attempt_ordinal, 2,
        "the continuation creates the run's second attempt"
    );
}

#[tokio::test]
async fn a_continuation_that_would_widen_the_ceiling_is_refused() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;

    let answer = fixture
        .tool(SESSION)
        .execute(serde_json::json!({
            "action": "continue",
            "run_id": run_id.as_str(),
            "allowed_paths": ["/etc"],
        }))
        .await
        .expect("the continuation request is answered");

    assert_eq!(
        refusal_code(&answer),
        "ceiling_widened",
        "a continuation may not widen the attempt's own ceiling: {answer}"
    );
}

#[tokio::test]
async fn the_attempt_record_reads_as_the_runs_first_attempt_before_any_continuation() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;

    assert_eq!(
        fixture
            .attempts
            .attempt_ordinal(&run_id)
            .await
            .expect("read the attempt ordinal"),
        1,
        "a run with one attempt reads as its first attempt"
    );
}

#[tokio::test]
async fn the_continuation_does_not_touch_the_delegated_jobs_retry_policy() {
    // The submission path a continuation uses is the ordinary one, and the
    // delegated job it submits keeps the delegated job's own admission.
    let store: Arc<dyn JobStore> = Arc::new(codegg_core::jobs::InMemoryJobStore::new());
    let created = store
        .create_job(codegg_core::jobs::NewJob {
            workspace_id: WorkspaceId::new_unchecked("ws-c004"),
            session_id: Some(SESSION.into()),
            turn_id: Some("turn-2".into()),
            kind: codegg_core::jobs::JobKind::Subagent,
            source: codegg_core::jobs::JobSource::AgentDelegated,
            priority: codegg_core::jobs::JobPriority::Interactive,
            payload: JobPayload::SubagentRun {
                prompt: "do the delegated work".into(),
                agent: "general".into(),
                model: None,
                parent_id: Some(SESSION.into()),
                denied_tools: vec![],
                allowed_paths: vec![],
                max_tool_calls: None,
                task_id: AgentTaskId::new(),
                run_id: AgentRunId::new(),
                delegation_key: "delegation-c004#continue#2".into(),
                base_commit: None,
                placement: Default::default(),
            },
            resource_request: codegg_core::jobs::ResourceRequest::for_kind(
                codegg_core::jobs::JobKind::Subagent,
            ),
            timeout: None,
            retry_policy: codegg_core::jobs::RetryPolicy::no_retry(),
            idempotency: codegg_core::jobs::IdempotencyClass::NonIdempotent,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: Default::default(),
        })
        .await
        .expect("create the delegated job");
    assert_eq!(
        created.retry_policy,
        codegg_core::jobs::RetryPolicy::no_retry(),
        "a continuation's job does not retry itself"
    );
    assert_eq!(
        created.idempotency,
        codegg_core::jobs::IdempotencyClass::NonIdempotent,
        "a continuation's job is not idempotent"
    );
}

/// The positive path: a continuable failure is continued by the spawning
/// principal, under the parent's own ceiling, with the recorded transcript as
/// its seeded context.
#[tokio::test]
async fn a_continuable_failure_is_continued_under_the_parent_ceiling() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;
    let submission = fixture.submission().await;

    let answer = fixture
        .tool_with_submission(SESSION, Some(submission))
        .execute(serde_json::json!({
            "action": "continue",
            "run_id": run_id.as_str(),
        }))
        .await
        .expect("the continuation is dispatched");

    assert!(
        answer.contains("Continuation accepted"),
        "the continuation is accepted: {answer}"
    );
    assert!(
        answer.contains(&format!("Run: {run_id}")),
        "the run id is unchanged: {answer}"
    );
    assert!(
        answer.contains("Attempt: 2"),
        "the attempt ordinal is incremented: {answer}"
    );

    // The seeded context is exactly the stored transcript.
    let page = match fixture
        .transcript
        .read(
            &run_id,
            0,
            codegg_core::agent_run_transcript::MAX_TRANSCRIPT_READ_LIMIT,
        )
        .await
        .expect("read the transcript back")
    {
        codegg_core::agent_run_transcript::AgentRunTranscriptRead::Present(page) => page,
        codegg_core::agent_run_transcript::AgentRunTranscriptRead::Absent => {
            panic!("the transcript was recorded")
        }
    };
    let seeded = codegg_core::agent_run_attempt::build_seeded_context(&page);
    assert_eq!(
        seeded, "orchestrator/model_message: the child's recorded turn",
        "the seeded context is exactly the stored transcript"
    );

    // The attempt's record carries the seeded transcript and the ceiling.
    let attempts = fixture
        .attempts
        .attempts(&run_id)
        .await
        .expect("read the attempts");
    let continuation = attempts
        .iter()
        .find(|row| row.attempt_ordinal == 2 && row.event == AgentRunAttemptEvent::Attempt)
        .expect("the continuation's attempt row");
    assert_eq!(
        continuation.attempt_kind,
        AgentRunAttemptKind::Continuation,
        "the continuation is a new attempt of the same run"
    );
    assert_eq!(
        continuation.transcript_digest,
        Some(codegg_core::agent_run_attempt::transcript_digest(
            &page.entries
        )),
        "the attempt records the transcript it was seeded from"
    );

    // The dispatched job carries the attempt's own ceiling, unchanged.
    let run = fixture
        .runs
        .get_run(&run_id)
        .await
        .expect("read the run")
        .expect("the run exists");
    let dispatched = fixture
        .jobs
        .get_job(&run.job_id.expect("the continuation attached its job"))
        .await
        .expect("read the job")
        .expect("the continuation's job");
    let JobPayload::SubagentRun {
        denied_tools,
        allowed_paths,
        model,
        run_id: payload_run,
        delegation_key,
        ..
    } = &dispatched.payload
    else {
        panic!("the continuation dispatches a delegated run's payload");
    };
    assert_eq!(
        payload_run, &run_id,
        "the continuation is a new attempt of the same run"
    );
    assert_eq!(
        denied_tools,
        &vec!["shell".to_string()],
        "the parent's denied tools are carried through unchanged"
    );
    assert_eq!(
        allowed_paths,
        &vec!["/tmp/c004-root".to_string()],
        "the parent's paths are carried through unchanged"
    );
    assert_eq!(
        model,
        &Some("test/model".to_string()),
        "the parent's model is carried through unchanged"
    );
    assert!(
        delegation_key.contains("#continue#2"),
        "the continuation dispatches under its own delegation key: {delegation_key}"
    );
    assert_eq!(
        dispatched.retry_policy,
        codegg_core::jobs::RetryPolicy::no_retry(),
        "the delegated job does not retry itself"
    );
    assert_eq!(
        dispatched.idempotency,
        codegg_core::jobs::IdempotencyClass::NonIdempotent,
        "the delegated job is not idempotent"
    );
}

/// Two concurrent continuation requests for the same run: exactly one wins.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_concurrent_continuations_leave_exactly_one_winner() {
    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;
    let submission = fixture.submission().await;

    let first_tool = fixture.tool_with_submission(SESSION, Some(submission.clone()));
    let second_tool = fixture.tool_with_submission(SESSION, Some(submission));
    let first_run_id = run_id.clone();
    let second_run_id = run_id.clone();
    let first = tokio::spawn(async move {
        first_tool
            .execute(serde_json::json!({
                "action": "continue",
                "run_id": first_run_id.as_str(),
            }))
            .await
    });
    let second = tokio::spawn(async move {
        second_tool
            .execute(serde_json::json!({
                "action": "continue",
                "run_id": second_run_id.as_str(),
            }))
            .await
    });
    let answers = [
        first.await.expect("the first request"),
        second.await.expect("the second request"),
    ];

    let accepted = answers
        .iter()
        .filter(|answer| {
            answer
                .as_ref()
                .is_ok_and(|answer| answer.contains("Continuation accepted"))
        })
        .count();
    assert_eq!(
        accepted, 1,
        "exactly one request wins the continuation: {answers:?}"
    );
    let refused = answers
        .iter()
        .filter(|answer| {
            answer
                .as_ref()
                .is_ok_and(|answer| answer.contains("already_continued"))
        })
        .count();
    assert_eq!(
        refused, 1,
        "the other request is refused with a typed reason: {answers:?}"
    );
    let attempts = fixture
        .attempts
        .attempts(&run_id)
        .await
        .expect("read the attempts");
    assert_eq!(
        attempts
            .iter()
            .filter(|row| row.event == AgentRunAttemptEvent::Attempt)
            .count(),
        2,
        "the race must not create two continuation attempts: {attempts:?}"
    );
}

/// The continuation's outcome arrives on the **same** durable notice channel
/// the first attempt's outcome arrived on: the continuation's job subscribes
/// for its own terminal notice, and the parent's existing drain picks it up.
/// No second channel is created.
#[tokio::test]
async fn the_continuations_outcome_arrives_through_the_durable_notice_path() {
    use codegg::agent::job_notification_recovery::inject_recoverable_job_notifications;
    use codegg::provider::Message;
    use codegg::scheduler::job_notifications::JobNotificationService;

    let fixture = Fixture::new().await;
    let (run_id, _) = fixture
        .failed_run(SESSION, Some("provider_rate_limit"), None)
        .await;
    fixture
        .record_transcript(&run_id, "the child's recorded turn")
        .await;
    let submission = fixture.submission().await;
    let notices = Arc::new(JobNotificationService::with_pool(fixture.pool.clone()));
    ensure_session(&fixture.pool, SESSION).await;

    fixture
        .tool_with_submission(SESSION, Some(submission))
        .with_job_notifications(Some(notices.clone()))
        .execute(serde_json::json!({
            "action": "continue",
            "run_id": run_id.as_str(),
        }))
        .await
        .expect("the continuation is dispatched");

    let run = fixture
        .runs
        .get_run(&run_id)
        .await
        .expect("read the run")
        .expect("the run exists");
    let job = fixture
        .jobs
        .get_job(&run.job_id.expect("the continuation attached its job"))
        .await
        .expect("read the job")
        .expect("the continuation's job");
    // The continued attempt runs and ends: the job reaches its terminal state
    // the way the executor's own path reaches it.
    let attempt = fixture
        .jobs
        .begin_attempt(&job.job_id, &DaemonGeneration::new_unchecked("gen-c004"))
        .await
        .expect("begin the attempt");
    fixture
        .jobs
        .mark_attempt_running(&attempt.attempt_id)
        .await
        .expect("mark the attempt running");
    fixture
        .jobs
        .finish_attempt(codegg_core::jobs::AttemptCompletion {
            attempt_id: attempt.attempt_id,
            state: codegg_core::jobs::AttemptState::Completed,
            error: None,
            run_id: None,
        })
        .await
        .expect("finish the attempt");
    let job = fixture
        .jobs
        .get_job(&job.job_id)
        .await
        .expect("read the job")
        .expect("the continuation's job");
    notices
        .record_terminal(&job, "the continued attempt finished", None)
        .await
        .expect("record the terminal notice");

    let store = codegg_core::session::EventStore::new(fixture.pool.clone());
    let mut messages: Vec<Message> = vec![];
    let report = inject_recoverable_job_notifications(Some(&store), &notices, SESSION, |text| {
        messages.push(Message::System {
            content: std::sync::Arc::new(text),
        })
    })
    .await;
    assert_eq!(report.injected, 1, "the parent's drain injects the notice");
    let content = match &messages[0] {
        Message::System { content } => content.to_string(),
        other => panic!("the notice must arrive as a system message, got {other:?}"),
    };
    assert!(
        content.contains(run_id.as_str()),
        "the notice identifies the continued run: {content}"
    );
}

/// The session rows the durable notice drain writes its parent event into.
async fn ensure_session(pool: &SqlitePool, session_id: &str) {
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, time_created, time_updated, sandboxes) \
         VALUES ('proj-c004', '/tmp', 1, 1, '[]')",
    )
    .execute(pool)
    .await
    .expect("project row");
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, 'proj-c004', 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(session_id)
    .execute(pool)
    .await
    .expect("session row");
}
