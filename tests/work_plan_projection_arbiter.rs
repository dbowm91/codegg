//! Long-horizon M003 integration: WorkPlan tools, Todo sync, and arbiter.
//!
//! Deterministic store-backed coverage for the model-facing projection,
//! host-evidence authority, Goal-bound gating, Todo one-way contract, and
//! ordinary-turn completion arbitration without a live provider.

use codegg::tool::Tool;
use codegg_core::goal::GoalStore;
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, ExecutionTarget, IdempotencyClass,
    JobPayload, JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
    SqliteJobStore,
};
use codegg_core::work_plan::{
    NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind,
    WorkEvidenceRef, WorkItemStatus, WorkPlanStore,
};
use codegg_core::workspace::WorkspaceId;

async fn test_pool() -> sqlx::SqlitePool {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let url = format!(
        "file:workplan_src_it_{}?mode=memory&cache=shared",
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
    codegg::session::schema::migrate(&pool)
        .await
        .expect("migrate");
    pool
}

async fn ensure_session(pool: &sqlx::SqlitePool, session_id: &str, project_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) VALUES (?, ?, '[]', ?, ?)",
    )
    .bind(project_id)
    .bind("/tmp/workplan")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) VALUES (?, ?, 'test', '/tmp/workplan', 'Test', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
}

fn plan_input(session: &str, project: &str) -> NewWorkPlan {
    NewWorkPlan {
        session_id: session.to_string(),
        project_id: project.to_string(),
        origin_turn_id: Some("turn-1".to_string()),
        goal_id: None,
        objective: "implement projection and arbitration".to_string(),
        origin_provenance: "turn:turn-1".to_string(),
        current_phase: Some("projection".to_string()),
    }
}

fn item_input(description: &str) -> NewWorkItem {
    NewWorkItem {
        parent_item_id: None,
        dependencies: vec![],
        status: WorkItemStatus::Pending,
        description: description.to_string(),
        acceptance: vec![WorkAcceptance {
            description: "criterion".to_string(),
            disposition: WorkAcceptanceDisposition::Unmet,
            note: None,
        }],
        evidence: vec![],
        owner_run_id: None,
        owner_job_id: None,
        blocker: None,
        next_action: Some("start".to_string()),
    }
}

async fn insert_job_with_state(
    pool: &sqlx::SqlitePool,
    session_id: &str,
    state: AttemptState,
) -> String {
    let jobs = SqliteJobStore::new(pool.clone());
    let job = jobs
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("/tmp/workplan"),
            session_id: Some(session_id.to_string()),
            turn_id: None,
            kind: codegg_core::jobs::JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::Test {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: Some("/tmp/workplan".into()),
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
    jobs.finish_attempt(AttemptCompletion {
        attempt_id: attempt.attempt_id,
        state,
        error: None,
        run_id: None,
    })
    .await
    .unwrap();
    job.job_id.as_str().to_string()
}

#[tokio::test(flavor = "current_thread")]
async fn unfinished_plan_blocks_completion_and_complete_plan_exits() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    store.add_item(&plan.id, item_input("first")).await.unwrap();

    // Early-final attempt: the arbiter must require continuation.
    let decision = codegg::work_plan_arbiter::check_ordinary_completion(&pool, "sess-1")
        .await
        .unwrap()
        .expect("active plan");
    assert!(
        decision.should_continue(),
        "unfinished plan must continue, got {decision:?}"
    );

    // Complete the item through the host path (Satisfied acceptance set by
    // the host, never by model assertion), then close the turn.
    let items = store.list_items(&plan.id).await.unwrap();
    let item = &items[0];
    let patch = codegg_core::work_plan::model::WorkItemPatch {
        acceptance: Some(vec![WorkAcceptance {
            description: "criterion".to_string(),
            disposition: WorkAcceptanceDisposition::Satisfied,
            note: None,
        }]),
        ..Default::default()
    };
    let (_, updated) = store
        .update_item(&item.id, item.revision, patch)
        .await
        .unwrap();
    let (_, started) = store
        .transition_item(
            &updated.id,
            updated.revision,
            WorkItemStatus::InProgress,
            None,
            None,
        )
        .await
        .unwrap();
    store
        .transition_item(
            &started.id,
            started.revision,
            WorkItemStatus::Completed,
            None,
            None,
        )
        .await
        .unwrap();

    let assessment = codegg::work_plan_arbiter::assess_active_plan(&pool, "sess-1")
        .await
        .unwrap()
        .expect("active plan");
    assert!(matches!(
        assessment.2,
        codegg_core::work_plan::WorkPlanCompletionAssessment::Complete { .. }
    ));
    let decision = codegg::work_plan_arbiter::decide_from_assessment(&assessment.2);
    assert_eq!(
        decision,
        codegg::work_plan_arbiter::ArbiterDecision::AllowCompletion
    );

    // Turn-end close transitions the turn-scoped plan exactly once.
    let closed = codegg::work_plan_arbiter::maybe_complete_plan_on_turn_end(
        &pool,
        &assessment.0,
        &assessment.2,
        false,
    )
    .await
    .unwrap();
    assert!(closed);
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    assert_eq!(
        plan.status,
        codegg_core::work_plan::WorkPlanStatus::Completed
    );
}

