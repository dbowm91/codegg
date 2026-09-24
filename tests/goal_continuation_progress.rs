//! M001 Goal progress / verified-wait / no-progress qualification.
//!
//! Deterministic, store-backed coverage for the continuation contract without
//! a live provider or a new CI lane. The autonomous loop itself is exercised
//! through the same assessment + revision/CAS primitives it uses in
//! production.

use codegg_core::goal::progress::{
    assess_goal_continuation, disposition_for_evidence_failure, goal_continuation_fingerprint,
    stagnation_step, GoalContinuationEvidence, GoalNoProgressReason, GoalProgressDisposition,
    GoalStagnationStep, MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER,
};
use codegg_core::goal::{
    GoalCompletionProposal, GoalStatus, GoalVerificationService, HostEvidenceStatus,
};
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, ExecutionTarget, IdempotencyClass,
    JobPayload, JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
    SqliteJobStore,
};
use codegg_core::workspace::WorkspaceId;

async fn test_pool() -> sqlx::SqlitePool {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let url = format!(
        "file:goal_continuation_it_{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4().simple()
    );
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(std::time::Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect in-memory sqlite");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}

async fn ensure_test_session(pool: &sqlx::SqlitePool, session_id: &str, project_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, '[]', ?, ?)",
    )
    .bind(project_id)
    .bind("/tmp/test")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, 'test', '/tmp/test', 'Test', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
}

fn todo_state_with(pending: u32, completed: u32) -> codegg_core::task_state::TodoState {
    use codegg_core::task_state::{TodoItem, TodoPriority, TodoStatus};
    let mut items = Vec::new();
    for i in 0..pending {
        items.push(TodoItem {
            id: format!("pending-{i}"),
            content: format!("pending work {i}"),
            status: TodoStatus::Pending,
            priority: TodoPriority::Medium,
            blocker: None,
        });
    }
    for i in 0..completed {
        items.push(TodoItem {
            id: format!("done-{i}"),
            content: format!("done work {i}"),
            status: TodoStatus::Completed,
            priority: TodoPriority::Medium,
            blocker: None,
        });
    }
    let mut state = codegg_core::task_state::TodoState::new();
    state.items = items;
    state.revision = 1;
    state
}

async fn insert_job_for_goal(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    goal_id: &str,
    finish: Option<AttemptState>,
) -> String {
    let jobs = SqliteJobStore::new(pool.clone());
    let job = jobs
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("/tmp/test"),
            session_id: Some(session_id.to_string()),
            turn_id: None,
            kind: codegg_core::jobs::JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::Test {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: Some("/tmp/test".into()),
                scope: Some("workspace".into()),
                parent_run_id: None,
            },
            resource_request: ResourceRequest::for_kind(codegg_core::jobs::JobKind::Test),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
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
            target: ExecutionTarget::default(),
        })
        .await
        .unwrap();
    let mut labels = std::collections::HashMap::new();
    labels.insert(
        codegg_core::jobs::GOAL_PROVENANCE_LABEL_KEY.to_string(),
        goal_id.to_string(),
    );
    jobs.set_job_labels(&job.job_id, labels).await.unwrap();
    let attempt = jobs
        .begin_attempt(
            &job.job_id,
            &DaemonGeneration::new_unchecked("test-generation"),
        )
        .await
        .unwrap();
    jobs.mark_attempt_running(&attempt.attempt_id)
        .await
        .unwrap();
    if let Some(state) = finish {
        jobs.finish_attempt(AttemptCompletion {
            attempt_id: attempt.attempt_id,
            state,
            error: None,
            run_id: None,
        })
        .await
        .unwrap();
    }
    job.job_id.as_str().to_string()
}

async fn assemble(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    goal: &codegg_core::goal::Goal,
    todo: &codegg_core::task_state::TodoState,
) -> GoalContinuationEvidence {
    codegg::goal_continuation::assemble_continuation_evidence(pool, session_id, goal, todo)
        .await
        .expect("assemble continuation evidence")
}