#[tokio::test(flavor = "current_thread")]
async fn budget_expiry_preserves_remaining_state() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    let assessment = codegg_core::work_plan::assess_work_plan(
        &plan,
        &items,
        &codegg_core::work_plan::WorkPlanEvidenceSnapshot::empty(),
    );
    // Empty plan reads Complete, but budget expiry must still preserve (no
    // transition when the caller reports expiry).
    let _ = assessment;
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    store
        .add_item(&plan.id, item_input("remaining"))
        .await
        .unwrap();
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    let evidence = codegg::work_plan_evidence::assemble(&pool, &items)
        .await
        .unwrap();
    let assessment = codegg_core::work_plan::assess_work_plan(&plan, &items, &evidence);
    let closed =
        codegg::work_plan_arbiter::maybe_complete_plan_on_turn_end(&pool, &plan, &assessment, true)
            .await
            .unwrap();
    assert!(!closed, "budget expiry must preserve remaining state");
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    assert_eq!(plan.status, codegg_core::work_plan::WorkPlanStatus::Active);
}

#[tokio::test(flavor = "current_thread")]
async fn goal_bound_plan_requires_both_workplan_and_verifier() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active("sess-1", "proj-1", "Goal", "Do work", None, None, vec![])
        .await
        .unwrap();

    // Bind a plan with actionable work.
    let store = WorkPlanStore::new(pool.clone());
    let mut input = plan_input("sess-1", "proj-1");
    input.goal_id = Some(goal.id.clone());
    let plan = store.create_active(input).await.unwrap();
    store
        .add_item(&plan.id, item_input("remaining"))
        .await
        .unwrap();

    // Passing host test exists, but the bound plan still blocks completion.
    insert_job_with_state(&pool, "sess-1", AttemptState::Completed).await;
    // Label the job for the goal so the verifier sees it.
    {
        let jobs = SqliteJobStore::new(pool.clone());
        let records = jobs
            .list_job_records(codegg_core::jobs::store::JobStoreQuery {
                kinds: vec![codegg_core::jobs::JobKind::Test],
                session_id: Some("sess-1".to_string()),
                limit: Some(8),
                ..Default::default()
            })
            .await
            .unwrap();
        for record in records {
            let mut labels = std::collections::HashMap::new();
            labels.insert(
                codegg_core::jobs::GOAL_PROVENANCE_LABEL_KEY.to_string(),
                goal.id.clone(),
            );
            jobs.set_job_labels(&record.job_id, labels).await.unwrap();
        }
    }

    let gate = codegg::work_plan_arbiter::check_goal_completion_gate(&pool, &goal.id)
        .await
        .unwrap();
    assert!(
        matches!(
            gate,
            codegg::work_plan_arbiter::ArbiterDecision::ContinueWithPrompt(_)
        ),
        "bound actionable plan must gate Goal completion, got {gate:?}"
    );

    // The Goal tool surfaces the WorkPlan gate without completing the Goal.
    let tool =
        codegg::tool::goal::GoalRequestCompletionTool::new(pool.clone(), "sess-1".to_string());
    let result = tool
        .execute(serde_json::json!({
            "evidence": "all done",
            "tests_run": ["cargo test"]
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(parsed["accepted"], false);
    assert_eq!(parsed["verdict"], "not_met");
    assert!(parsed.get("work_plan_id").is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn failed_test_remains_unmet_actionable() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let job_id = insert_job_with_state(&pool, "sess-1", AttemptState::Failed).await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    let evidenced = NewWorkItem {
        status: WorkItemStatus::InProgress,
        evidence: vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: job_id,
            detail: None,
        }],
        ..item_input("tested")
    };
    store.add_item(&plan.id, evidenced).await.unwrap();
    let decision = codegg::work_plan_arbiter::check_ordinary_completion(&pool, "sess-1")
        .await
        .unwrap()
        .expect("active plan");
    assert!(
        matches!(
            decision,
            codegg::work_plan_arbiter::ArbiterDecision::ContinueWithPrompt(_)
        ),
        "failed canonical test must stay actionable, got {decision:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn in_flight_job_yields_wait_behavior() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    // Create a live running job directly.
    let jobs = SqliteJobStore::new(pool.clone());
    let job = jobs
        .create_job(NewJob {
            workspace_id: WorkspaceId::new_unchecked("/tmp/workplan"),
            session_id: Some("sess-1".to_string()),
            turn_id: None,
            kind: codegg_core::jobs::JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: JobPayload::Test {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: Some("/tmp/workplan".into()),
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
    let live_id = job.job_id.as_str().to_string();

    let in_flight = NewWorkItem {
        status: WorkItemStatus::InProgress,
        acceptance: vec![],
        evidence: vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: live_id,
            detail: None,
        }],
        ..item_input("waiting")
    };
    store.add_item(&plan.id, in_flight).await.unwrap();
    let decision = codegg::work_plan_arbiter::check_ordinary_completion(&pool, "sess-1")
        .await
        .unwrap()
        .expect("active plan");
    assert!(
        matches!(
            decision,
            codegg::work_plan_arbiter::ArbiterDecision::WaitForHandle { .. }
        ),
        "live job must yield wait behavior, got {decision:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn user_judgment_returns_control_without_fabrication() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    let judgment = NewWorkItem {
        acceptance: vec![WorkAcceptance {
            description: "owner signs off".to_string(),
            disposition: WorkAcceptanceDisposition::RequiresUserJudgment,
            note: None,
        }],
        ..item_input("judgment")
    };
    store.add_item(&plan.id, judgment).await.unwrap();
    let decision = codegg::work_plan_arbiter::check_ordinary_completion(&pool, "sess-1")
        .await
        .unwrap()
        .expect("active plan");
    assert!(
        matches!(
            decision,
            codegg::work_plan_arbiter::ArbiterDecision::NeedsUserJudgment(_)
        ),
        "judgment-only plan must return control, got {decision:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn work_plan_tools_enforce_bounds_and_authority() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    let (_, item) = store.add_item(&plan.id, item_input("work")).await.unwrap();

    // Bounded read defaults to the current slice.
    let get = codegg::tool::work_plan::WorkPlanGetTool::new(pool.clone(), "sess-1".to_string());
    let output = get.execute(serde_json::json!({})).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["plan_id"], plan.id.as_str());
    assert!(parsed["items"].as_array().unwrap().len() <= 8);

    // Forbidden host-owned fields are rejected loudly.
    let update =
        codegg::tool::work_plan::WorkPlanUpdateItemTool::new(pool.clone(), "sess-1".to_string());
    let err = update
        .execute(serde_json::json!({
            "item_id": item.id.as_str(),
            "expected_revision": item.revision,
            "acceptance": [{"description": "x", "disposition": "satisfied"}]
        }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("host-owned"));

    // Model cannot complete without host evidence.
    let err = update
        .execute(serde_json::json!({
            "item_id": item.id.as_str(),
            "expected_revision": item.revision,
            "status": "completed"
        }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("host-owned evidence"));

    // Stale revision fails without mutation.
    let err = update
        .execute(serde_json::json!({
            "item_id": item.id.as_str(),
            "expected_revision": item.revision + 99,
            "next_action": "stale write"
        }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("stale"));

    // Child cannot update an item assigned to a different run.
    let owned_input = NewWorkItem {
        owner_run_id: Some("run-owner".to_string()),
        ..item_input("owned")
    };
    let (_, owned) = store.add_item(&plan.id, owned_input).await.unwrap();
    let err = update
        .execute(serde_json::json!({
            "item_id": owned.id.as_str(),
            "expected_revision": owned.revision,
            "next_action": "sibling write",
            "caller_run_id": "run-other"
        }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("different run"));

    // Cross-session reads are rejected.
    let cross = codegg::tool::work_plan::WorkPlanGetTool::new(pool.clone(), "other".to_string());
    let err = cross.execute(serde_json::json!({})).await.unwrap_err();
    assert!(
        err.to_string().contains("no active work plan")
            || err.to_string().contains("different session")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn todo_completed_alone_never_completes_host_only_item() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    let (_, item) = store.add_item(&plan.id, item_input("work")).await.unwrap();
    let todo_id = codegg_core::work_plan::todo_id_for_work_item(&item);
    let todo = codegg_core::task_state::TodoItem {
        id: todo_id,
        content: "work".to_string(),
        status: codegg_core::task_state::TodoStatus::Completed,
        priority: codegg_core::task_state::TodoPriority::Medium,
        blocker: None,
    };
    let translated =
        codegg::work_plan_todo_sync::try_translate_todo_feedback(&pool, "sess-1", &[todo]).await;
    assert_eq!(translated, 0);
    let current = store.get_item(&item.id).await.unwrap().unwrap();
    assert_ne!(current.status, WorkItemStatus::Completed);
}

#[tokio::test(flavor = "current_thread")]
async fn work_plan_absent_session_uses_legacy_behavior() {
    let pool = test_pool().await;
    ensure_session(&pool, "sess-1", "proj-1").await;
    let ordinary = codegg::work_plan_arbiter::check_ordinary_completion(&pool, "sess-1")
        .await
        .unwrap();
    assert!(ordinary.is_none());
    let gate = codegg::work_plan_arbiter::check_goal_completion_gate(&pool, "no-goal")
        .await
        .unwrap();
    assert_eq!(
        gate,
        codegg::work_plan_arbiter::ArbiterDecision::AllowCompletion
    );
}