#[tokio::test(flavor = "current_thread")]
async fn progressing_goal_continues_and_completes_through_verifier() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-progress", "proj-progress").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let goal = store
        .create_active(
            "sess-progress",
            "proj-progress",
            "Progress goal",
            "Ship the feature",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();

    let todo_start = todo_state_with(2, 0);
    let before = assemble(&pool, "sess-progress", &goal, &todo_start).await;
    // First boundary is always progress (baseline observation).
    assert!(assess_goal_continuation(None, &before).is_progress());

    // Host-observed todo completion is progress and resets stagnation.
    let todo_advanced = todo_state_with(1, 1);
    let after = assemble(&pool, "sess-progress", &goal, &todo_advanced).await;
    assert_ne!(
        goal_continuation_fingerprint(&before),
        goal_continuation_fingerprint(&after)
    );
    let disposition = assess_goal_continuation(Some(&before), &after);
    assert!(disposition.is_progress());

    // A passing goal-owned test satisfies the verifier for empty criteria.
    insert_job_for_goal(
        &pool,
        "sess-progress",
        &goal.id,
        Some(AttemptState::Completed),
    )
    .await;
    let evidence =
        codegg::goal_verification::assemble(&pool, "sess-progress", &goal.id, goal.created_at)
            .await
            .unwrap();
    let proposal = GoalCompletionProposal::from_request(codegg_core::goal::CompletionRequest {
        evidence: "done".into(),
        files_changed: vec!["src/lib.rs".into()],
        tests_run: vec!["cargo test".into()],
        remaining_risks: Vec::new(),
    })
    .unwrap();
    let verdict = GoalVerificationService.verify(&goal, &proposal, &evidence);
    assert!(matches!(
        verdict,
        codegg_core::goal::GoalVerificationVerdict::Met { .. }
    ));
    let completed = store
        .complete_if_active(&goal.id, goal.revision)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, GoalStatus::Complete);
}

#[tokio::test(flavor = "current_thread")]
async fn stalled_goal_replans_then_awaits_user_before_emergency_cap() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-stall", "proj-stall").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let goal = store
        .create_active(
            "sess-stall",
            "proj-stall",
            "Stalled goal",
            "Do something",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    let todo = todo_state_with(1, 0);
    let baseline = assemble(&pool, "sess-stall", &goal, &todo).await;

    // Three identical boundaries: nudge, replan, then awaiting-user.
    let mut consecutive: u8 = 0;
    for expected in [
        GoalStagnationStep::ContinueWithNudge,
        GoalStagnationStep::ContinueWithReplan,
        GoalStagnationStep::EscalateToAwaitingUser,
    ] {
        let disposition = assess_goal_continuation(Some(&baseline), &baseline);
        assert!(matches!(
            disposition,
            GoalProgressDisposition::NoProgress {
                reason: GoalNoProgressReason::NoStateChange,
                ..
            }
        ));
        consecutive = consecutive.saturating_add(1);
        assert_eq!(stagnation_step(consecutive), expected);
    }
    assert!(
        consecutive < 32,
        "stagnation must stop well before the emergency cap"
    );
    assert_eq!(
        MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER, 3,
        "terminal threshold stays small"
    );

    let updated = store
        .update_status_if_revision(&goal.id, goal.revision, GoalStatus::AwaitingUser)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, GoalStatus::AwaitingUser);
    // Progress policy never revives a non-active goal.
    assert!(!codegg_core::goal::runtime::should_continue(&updated).should_continue);
}

#[tokio::test(flavor = "current_thread")]
async fn live_job_produces_verified_wait_without_duplication() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-wait", "proj-wait").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let goal = store
        .create_active(
            "sess-wait",
            "proj-wait",
            "Waiting goal",
            "Await the test",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    let todo = todo_state_with(1, 0);
    let before = assemble(&pool, "sess-wait", &goal, &todo).await;
    assert!(before.executions.is_empty());

    let job_id = insert_job_for_goal(&pool, "sess-wait", &goal.id, None).await;
    let waiting = assemble(&pool, "sess-wait", &goal, &todo).await;
    assert_eq!(waiting.executions.len(), 1);
    // New live evidence first reads as progress; the persisting live handle
    // then reads as a verified wait that must not relaunch work.
    assert!(assess_goal_continuation(Some(&before), &waiting).is_progress());
    let second = assess_goal_continuation(Some(&waiting), &waiting);
    match &second {
        GoalProgressDisposition::VerifiedWait { handle, .. } => {
            assert_eq!(handle.job_id(), job_id);
        }
        other => panic!("expected verified wait, got {other:?}"),
    }
    // No duplicate was created by the assessment path.
    let again = assemble(&pool, "sess-wait", &goal, &todo).await;
    assert_eq!(again.executions.len(), 1);
    assert_eq!(again.executions[0].id, job_id);
    assert_eq!(again.executions[0].status, HostEvidenceStatus::InProgress);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_job_is_progress_evidence_and_blocks_completion() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-fail", "proj-fail").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let goal = store
        .create_active(
            "sess-fail",
            "proj-fail",
            "Failing goal",
            "Recover from failure",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    let todo = todo_state_with(1, 0);
    let running_id = insert_job_for_goal(&pool, "sess-fail", &goal.id, None).await;
    let running = assemble(&pool, "sess-fail", &goal, &todo).await;
    assert_eq!(running.executions[0].id, running_id);

    // Simulate the live handle reaching a terminal failure between
    // boundaries: finish the attempt, then reload. The transition is new
    // host evidence (progress), not a duplicate launch.
    let jobs = SqliteJobStore::new(pool.clone());
    let records = jobs
        .list_job_records(codegg_core::jobs::store::JobStoreQuery {
            kinds: vec![codegg_core::jobs::JobKind::Test],
            session_id: Some("sess-fail".to_string()),
            limit: Some(8),
            ..Default::default()
        })
        .await
        .unwrap();
    let job = records
        .iter()
        .find(|j| j.job_id.as_str() == running_id)
        .unwrap();
    let attempts = jobs.list_attempts(&job.job_id).await.unwrap();
    let running_attempt = attempts
        .iter()
        .find(|a| matches!(a.state, codegg_core::jobs::AttemptState::Running))
        .expect("running attempt");
    jobs.finish_attempt(AttemptCompletion {
        attempt_id: running_attempt.attempt_id.clone(),
        state: AttemptState::Failed,
        error: Some(codegg_core::jobs::JobErrorRecord {
            class: codegg_core::jobs::FailureClass::Execution,
            message: "boom".into(),
            transient: false,
        }),
        run_id: None,
    })
    .await
    .unwrap();

    let failed = assemble(&pool, "sess-fail", &goal, &todo).await;
    assert_eq!(failed.executions[0].status, HostEvidenceStatus::Failed);
    assert!(assess_goal_continuation(Some(&running), &failed).is_progress());

    let evidence =
        codegg::goal_verification::assemble(&pool, "sess-fail", &goal.id, goal.created_at)
            .await
            .unwrap();
    let proposal = GoalCompletionProposal::from_request(codegg_core::goal::CompletionRequest {
        evidence: "model claims done".into(),
        files_changed: Vec::new(),
        tests_run: vec!["cargo test".into()],
        remaining_risks: Vec::new(),
    })
    .unwrap();
    let verdict = GoalVerificationService.verify(&goal, &proposal, &evidence);
    assert!(matches!(
        verdict,
        codegg_core::goal::GoalVerificationVerdict::NotMet { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn resume_with_live_job_still_verifies_wait() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-resume", "proj-resume").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let goal = store
        .create_active(
            "sess-resume",
            "proj-resume",
            "Resumable goal",
            "Survive restart",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    insert_job_for_goal(&pool, "sess-resume", &goal.id, None).await;
    // Simulate a daemon restart: reload durable goal state and reassemble
    // from durable job records with a fresh todo snapshot.
    let reloaded = store.get(&goal.id).await.unwrap().unwrap();
    assert!(reloaded.is_active());
    let fresh_todo = todo_state_with(1, 0);
    let evidence = assemble(&pool, "sess-resume", &reloaded, &fresh_todo).await;
    let second = assess_goal_continuation(Some(&evidence), &evidence);
    assert!(matches!(
        second,
        GoalProgressDisposition::VerifiedWait { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn stale_revision_cannot_apply_no_progress_decision() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-stale", "proj-stale").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let goal = store
        .create_active(
            "sess-stale",
            "proj-stale",
            "Racy goal",
            "Win the race",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    let stale_revision = goal.revision;
    // Concurrent progress wins first and bumps the revision.
    let advanced = store
        .update_progress(
            &goal.id,
            codegg_core::goal::GoalProgressUpdate {
                next_action: Some("new action".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(advanced.revision > stale_revision);
    // A stale no-progress escalation based on the old revision aborts.
    assert!(store
        .update_status_if_revision(&goal.id, stale_revision, GoalStatus::AwaitingUser)
        .await
        .unwrap()
        .is_none());
    // The current revision still owns the decision.
    assert!(store
        .update_status_if_revision(&goal.id, advanced.revision, GoalStatus::AwaitingUser)
        .await
        .unwrap()
        .is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn pause_cancel_replace_stop_continuation() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-stop", "proj-stop").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let goal = store
        .create_active(
            "sess-stop",
            "proj-stop",
            "Stoppable goal",
            "Stop when asked",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    assert!(codegg_core::goal::runtime::should_continue(&goal).should_continue);

    store
        .update_status(&goal.id, GoalStatus::Paused)
        .await
        .unwrap();
    let paused = store.get(&goal.id).await.unwrap().unwrap();
    assert!(!codegg_core::goal::runtime::should_continue(&paused).should_continue);

    store
        .update_status(&goal.id, GoalStatus::Active)
        .await
        .unwrap();
    store.clear_active_for_session("sess-stop").await.unwrap();
    let cancelled = store.get(&goal.id).await.unwrap().unwrap();
    assert_eq!(cancelled.status, GoalStatus::Cancelled);
    assert!(!codegg_core::goal::runtime::should_continue(&cancelled).should_continue);

    // Replacement pauses the predecessor; the stale id must not continue.
    let first = store
        .create_active(
            "sess-stop",
            "proj-stop",
            "First",
            "Old objective",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    let second = store
        .create_active(
            "sess-stop",
            "proj-stop",
            "Second",
            "New objective",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    assert_ne!(first.id, second.id);
    let reloaded_first = store.get(&first.id).await.unwrap().unwrap();
    assert_eq!(reloaded_first.status, GoalStatus::Paused);
}

#[tokio::test(flavor = "current_thread")]
async fn fake_job_prose_never_produces_verified_wait() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-fake", "proj-fake").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let mut goal = store
        .create_active(
            "sess-fake",
            "proj-fake",
            "Fake wait goal",
            "Do not trust prose",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    // Model prose could claim a job id in progress_summary/open questions,
    // but the assembler only accepts durable store records.
    goal.progress_summary = "waiting on job-fake-123".to_string();
    goal.open_questions = vec!["waiting on job-fake-123".to_string()];
    let todo = todo_state_with(1, 0);
    let evidence = assemble(&pool, "sess-fake", &goal, &todo).await;
    assert!(evidence.executions.is_empty());
    assert!(codegg_core::goal::progress::live_wait_handle(&evidence).is_none());
    let disposition = assess_goal_continuation(Some(&evidence), &evidence);
    assert!(!matches!(
        disposition,
        GoalProgressDisposition::VerifiedWait { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn fingerprint_and_diagnostics_exclude_hidden_reasoning() {
    let pool = test_pool().await;
    ensure_test_session(&pool, "sess-clean", "proj-clean").await;
    let store = codegg_core::goal::GoalStore::new(pool.clone());
    let mut goal = store
        .create_active(
            "sess-clean",
            "proj-clean",
            "Clean goal",
            "Keep diagnostics bounded",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    goal.progress_summary = "secret chain-of-thought: hunter2".to_string();
    goal.open_questions = Vec::new();
    let todo = todo_state_with(1, 0);
    let evidence = assemble(&pool, "sess-clean", &goal, &todo).await;
    let fingerprint = goal_continuation_fingerprint(&evidence);
    assert!(!fingerprint.contains("hunter2"));
    assert!(!fingerprint.contains("chain-of-thought"));
    let report =
        codegg_core::goal::runtime::build_awaiting_user_blocker_report(&goal, &fingerprint, 3);
    assert!(!report.contains("hunter2"));
    let failure = disposition_for_evidence_failure(Some(&fingerprint));
    assert!(matches!(
        failure,
        GoalProgressDisposition::NoProgress {
            reason: GoalNoProgressReason::EvidenceLoadFailed,
            ..
        }
    ));
    assert!(!failure.is_progress());
}
