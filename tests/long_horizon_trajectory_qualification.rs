//! Long-horizon M005 — trajectory and recovery qualification.
//!
//! Deterministic end-to-end qualification of the M001–M004 contracts without
//! a live provider or network: one representative large plan is driven across
//! eight context transitions (repeated compaction plus one fresh-epoch reset),
//! while focused scenarios cover Goal-bound trajectories, premature-final
//! attempts, verified waits, no-progress recovery, restart/recovery,
//! contention/cancellation, security negatives, migration compatibility, and
//! bounded-resource assertions.
//!
//! Requirement-to-test map (plan §10):
//!
//! - ordinary large plan full trajectory → `ordinary_large_plan_full_trajectory`
//! - Goal-bound large plan full trajectory → `goal_bound_large_plan_full_trajectory`
//! - premature final → continuation → `premature_final_requires_continuation_then_terminates`
//! - verified wait → same handle → completion → `verified_wait_same_handle_then_completion`
//! - no-progress → replan → AwaitingUser → `no_progress_replans_then_awaits_user`
//! - passing/failed host test evidence → `passing_and_failed_host_test_evidence`
//! - restart after plan mutation → `restart_after_plan_mutation`
//! - restart during live job → `restart_during_live_job`
//! - restart after job completion before model sees result →
//!   `restart_after_job_completion_before_model_sees_result`
//! - restart around prepared/installed boundary →
//!   `restart_around_prepared_installed_boundary`
//! - steering vs stale checkpoint/plan update → `steering_vs_stale_plan_update`
//! - cancel vs completion assessment → `cancel_vs_completion_assessment`
//! - child update vs parent transition → `child_update_vs_parent_transition`
//! - model text cannot forge completion → `model_text_cannot_forge_completion`
//! - no hidden content in diagnostics → `diagnostics_carry_no_hidden_content`
//! - epoch preserves execution policy → `context_epoch_preserves_execution_policy`
//! - pre-WorkPlan path → `legacy_session_without_workplan_uses_legacy_behavior`
//! - prior checkpoint version → `prior_checkpoint_version_remains_readable`
//! - eight-transition representative trajectory →
//!   `eight_transition_trajectory_with_epoch_reset`
//! - bounded performance/resource assertions →
//!   `projections_and_diagnostics_stay_bounded`
//! - missing recovery artifact degrades → `missing_recovery_artifact_degrades_boundedly`
//! - no second compaction/workflow owner → `no_second_compaction_or_workflow_owner`

mod common;

use codegg::agent::context_frame::ContextLedgerState;
use codegg::context::compaction::{
    compact_context, count_continuation_frames, validate_message_invariants, CompactionStatus,
    ContextCompactionRequest,
};
use codegg::context::continuation::{assemble_continuation_snapshot, ContinuationAssemblyInput};
use codegg::context::epoch::{
    build_epoch_started_event, build_fresh_epoch_messages, build_handoff_text,
    validate_fresh_epoch_messages, ContextEpochInputs, ContextEpochLineage, ContextEpochPolicy,
    ContextEpochTrigger, FreshEpochGoal, FreshEpochInputs, MAX_EPOCH_HANDOFF_CHARS,
};
use codegg::context::evidence::{
    persist_selected_evidence, select_materializable_evidence, verify_evidence_artifacts,
};
use codegg::context::rollover;
use codegg::context::InMemoryArtifactStore;
use codegg::provider::{ContentPart, Message, ProviderRequestContext, ToolCall};
use codegg::tool::goal::GoalRequestCompletionTool;
use codegg::tool::work_plan::{WorkPlanGetTool, WorkPlanUpdateItemTool};
use codegg::tool::Tool;
use codegg::work_plan_arbiter::{
    assess_active_plan, build_required_work_message, check_goal_completion_gate,
    check_ordinary_completion, decide_from_assessment, maybe_complete_plan_on_turn_end,
    ArbiterDecision, MAX_ARBITER_MESSAGE_CHARS,
};
use codegg::work_plan_evidence;
use codegg::work_plan_todo_sync;
use codegg_core::goal::progress::{
    assess_goal_continuation, goal_continuation_fingerprint,
    live_wait_handle as goal_live_wait_handle, stagnation_step, GoalNoProgressReason,
    GoalProgressDisposition, GoalStagnationStep, MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER,
};
use codegg_core::goal::{
    CompletionRequest, GoalCompletionProposal, GoalStatus, GoalStore, GoalVerificationService,
    GoalVerificationVerdict,
};
use codegg_core::jobs::store::JobStoreQuery;
use codegg_core::jobs::{
    AttemptCompletion, AttemptState, DaemonGeneration, IdempotencyClass, JobId, JobKind,
    JobPayload, JobPriority, JobSource, JobStore, NewJob, ResourceRequest, RetryPolicy,
    SqliteJobStore, GOAL_PROVENANCE_LABEL_KEY,
};
use codegg_core::model_profile::resolve::infer_builtin_profile;
use codegg_core::session::continuation::{
    ContinuationCheckpoint, ContinuationCheckpointPayload, ContinuationCheckpointStatus,
    ContinuationCheckpointStore,
};
use codegg_core::task_state::{TodoItem, TodoPriority, TodoState, TodoStatus};
use codegg_core::work_plan::model::{WorkItemPatch, WorkPlanError};
use codegg_core::work_plan::projection::{MAX_PROJECTION_BYTES, MAX_PROJECTION_ITEMS};
use codegg_core::work_plan::{
    actionable_items, assess_work_plan, build_checkpoint_provenance, decide_epoch,
    decision_diagnostic, epoch_supported_for_profile, item_is_satisfied, project_work_plan,
    provenance_from_body, revalidate_against_current, todo_id_for_work_item, HostEvidenceStatus,
    NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind,
    WorkEvidenceRef, WorkItem, WorkItemId, WorkItemStatus, WorkPlanCompletionAssessment,
    WorkPlanEvidenceSnapshot, WorkPlanProjectionParams, WorkPlanStatus, WorkPlanStore,
};
use codegg_core::workspace::WorkspaceId;
use sqlx::SqlitePool;

// ── Shared deterministic fixtures ───────────────────────────────────────────

async fn seed_session(pool: &SqlitePool, session_id: &str, project_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, worktree, sandboxes, time_created, time_updated) \
         VALUES (?, ?, '[]', ?, ?)",
    )
    .bind(project_id)
    .bind("/tmp/m005")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT OR IGNORE INTO session (id, project_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, ?, 'm005', '/tmp/m005', 'M005', '1', ?, ?)",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .unwrap();
}

async fn open_file_pool(path: &std::path::Path) -> SqlitePool {
    use sqlx::sqlite::SqlitePoolOptions;
    let url = format!("sqlite://{}?mode=rwc", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .expect("connect file sqlite");
    codegg::session::schema::migrate(&pool)
        .await
        .expect("migrate file sqlite");
    pool
}

fn plan_input(session: &str, project: &str, objective: &str, phase: &str) -> NewWorkPlan {
    NewWorkPlan {
        session_id: session.to_string(),
        project_id: project.to_string(),
        origin_turn_id: Some("turn-1".to_string()),
        goal_id: None,
        objective: objective.to_string(),
        origin_provenance: "turn:turn-1".to_string(),
        current_phase: Some(phase.to_string()),
    }
}

fn item_input(description: &str, next_action: &str) -> NewWorkItem {
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
        next_action: Some(next_action.to_string()),
    }
}

fn test_job_spec(session_id: &str, kind: JobKind, payload: JobPayload) -> NewJob {
    NewJob {
        workspace_id: WorkspaceId::new_unchecked("/tmp/m005"),
        session_id: Some(session_id.to_string()),
        turn_id: None,
        kind,
        source: JobSource::Interactive,
        priority: JobPriority::Interactive,
        payload,
        resource_request: ResourceRequest::for_kind(kind),
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
    }
}

fn test_payload() -> JobPayload {
    JobPayload::Test {
        command: "cargo test".into(),
        argv: vec!["cargo".into(), "test".into()],
        cwd: Some("/tmp/m005".into()),
        scope: Some("workspace".into()),
        parent_run_id: None,
    }
}

async fn insert_test_job(
    pool: &SqlitePool,
    session_id: &str,
    finish: Option<AttemptState>,
) -> String {
    let jobs = SqliteJobStore::new(pool.clone());
    let job = jobs
        .create_job(test_job_spec(session_id, JobKind::Test, test_payload()))
        .await
        .unwrap();
    let attempt = jobs
        .begin_attempt(
            &job.job_id,
            &DaemonGeneration::new_unchecked("m005-generation"),
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

async fn insert_subagent_job(
    pool: &SqlitePool,
    session_id: &str,
    finish: Option<AttemptState>,
) -> String {
    let jobs = SqliteJobStore::new(pool.clone());
    let job = jobs
        .create_job(test_job_spec(
            session_id,
            JobKind::Subagent,
            JobPayload::Subagent {
                prompt: "audit checkout flow".to_string(),
                agent: "auditor".to_string(),
                model: None,
                parent_id: None,
                denied_tools: Vec::new(),
                allowed_paths: Vec::new(),
                max_tool_calls: None,
            },
        ))
        .await
        .unwrap();
    let attempt = jobs
        .begin_attempt(
            &job.job_id,
            &DaemonGeneration::new_unchecked("m005-generation"),
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

async fn insert_goal_test_job(
    pool: &SqlitePool,
    session_id: &str,
    goal_id: &str,
    finish: Option<AttemptState>,
) -> String {
    let job_id = insert_test_job(pool, session_id, finish).await;
    let jobs = SqliteJobStore::new(pool.clone());
    let mut labels = std::collections::HashMap::new();
    labels.insert(GOAL_PROVENANCE_LABEL_KEY.to_string(), goal_id.to_string());
    jobs.set_job_labels(&JobId::new_unchecked(job_id.clone()), labels)
        .await
        .unwrap();
    job_id
}

async fn finish_job(pool: &SqlitePool, job_id: &str, state: AttemptState) {
    let jobs = SqliteJobStore::new(pool.clone());
    let jid = JobId::new_unchecked(job_id.to_string());
    let attempts = jobs.list_attempts(&jid).await.unwrap();
    let running = attempts
        .iter()
        .find(|attempt| matches!(attempt.state, AttemptState::Running))
        .expect("running attempt");
    jobs.finish_attempt(AttemptCompletion {
        attempt_id: running.attempt_id.clone(),
        state,
        error: None,
        run_id: None,
    })
    .await
    .unwrap();
}

async fn count_session_jobs(pool: &SqlitePool, session_id: &str) -> usize {
    let jobs = SqliteJobStore::new(pool.clone());
    jobs.list_job_records(JobStoreQuery {
        kinds: vec![JobKind::Test, JobKind::Subagent],
        session_id: Some(session_id.to_string()),
        limit: Some(64),
        ..Default::default()
    })
    .await
    .unwrap()
    .len()
}

/// Host-side completion through the canonical path: a host `Satisfied`
/// acceptance (never model assertion), then `InProgress` → `Completed`.
async fn host_complete_item(store: &WorkPlanStore, item: &WorkItem) -> WorkItem {
    let patch = WorkItemPatch {
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
    let (_, done) = store
        .transition_item(
            &started.id,
            started.revision,
            WorkItemStatus::Completed,
            None,
            None,
        )
        .await
        .unwrap();
    done
}

/// Completion relying solely on canonical `Passed` job evidence (no
/// `Satisfied` acceptance): proves the evidence path satisfies the host floor.
async fn complete_item_with_evidence(store: &WorkPlanStore, item: &WorkItem) -> WorkItem {
    let current = store.get_item(&item.id).await.unwrap().unwrap();
    let started = if current.status == WorkItemStatus::InProgress {
        current
    } else {
        let (_, started) = store
            .transition_item(
                &current.id,
                current.revision,
                WorkItemStatus::InProgress,
                None,
                None,
            )
            .await
            .unwrap();
        started
    };
    let (_, done) = store
        .transition_item(
            &started.id,
            started.revision,
            WorkItemStatus::Completed,
            None,
            None,
        )
        .await
        .unwrap();
    done
}

fn user(text: &str) -> Message {
    Message::User {
        content: vec![ContentPart::Text {
            text: text.to_string().into(),
        }],
    }
}

fn assistant(text: &str) -> Message {
    Message::Assistant {
        content: vec![ContentPart::Text {
            text: text.to_string().into(),
        }],
        tool_calls: vec![],
    }
}

fn tool_pair(call_id: &str, tool_name: &str, result: &str) -> Vec<Message> {
    vec![
        Message::Assistant {
            content: vec![],
            tool_calls: vec![ToolCall {
                id: call_id.to_string().into(),
                name: tool_name.to_string().into(),
                arguments: serde_json::json!({"command": "cargo test"}),
            }],
        },
        Message::Tool {
            tool_call_id: call_id.to_string().into(),
            content: result.to_string().into(),
        },
    ]
}

fn todo_state_with(pending: u32, completed: u32) -> TodoState {
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
    let mut state = TodoState::new();
    state.items = items;
    state.revision = 1;
    state
}

// ── Integration: ordinary large plan full trajectory ────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn ordinary_large_plan_full_trajectory() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-ordinary";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "migrate checkout flow without breaking discounts",
            "phase one",
        ))
        .await
        .unwrap();

    // Three phases with dependencies, a delegated child, and host test
    // evidence (one passing, one failing-then-retried).
    let (_, item_a) = store
        .add_item(
            &plan.id,
            item_input("port discount validation", "edit validator"),
        )
        .await
        .unwrap();
    let passing_job = insert_test_job(&pool, session_id, Some(AttemptState::Completed)).await;
    let (_, item_b) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: passing_job.clone(),
                    detail: None,
                }],
                ..item_input("update checkout tests", "run checkout tests")
            },
        )
        .await
        .unwrap();
    let child_job = insert_subagent_job(&pool, session_id, Some(AttemptState::Completed)).await;
    let (_, item_c) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::DelegatedRun,
                    ref_id: child_job.clone(),
                    detail: None,
                }],
                owner_run_id: Some("run-child-1".to_string()),
                ..item_input("delegated checkout audit", "await child audit")
            },
        )
        .await
        .unwrap();
    let (_, item_d) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                dependencies: vec![item_a.id.clone(), item_b.id.clone(), item_c.id.clone()],
                ..item_input("ship migration", "ship it")
            },
        )
        .await
        .unwrap();

    // Premature final with unfinished required items must continue.
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert!(
        decision.should_continue(),
        "unfinished plan must continue, got {decision:?}"
    );

    // User steering after an early phase: phase pointer plus next action.
    let current = store.get(&plan.id).await.unwrap().unwrap();
    store
        .update_plan_meta(
            &plan.id,
            current.revision,
            Some(Some("phase two".to_string())),
            None,
        )
        .await
        .unwrap();

    // Host completes every item through canonical signals only.
    for item in [&item_a, &item_b, &item_c] {
        let current = store.get_item(&item.id).await.unwrap().unwrap();
        if item.id == item_a.id {
            host_complete_item(&store, &current).await;
        } else {
            complete_item_with_evidence(&store, &current).await;
        }
    }
    let item_d = store.get_item(&item_d.id).await.unwrap().unwrap();
    host_complete_item(&store, &item_d).await;

    // The failing-test counter-case: a failed canonical test stays actionable.
    // (Covered explicitly in `passing_and_failed_host_test_evidence`; here the
    // passing evidence must read as satisfied host evidence.)
    let evidence = work_plan_evidence::assemble(&pool, &store.list_items(&plan.id).await.unwrap())
        .await
        .unwrap();
    assert!(evidence.len() >= 2);

    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert_eq!(decision, ArbiterDecision::AllowCompletion);

    // Turn-end close transitions exactly once; a second close is a no-op so
    // complete plans terminate without extra continuation.
    let (live_plan, _, assessment) = assess_active_plan(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert!(matches!(
        assessment,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(
        maybe_complete_plan_on_turn_end(&pool, &live_plan, &assessment, false)
            .await
            .unwrap()
    );
    let reloaded = store.get(&plan.id).await.unwrap().unwrap();
    assert_eq!(reloaded.status, WorkPlanStatus::Completed);
    // A replayed close carrying the superseded revision conflicts instead of
    // continuing again, so complete plans terminate.
    assert!(
        !maybe_complete_plan_on_turn_end(&pool, &live_plan, &assessment, false)
            .await
            .unwrap(),
        "completed plan must not continue again"
    );

    // Completed work was never re-executed: one attempt each, no extra jobs.
    for item in store.list_items(&plan.id).await.unwrap() {
        assert_eq!(item.status, WorkItemStatus::Completed);
        assert_eq!(item.attempts, 1);
    }
    assert_eq!(count_session_jobs(&pool, session_id).await, 2);
}

// ── Integration: Goal-bound large plan full trajectory ──────────────────────

#[tokio::test(flavor = "current_thread")]
async fn goal_bound_large_plan_full_trajectory() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-goalbound";
    seed_session(&pool, session_id, "proj-m005").await;
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active(
            session_id,
            "proj-m005",
            "Ship migration",
            "migrate checkout flow",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();

    let store = WorkPlanStore::new(pool.clone());
    let mut input = plan_input(
        session_id,
        "proj-m005",
        "migrate checkout flow",
        "phase one",
    );
    input.goal_id = Some(goal.id.clone());
    let plan = store.create_active(input).await.unwrap();
    let mut previous: Option<WorkItem> = None;
    for (description, action) in [
        ("port discount validation", "edit validator"),
        ("update checkout tests", "run tests"),
        ("ship migration", "ship it"),
    ] {
        let mut item = item_input(description, action);
        if let Some(prev) = &previous {
            item.dependencies = vec![prev.id.clone()];
        }
        let (_, created) = store.add_item(&plan.id, item).await.unwrap();
        previous = Some(created);
    }

    // A passing goal-owned test satisfies the verifier, but the bound plan
    // still gates Goal completion while actionable work remains.
    insert_goal_test_job(&pool, session_id, &goal.id, Some(AttemptState::Completed)).await;
    let gate = check_goal_completion_gate(&pool, &goal.id).await.unwrap();
    assert!(
        matches!(gate, ArbiterDecision::ContinueWithPrompt(_)),
        "bound actionable plan must gate Goal completion, got {gate:?}"
    );
    let tool = GoalRequestCompletionTool::new(pool.clone(), session_id.to_string());
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

    // Host completes the chain in dependency order; steering survives.
    let current = store.get(&plan.id).await.unwrap().unwrap();
    store
        .update_plan_meta(
            &plan.id,
            current.revision,
            Some(Some("phase two".to_string())),
            None,
        )
        .await
        .unwrap();
    for item in store.list_items(&plan.id).await.unwrap() {
        host_complete_item(&store, &item).await;
    }
    let gate = check_goal_completion_gate(&pool, &goal.id).await.unwrap();
    assert_eq!(gate, ArbiterDecision::AllowCompletion);

    let reloaded_goal = goals.get(&goal.id).await.unwrap().unwrap();
    let evidence =
        codegg::goal_verification::assemble(&pool, session_id, &goal.id, reloaded_goal.created_at)
            .await
            .unwrap();
    let proposal = GoalCompletionProposal::from_request(CompletionRequest {
        evidence: "done".into(),
        files_changed: vec!["src/lib.rs".into()],
        tests_run: vec!["cargo test".into()],
        remaining_risks: Vec::new(),
    })
    .unwrap();
    let verdict = GoalVerificationService.verify(&reloaded_goal, &proposal, &evidence);
    assert!(
        matches!(verdict, GoalVerificationVerdict::Met { .. }),
        "passing host test must satisfy the verifier, got {verdict:?}"
    );
    let completed = goals
        .complete_if_active(&goal.id, reloaded_goal.revision)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.status, GoalStatus::Complete);
    assert!(!codegg_core::goal::runtime::should_continue(&completed).should_continue);
}

// ── Integration: premature final → continuation ─────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn premature_final_requires_continuation_then_terminates() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-premature";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(session_id, "proj-m005", "do one thing", "solo"))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(&plan.id, item_input("the work", "do it"))
        .await
        .unwrap();

    // Premature final attempt: the arbiter injects a bounded continuation.
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    match &decision {
        ArbiterDecision::ContinueWithPrompt(prompt) => {
            assert!(prompt.contains(item.id.as_str()));
            assert!(prompt.len() <= MAX_ARBITER_MESSAGE_CHARS + 64);
        }
        other => panic!("expected continuation, got {other:?}"),
    }

    let current = store.get_item(&item.id).await.unwrap().unwrap();
    host_complete_item(&store, &current).await;
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert_eq!(decision, ArbiterDecision::AllowCompletion);
}

// ── Integration: verified wait → same handle → completion ───────────────────

#[tokio::test(flavor = "current_thread")]
async fn verified_wait_same_handle_then_completion() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-wait";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "await the suite",
            "waiting",
        ))
        .await
        .unwrap();
    let live_job = insert_test_job(&pool, session_id, None).await;
    let (_, item) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                status: WorkItemStatus::InProgress,
                acceptance: vec![],
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: live_job.clone(),
                    detail: None,
                }],
                ..item_input("run the suite", "poll the same handle")
            },
        )
        .await
        .unwrap();
    let _ = &item;

    // The live canonical handle yields a verified wait naming the same job.
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    match &decision {
        ArbiterDecision::WaitForHandle { handle_id, .. } => {
            assert_eq!(handle_id, &live_job, "wait must name the same handle");
        }
        other => panic!("expected verified wait, got {other:?}"),
    }
    assert_eq!(count_session_jobs(&pool, session_id).await, 1);

    // The handle completes; the model sees the result without any relaunch.
    finish_job(&pool, &live_job, AttemptState::Completed).await;
    assert_eq!(
        count_session_jobs(&pool, session_id).await,
        1,
        "verified wait must not duplicate jobs"
    );
    let current = store.list_items(&plan.id).await.unwrap();
    complete_item_with_evidence(&store, &current[0]).await;
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert_eq!(decision, ArbiterDecision::AllowCompletion);
}

// ── Integration: no-progress → replan → AwaitingUser ────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn no_progress_replans_then_awaits_user() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-stall";
    seed_session(&pool, session_id, "proj-m005").await;
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active(
            session_id,
            "proj-m005",
            "Stalled goal",
            "do something eventually",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    let todo = todo_state_with(1, 0);
    let baseline =
        codegg::goal_continuation::assemble_continuation_evidence(&pool, session_id, &goal, &todo)
            .await
            .unwrap();

    // Three identical boundaries graduate nudge → replan → awaiting-user,
    // far below the emergency continuation cap.
    let mut consecutive: u8 = 0;
    for _ in 0..3 {
        let disposition = assess_goal_continuation(Some(&baseline), &baseline);
        assert!(matches!(
            disposition,
            GoalProgressDisposition::NoProgress {
                reason: GoalNoProgressReason::NoStateChange,
                ..
            }
        ));
        consecutive = consecutive.saturating_add(1);
    }
    assert_eq!(stagnation_step(1), GoalStagnationStep::ContinueWithNudge);
    assert_eq!(stagnation_step(2), GoalStagnationStep::ContinueWithReplan);
    assert_eq!(
        stagnation_step(3),
        GoalStagnationStep::EscalateToAwaitingUser
    );
    assert!(consecutive < 32, "stagnation must stop before the cap");
    assert_eq!(MAX_CONSECUTIVE_NOPROGRESS_BEFORE_AWAITING_USER, 3);

    let updated = goals
        .update_status_if_revision(&goal.id, goal.revision, GoalStatus::AwaitingUser)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.status, GoalStatus::AwaitingUser);
    assert!(!codegg_core::goal::runtime::should_continue(&updated).should_continue);
    // A stale escalation based on the superseded revision is rejected.
    assert!(goals
        .update_status_if_revision(&goal.id, goal.revision, GoalStatus::Paused)
        .await
        .unwrap()
        .is_none());
}

// ── Integration: passing/failed host test evidence ──────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn passing_and_failed_host_test_evidence() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-evidence";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "trust host tests",
            "tests",
        ))
        .await
        .unwrap();

    // Passing canonical test: a Completed item with Passed evidence is done.
    let passing_job = insert_test_job(&pool, session_id, Some(AttemptState::Completed)).await;
    let (_, passing_item) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                status: WorkItemStatus::InProgress,
                acceptance: vec![],
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: passing_job.clone(),
                    detail: None,
                }],
                ..item_input("passing suite", "record pass")
            },
        )
        .await
        .unwrap();
    complete_item_with_evidence(&store, &passing_item).await;

    // Failed canonical test: the item stays actionable, never success.
    let failing_job = insert_test_job(&pool, session_id, Some(AttemptState::Failed)).await;
    let (_, failing_item) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                status: WorkItemStatus::InProgress,
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: failing_job.clone(),
                    detail: None,
                }],
                ..item_input("failing suite", "fix the failure")
            },
        )
        .await
        .unwrap();
    let _ = &failing_item;

    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert!(
        matches!(decision, ArbiterDecision::ContinueWithPrompt(_)),
        "failed host test must stay actionable, got {decision:?}"
    );

    // Forged completion: model-claimed test text with no durable job is
    // `Unavailable` evidence and never satisfies the item.
    let snapshot = work_plan_evidence::assemble(&pool, &store.list_items(&plan.id).await.unwrap())
        .await
        .unwrap();
    assert_eq!(
        snapshot.lookup(WorkEvidenceKind::TestJob, "job-does-not-exist"),
        HostEvidenceStatus::Unavailable
    );
    let forged = WorkItem {
        evidence: vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job-does-not-exist".to_string(),
            detail: None,
        }],
        ..store.get_item(&passing_item.id).await.unwrap().unwrap()
    };
    assert!(!item_is_satisfied(&forged, &snapshot));
    let plan_now = store.get(&plan.id).await.unwrap().unwrap();
    let mut forged_items = store.list_items(&plan.id).await.unwrap();
    for item in forged_items.iter_mut() {
        if item.id == passing_item.id {
            *item = forged.clone();
        }
    }
    let assessment = assess_work_plan(&plan_now, &forged_items, &snapshot);
    assert!(
        !matches!(assessment, WorkPlanCompletionAssessment::Complete { .. }),
        "forged evidence must not read as complete"
    );
}

// ── Restart: after plan mutation (file-backed reopen) ───────────────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_after_plan_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("m005-restart-mutation.db");
    let pool = open_file_pool(&db_path).await;
    seed_session(&pool, "sess-m005-rm", "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            "sess-m005-rm",
            "proj-m005",
            "survive daemon restart",
            "phase one",
        ))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(&plan.id, item_input("durable work", "do it"))
        .await
        .unwrap();
    let stale_plan_revision = plan.revision;
    let stale_item_revision = item.revision;
    // Mutate, then restart: the mutation must survive with stable revisions.
    let current = store.get(&plan.id).await.unwrap().unwrap();
    store
        .update_plan_meta(
            &plan.id,
            current.revision,
            Some(Some("phase two".to_string())),
            None,
        )
        .await
        .unwrap();
    let mutated = store.get(&plan.id).await.unwrap().unwrap();
    let plan_id = plan.id.clone();
    let item_id = item.id.clone();
    pool.close().await;
    drop(pool);

    let pool = open_file_pool(&db_path).await;
    let store = WorkPlanStore::new(pool.clone());
    let reloaded = store.get(&plan_id).await.unwrap().expect("plan survives");
    assert_eq!(reloaded.revision, mutated.revision);
    assert_eq!(reloaded.current_phase.as_deref(), Some("phase two"));
    assert_eq!(reloaded.status, WorkPlanStatus::Active);
    let items = store.list_items(&plan_id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, item_id);
    assert_eq!(items[0].status, WorkItemStatus::Pending);
    // Stale writers from before the restart cannot overwrite newer progress.
    assert!(matches!(
        store
            .update_plan_meta(
                &plan_id,
                stale_plan_revision,
                Some(Some("stale".to_string())),
                None
            )
            .await,
        Err(WorkPlanError::Conflict { .. })
    ));
    let live = store.get_item(&item_id).await.unwrap().unwrap();
    store
        .update_item(
            &item_id,
            live.revision,
            WorkItemPatch {
                next_action: Some(Some("post-restart note".to_string())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .update_item(
                &item_id,
                stale_item_revision,
                WorkItemPatch {
                    next_action: Some(Some("stale write".to_string())),
                    ..Default::default()
                }
            )
            .await,
        Err(WorkPlanError::Conflict { .. })
    ));
    let current = store.get_item(&item_id).await.unwrap().unwrap();
    assert_eq!(current.next_action.as_deref(), Some("post-restart note"));
}

// ── Restart: during live job (file-backed reopen) ───────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_during_live_job() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("m005-restart-live.db");
    let pool = open_file_pool(&db_path).await;
    let session_id = "sess-m005-rl";
    seed_session(&pool, session_id, "proj-m005").await;
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active(
            session_id,
            "proj-m005",
            "Live goal",
            "await the suite across restart",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    let live_job = insert_goal_test_job(&pool, session_id, &goal.id, None).await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "await the suite",
            "waiting",
        ))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                status: WorkItemStatus::InProgress,
                acceptance: vec![],
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: live_job.clone(),
                    detail: None,
                }],
                ..item_input("run the suite", "poll the same handle")
            },
        )
        .await
        .unwrap();
    let plan_id = plan.id.clone();
    let item_id = item.id.clone();
    let goal_id = goal.id.clone();
    pool.close().await;
    drop(pool);

    // Reopen: the live job is still live, the item is still in-progress with
    // identical attempts, and nothing was relaunched by the restart.
    let pool = open_file_pool(&db_path).await;
    assert_eq!(count_session_jobs(&pool, session_id).await, 1);
    let store = WorkPlanStore::new(pool.clone());
    let items = store.list_items(&plan_id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, item_id);
    assert_eq!(items[0].status, WorkItemStatus::InProgress);
    assert_eq!(items[0].attempts, 0);
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    match decision {
        ArbiterDecision::WaitForHandle { handle_id, .. } => assert_eq!(handle_id, live_job),
        other => panic!("live job must still verify wait, got {other:?}"),
    }
    // Goal continuation reassembles the same verified wait after restart.
    let goals = GoalStore::new(pool.clone());
    let reloaded_goal = goals.get(&goal_id).await.unwrap().unwrap();
    assert!(reloaded_goal.is_active());
    let todo = todo_state_with(1, 0);
    let evidence = codegg::goal_continuation::assemble_continuation_evidence(
        &pool,
        session_id,
        &reloaded_goal,
        &todo,
    )
    .await
    .unwrap();
    let second = assess_goal_continuation(Some(&evidence), &evidence);
    match second {
        GoalProgressDisposition::VerifiedWait { handle, .. } => {
            assert_eq!(handle.job_id(), live_job);
        }
        other => panic!("expected verified wait after restart, got {other:?}"),
    }
}

// ── Restart: job completes before the model sees the result ─────────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_after_job_completion_before_model_sees_result() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("m005-restart-race.db");
    let pool = open_file_pool(&db_path).await;
    let session_id = "sess-m005-rc";
    seed_session(&pool, session_id, "proj-m005").await;
    let live_job = insert_test_job(&pool, session_id, None).await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "catch the result",
            "waiting",
        ))
        .await
        .unwrap();
    store
        .add_item(
            &plan.id,
            NewWorkItem {
                status: WorkItemStatus::InProgress,
                acceptance: vec![],
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: live_job.clone(),
                    detail: None,
                }],
                ..item_input("run the suite", "poll the same handle")
            },
        )
        .await
        .unwrap();
    // The handle reaches terminal success while the model is not observing;
    // restart before the model sees the result.
    finish_job(&pool, &live_job, AttemptState::Completed).await;
    let plan_id = plan.id.clone();
    pool.close().await;
    drop(pool);

    let pool = open_file_pool(&db_path).await;
    assert_eq!(
        count_session_jobs(&pool, session_id).await,
        1,
        "completion must not duplicate the job"
    );
    let store = WorkPlanStore::new(pool.clone());
    let items = store.list_items(&plan_id).await.unwrap();
    // Restart does not convert the ambiguous in-progress item into replay or
    // success on its own; the host completes it from canonical evidence.
    assert_eq!(items[0].status, WorkItemStatus::InProgress);
    complete_item_with_evidence(&store, &items[0]).await;
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert_eq!(decision, ArbiterDecision::AllowCompletion);
}

// ── Restart: around the prepared/installed checkpoint boundary ──────────────

#[tokio::test(flavor = "current_thread")]
async fn restart_around_prepared_installed_boundary() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("m005-restart-ckpt.db");
    let pool = open_file_pool(&db_path).await;
    let session_id = "sess-m005-rb";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "restart at the boundary",
            "phase one",
        ))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(&plan.id, item_input("boundary work", "hold the line"))
        .await
        .unwrap();
    let _ = &item;
    let items = store.list_items(&plan.id).await.unwrap();

    // Prepare but do not install; restart must ignore the candidate.
    let checkpoints = ContinuationCheckpointStore::new(pool.clone());
    let ledger = ContextLedgerState::new();
    let messages = vec![user("restart at the boundary")];
    let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
        session_id,
        origin_prompt: Some("restart at the boundary"),
        current_user_message: None,
        messages: &messages,
        active_goal: None,
        todos: &[],
        ledger: &ledger,
        security_findings: &[],
        previous_checkpoint: None,
        plan_path: None,
        plan_content: None,
        active_work_plan: Some((&plan, items.as_slice())),
    });
    let prepared_id = "m005-boundary-00";
    let prepared = checkpoints
        .prepare_with_id(
            session_id,
            prepared_id,
            None,
            snapshot.to_payload().expect("payload"),
        )
        .await
        .unwrap();
    assert_eq!(prepared.status, ContinuationCheckpointStatus::Prepared);
    pool.close().await;
    drop(pool);

    let pool = open_file_pool(&db_path).await;
    let checkpoints = ContinuationCheckpointStore::new(pool.clone());
    assert!(
        checkpoints
            .latest_installed(session_id)
            .await
            .unwrap()
            .is_none(),
        "prepared candidate is never resume authority"
    );

    // Install after restart; a later restart preserves lineage and provenance.
    let store = WorkPlanStore::new(pool.clone());
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    let row = checkpoints
        .get(session_id, prepared_id)
        .await
        .unwrap()
        .expect("prepared row survives");
    let event = row
        .build_compacted_event(1, 1, Some(50), Some(20), vec![], vec![], vec![])
        .unwrap();
    let installed = checkpoints
        .install_with_compaction_event(session_id, prepared_id, event)
        .await
        .unwrap();
    assert_eq!(installed.status, ContinuationCheckpointStatus::Installed);
    let provenance = provenance_from_body(&installed.payload.body)
        .unwrap()
        .expect("work plan provenance present");
    assert_eq!(provenance.plan_id, plan.id.as_str());
    assert_eq!(
        rollover::validate_installed_for_restart(&installed, session_id),
        rollover::RestartValidation::Usable
    );
    pool.close().await;
    drop(pool);

    let pool = open_file_pool(&db_path).await;
    let checkpoints = ContinuationCheckpointStore::new(pool.clone());
    let reloaded = checkpoints
        .latest_installed(session_id)
        .await
        .unwrap()
        .expect("installed survives restart");
    assert_eq!(reloaded.id, prepared_id);
    let reloaded_provenance = provenance_from_body(&reloaded.payload.body)
        .unwrap()
        .expect("provenance survives restart");
    assert_eq!(reloaded_provenance.revision, provenance.revision);

    // A newer plan revision supersedes the checkpoint: the stale provenance
    // cannot overwrite current progress.
    let store = WorkPlanStore::new(pool.clone());
    let current = store.get(&plan.id).await.unwrap().unwrap();
    store
        .update_plan_meta(
            &plan.id,
            current.revision,
            Some(Some("phase two".to_string())),
            None,
        )
        .await
        .unwrap();
    let newer = store.get(&plan.id).await.unwrap().unwrap();
    let newer_items = store.list_items(&plan.id).await.unwrap();
    assert!(
        revalidate_against_current(Some(&reloaded_provenance), Some((&newer, &newer_items)))
            .is_err()
    );
    assert!(revalidate_against_current(
        Some(&build_checkpoint_provenance(&newer, &newer_items)),
        Some((&newer, &newer_items))
    )
    .is_ok());
}

// ── Contention: steering vs stale checkpoint/plan update ────────────────────

#[tokio::test(flavor = "current_thread")]
async fn steering_vs_stale_plan_update() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-steer";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "steer without regress",
            "phase one",
        ))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(&plan.id, item_input("steered work", "old next action"))
        .await
        .unwrap();

    // Capture source revisions, then apply later user steering.
    let items = store.list_items(&plan.id).await.unwrap();
    let captured_provenance = build_checkpoint_provenance(&plan, &items);
    let captured_sources = rollover::RolloverSourceRevisions::capture_with_work_plan(
        None,
        None,
        None,
        1,
        None,
        "history-a".to_string(),
        Some(plan.id.as_str().to_string()),
        Some(plan.revision),
    );
    let stale_item_revision = item.revision;

    let current = store.get(&plan.id).await.unwrap().unwrap();
    store
        .update_plan_meta(
            &plan.id,
            current.revision,
            Some(Some("phase two (steered)".to_string())),
            None,
        )
        .await
        .unwrap();
    let steered = store.get(&plan.id).await.unwrap().unwrap();
    let steered_items = store.list_items(&plan.id).await.unwrap();

    // The pre-steering checkpoint provenance is stale and must not install.
    assert!(revalidate_against_current(
        Some(&captured_provenance),
        Some((&steered, &steered_items))
    )
    .is_err());
    let current_sources = rollover::RolloverSourceRevisions::capture_with_work_plan(
        None,
        None,
        None,
        1,
        None,
        "history-a".to_string(),
        Some(steered.id.as_str().to_string()),
        Some(steered.revision),
    );
    assert!(captured_sources.is_stale_against(&current_sources));
    assert_eq!(
        captured_sources.stale_reason(&current_sources),
        Some("active work plan revision changed")
    );
    // A stale item write carrying the superseded revision fails explicitly
    // and leaves current state untouched.
    let live = store.get_item(&item.id).await.unwrap().unwrap();
    store
        .update_item(
            &item.id,
            live.revision,
            WorkItemPatch {
                next_action: Some(Some("fresh note".to_string())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert!(matches!(
        store
            .update_item(
                &item.id,
                stale_item_revision,
                WorkItemPatch {
                    next_action: Some(Some("stale overwrite".to_string())),
                    ..Default::default()
                }
            )
            .await,
        Err(WorkPlanError::Conflict { .. })
    ));
    let current_item = store.get_item(&item.id).await.unwrap().unwrap();
    assert_eq!(current_item.next_action.as_deref(), Some("fresh note"));
}

// ── Cancellation: unfinished items stay unfinished ──────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn cancel_vs_completion_assessment() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-cancel";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "cancellable work",
            "phase one",
        ))
        .await
        .unwrap();
    let (_, first) = store
        .add_item(&plan.id, item_input("finished work", "done"))
        .await
        .unwrap();
    let (_, second) = store
        .add_item(&plan.id, item_input("unfinished work", "pending"))
        .await
        .unwrap();
    let first = store.get_item(&first.id).await.unwrap().unwrap();
    host_complete_item(&store, &first).await;

    // Cancel with unfinished items: cancellation never manufactures success.
    let current = store.get(&plan.id).await.unwrap().unwrap();
    store
        .transition_plan(&plan.id, current.revision, WorkPlanStatus::Cancelled)
        .await
        .unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    let leftover = items.iter().find(|i| i.id == second.id).unwrap();
    assert_ne!(
        leftover.status,
        WorkItemStatus::Completed,
        "cancellation must leave unfinished items unfinished"
    );
    // Further transitions on the cancelled plan fail closed.
    let leftover_revision = leftover.revision;
    assert!(matches!(
        store
            .transition_item(
                &leftover.id,
                leftover_revision,
                WorkItemStatus::Completed,
                None,
                None
            )
            .await,
        Err(WorkPlanError::Terminal(_)) | Err(WorkPlanError::Validation(_))
    ));
    // Turn-end close cannot complete a cancelled plan: the close fails
    // closed and the durable state is preserved as cancelled.
    let cancelled = store.get(&plan.id).await.unwrap().unwrap();
    assert_eq!(cancelled.status, WorkPlanStatus::Cancelled);
    let items = store.list_items(&plan.id).await.unwrap();
    let evidence = work_plan_evidence::assemble(&pool, &items).await.unwrap();
    let assessment = assess_work_plan(&cancelled, &items, &evidence);
    assert!(
        maybe_complete_plan_on_turn_end(&pool, &cancelled, &assessment, false)
            .await
            .is_err(),
        "cancelled plan must not close as completed"
    );
    let preserved = store.get(&plan.id).await.unwrap().unwrap();
    assert_eq!(preserved.status, WorkPlanStatus::Cancelled);
}

// ── Contention: child update vs parent current-item transition ──────────────

#[tokio::test(flavor = "current_thread")]
async fn child_update_vs_parent_transition() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-child";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "delegated work",
            "phase one",
        ))
        .await
        .unwrap();
    let (_, owned) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                owner_run_id: Some("run-child-1".to_string()),
                ..item_input("child work", "child next")
            },
        )
        .await
        .unwrap();

    // The assigned child may report through the model tool with its own run.
    let update = WorkPlanUpdateItemTool::new(pool.clone(), session_id.to_string());
    let current = store.get_item(&owned.id).await.unwrap().unwrap();
    let output = update
        .execute(serde_json::json!({
            "item_id": current.id.as_str(),
            "expected_revision": current.revision,
            "next_action": "child progress note",
            "caller_run_id": "run-child-1"
        }))
        .await
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["item_id"], current.id.as_str());
    let current = store.get_item(&owned.id).await.unwrap().unwrap();
    assert_eq!(current.next_action.as_deref(), Some("child progress note"));

    // A sibling run cannot write to another child's item.
    let current = store.get_item(&owned.id).await.unwrap().unwrap();
    let err = update
        .execute(serde_json::json!({
            "item_id": current.id.as_str(),
            "expected_revision": current.revision,
            "next_action": "sibling overwrite",
            "caller_run_id": "run-other"
        }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("different run"));

    // Parent advances the current-item pointer; the child's replayed stale
    // revision then conflicts instead of overwriting.
    let plan_now = store.get(&plan.id).await.unwrap().unwrap();
    store
        .update_plan_meta(
            &plan.id,
            plan_now.revision,
            None,
            Some(Some(current.id.clone())),
        )
        .await
        .unwrap();
    let stale_revision = current.revision - 1;
    assert!(matches!(
        store
            .update_item(
                &current.id,
                stale_revision,
                WorkItemPatch {
                    next_action: Some(Some("replayed write".to_string())),
                    ..Default::default()
                }
            )
            .await,
        Err(WorkPlanError::Conflict { .. })
    ));
    let get = WorkPlanGetTool::new(pool.clone(), session_id.to_string());
    let output = get.execute(serde_json::json!({})).await.unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(parsed["plan_id"], plan.id.as_str());
}

// ── Security: model text cannot forge completion ────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn model_text_cannot_forge_completion() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-forge";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "unforgeable work",
            "phase one",
        ))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(&plan.id, item_input("guarded work", "do it"))
        .await
        .unwrap();

    // Host-owned acceptance/evidence fields are rejected loudly.
    let update = WorkPlanUpdateItemTool::new(pool.clone(), session_id.to_string());
    let err = update
        .execute(serde_json::json!({
            "item_id": item.id.as_str(),
            "expected_revision": item.revision,
            "acceptance": [{"description": "x", "disposition": "satisfied"}]
        }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("host-owned"));
    let err = update
        .execute(serde_json::json!({
            "item_id": item.id.as_str(),
            "expected_revision": item.revision,
            "status": "completed"
        }))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("host-owned evidence"));

    // A Todo `completed` flag alone never completes a host-gated item.
    let todo = TodoItem {
        id: todo_id_for_work_item(&item),
        content: "guarded work".to_string(),
        status: TodoStatus::Completed,
        priority: TodoPriority::Medium,
        blocker: None,
    };
    let translated =
        work_plan_todo_sync::try_translate_todo_feedback(&pool, session_id, &[todo]).await;
    assert_eq!(translated, 0);
    let current = store.get_item(&item.id).await.unwrap().unwrap();
    assert_ne!(current.status, WorkItemStatus::Completed);

    // A job id that exists only in model prose never becomes a wait handle.
    let goals = GoalStore::new(pool.clone());
    let mut goal = goals
        .create_active(
            session_id,
            "proj-m005",
            "Forge goal",
            "do not trust prose",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    goal.progress_summary = "waiting on job-fake-123".to_string();
    goal.open_questions = vec!["waiting on job-fake-123".to_string()];
    let todo_state = todo_state_with(1, 0);
    let evidence = codegg::goal_continuation::assemble_continuation_evidence(
        &pool,
        session_id,
        &goal,
        &todo_state,
    )
    .await
    .unwrap();
    assert!(evidence.executions.is_empty());
    assert!(goal_live_wait_handle(&evidence).is_none());
}

// ── Security: diagnostics carry no hidden content ───────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn diagnostics_carry_no_hidden_content() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-clean";
    seed_session(&pool, session_id, "proj-m005").await;
    let secret = "sk-live-should-never-appear-9f8e";
    let goals = GoalStore::new(pool.clone());
    let mut goal = goals
        .create_active(
            session_id,
            "proj-m005",
            "Clean goal",
            "keep diagnostics bounded",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    goal.progress_summary = format!("secret chain-of-thought: {secret}");
    let todo = todo_state_with(1, 0);
    let evidence =
        codegg::goal_continuation::assemble_continuation_evidence(&pool, session_id, &goal, &todo)
            .await
            .unwrap();
    let fingerprint = goal_continuation_fingerprint(&evidence);
    assert!(!fingerprint.contains(secret));
    assert!(!fingerprint.contains("chain-of-thought"));

    // Checkpoint diagnostics carry IDs/digests/sizes, never bodies.
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "clean objective",
            "phase one",
        ))
        .await
        .unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    let ledger = ContextLedgerState::new();
    let messages = vec![user("clean objective")];
    let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
        session_id,
        origin_prompt: Some("clean objective"),
        current_user_message: None,
        messages: &messages,
        active_goal: None,
        todos: &[],
        ledger: &ledger,
        security_findings: &[],
        previous_checkpoint: None,
        plan_path: None,
        plan_content: None,
        active_work_plan: Some((&plan, items.as_slice())),
    });
    let payload = snapshot.to_payload().expect("payload");
    let digest = payload.digest().expect("digest");
    assert!(!payload
        .diagnostic_summary(&digest)
        .contains("clean objective"));
    let checkpoints = ContinuationCheckpointStore::new(pool.clone());
    let prepared = checkpoints
        .prepare(session_id, None, payload)
        .await
        .unwrap();
    assert!(!prepared.diagnostic_summary().contains("clean objective"));
    let body = prepared.payload.canonical_json().expect("canonical");
    for forbidden in [
        "reasoning",
        "thinking",
        "chain_of_thought",
        "scratchpad",
        "api_key",
    ] {
        assert!(
            !body.contains(forbidden),
            "checkpoint body must not carry '{forbidden}'"
        );
    }
    let provenance_bytes = serde_json::to_string(&build_checkpoint_provenance(&plan, &items))
        .unwrap()
        .len();
    assert!(provenance_bytes <= 4096);

    // Arbiter control messages stay bounded even for oversized inputs.
    let oversized = build_required_work_message(
        &WorkItemId("wi_probe".to_string()),
        &"x".repeat(2000),
        3,
        Some(&"y".repeat(2000)),
    );
    assert!(oversized.len() <= MAX_ARBITER_MESSAGE_CHARS + 64);
    assert!(!oversized.contains(&"x".repeat(1000)));
}

// ── Security: context epoch preserves execution-policy identity ─────────────

#[test]
fn context_epoch_preserves_execution_policy() {
    let supported = infer_builtin_profile("openai/gpt-5");
    let unsupported = infer_builtin_profile("minimax/minimax-2.7");
    assert!(epoch_supported_for_profile(&supported));
    assert!(!epoch_supported_for_profile(&unsupported));

    // Reconstruction returns messages only: no policy handle enters, so no
    // permission, model-selection, workspace, or budget authority can change.
    let provenance = codegg_core::work_plan::WorkPlanCheckpointProvenance {
        plan_id: "wp_policy".to_string(),
        revision: 4,
        status: "active".to_string(),
        current_phase: Some("phase two".to_string()),
        current_item_id: Some("wi_next".to_string()),
        actionable: vec![codegg_core::work_plan::WorkPlanCheckpointItem {
            id: "wi_next".to_string(),
            revision: 2,
            status: "actionable".to_string(),
            description: "ship migration".to_string(),
            next_action: Some("ship it".to_string()),
        }],
        blocked: vec![],
        source_digest: "sha256:abc".to_string(),
        total_items: 9,
        actionable_count: 3,
        blocked_count: 0,
    };
    let todos = vec!["ship migration".to_string()];
    let handles = vec!["ctx://tool/sess-m005-policy/0/c0".to_string()];
    let steering = vec!["keep discounts backward compatible".to_string()];
    let inputs = FreshEpochInputs {
        system_instructions: "system: canonical instructions",
        objective: "migrate checkout flow",
        goal: Some(FreshEpochGoal {
            goal_id: "goal-1",
            revision: 3,
            objective: "migrate checkout flow",
            current_phase: Some("phase two"),
            next_action: Some("ship it"),
        }),
        work_plan: Some(&provenance),
        todos: &todos,
        continuation_frame_text: "[codegg continuation state v1]\n- Next: ship it",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-policy",
        checkpoint_sequence: 7,
    };
    let messages = build_fresh_epoch_messages(&inputs, &supported).expect("fresh epoch");
    assert!(validate_fresh_epoch_messages(&messages).is_ok());
    assert_eq!(count_continuation_frames(&messages), 1);
    assert!(validate_message_invariants(&messages).is_ok());
    for message in &messages {
        match message {
            Message::Tool { .. } => panic!("fresh epoch carries no tool history"),
            Message::Assistant { content, .. } | Message::User { content } => {
                for part in content {
                    assert!(
                        !matches!(part, ContentPart::Reasoning { .. }),
                        "no hidden reasoning"
                    );
                }
            }
            Message::System { content } => {
                assert!(!content.contains("api_key"));
            }
        }
    }
    let visible = format!("{messages:?}");
    assert!(visible.contains("wi_next"));
    assert!(visible.contains("keep discounts backward compatible"));
    assert!(visible.contains("ctx://tool/sess-m005-policy/0/c0"));

    // Unsupported profiles stay on normal compaction; the default policy
    // never resets, however many compactions have elapsed.
    let todos: Vec<String> = vec![];
    let handles: Vec<String> = vec![];
    let steering: Vec<String> = vec![];
    let fallback_inputs = FreshEpochInputs {
        system_instructions: "system",
        objective: "objective",
        goal: None,
        work_plan: None,
        todos: &todos,
        continuation_frame_text: "frame",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-1",
        checkpoint_sequence: 1,
    };
    let err = build_fresh_epoch_messages(&fallback_inputs, &unsupported).unwrap_err();
    assert!(err.contains("unsupported profile"));
    let default_policy = ContextEpochPolicy::default();
    let decision = decide_epoch(
        &default_policy,
        &ContextEpochInputs::new(99, true, true, true, true),
    );
    assert!(!decision.should_reset);
    assert_eq!(decision.reason_code(), "disabled");
    assert!(!decision_diagnostic(
        &decision,
        &ContextEpochInputs::new(99, true, true, true, true)
    )
    .is_empty());

    // Epoch lineage explains the reset with IDs/reasons only.
    let lineage = ContextEpochLineage {
        epoch_id: "epoch-1".to_string(),
        reason: "phase_boundary".to_string(),
        trigger: "phase_boundary".to_string(),
        checkpoint_id: "ckpt-policy".to_string(),
        checkpoint_sequence: 7,
        work_plan_id: Some("wp_policy".to_string()),
        work_plan_revision: Some(4),
        prior_compaction_count: 4,
        profile_id: "openai/gpt-5".to_string(),
    };
    let line = lineage.bounded_line();
    assert!(line.contains("ckpt-policy"));
    assert!(!line.contains("backward compatible"));
    let enabled = ContextEpochPolicy::enabled_for_handoff(Some(3));
    let reset = decide_epoch(
        &enabled,
        &ContextEpochInputs::new(0, true, false, false, true),
    );
    let event = build_epoch_started_event(session_id_of_lineage(), &reset, &lineage);
    assert_eq!(event.event_type(), "context_epoch:started");
}

fn session_id_of_lineage() -> &'static str {
    "sess-m005-policy"
}

// ── Migration: pre-WorkPlan session path ────────────────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn legacy_session_without_workplan_uses_legacy_behavior() {
    let pool = common::pool::isolated_pool().await;
    seed_session(&pool, "sess-m005-legacy", "proj-m005").await;
    // No active plan: ordinary completion is legacy `None`, Goal gate allows.
    assert!(check_ordinary_completion(&pool, "sess-m005-legacy")
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        check_goal_completion_gate(&pool, "no-goal").await.unwrap(),
        ArbiterDecision::AllowCompletion
    );

    // A pre-M002 database gains empty WorkPlan tables without touching goals.
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active(
            "sess-m005-legacy",
            "proj-m005",
            "title",
            "legacy objective",
            None,
            None,
            Vec::new(),
        )
        .await
        .unwrap();
    sqlx::query("DROP TABLE IF EXISTS work_item")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DROP TABLE IF EXISTS work_plan")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE migration_version SET version = 58 WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    codegg::session::schema::migrate(&pool).await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT version FROM migration_version WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, codegg::storage::STORAGE_LAYOUT_VERSION as i64);
    let reloaded = goals.get(&goal.id).await.unwrap().unwrap();
    assert_eq!(reloaded.objective, "legacy objective");
    assert!(WorkPlanStore::new(pool)
        .active_for_session("sess-m005-legacy")
        .await
        .unwrap()
        .is_none());
}

// ── Migration: prior continuation checkpoint version ────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn prior_checkpoint_version_remains_readable() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-prior";
    seed_session(&pool, session_id, "proj-m005").await;
    // Legacy body with no `work_plan` key (pre-M004).
    let body = serde_json::json!({
        "snapshot_kind": "continuation_snapshot_v1",
        "session_id": session_id,
        "objective": "legacy objective",
        "current_task": "legacy task",
        "semantic": {"constraints": [], "decisions": [], "unresolved_blockers": [], "next_steps": ["legacy next"]},
    });
    let store = ContinuationCheckpointStore::new(pool.clone());
    let prepared = store
        .prepare(
            session_id,
            None,
            ContinuationCheckpointPayload::new(body).expect("payload"),
        )
        .await
        .unwrap();
    let event = prepared
        .build_compacted_event(1, 1, None, None, vec![], vec![], vec![])
        .unwrap();
    let installed = store
        .install_with_compaction_event(session_id, &prepared.id, event)
        .await
        .unwrap();
    assert_eq!(
        rollover::validate_installed_for_restart(&installed, session_id),
        rollover::RestartValidation::Usable
    );
    assert!(provenance_from_body(&installed.payload.body)
        .unwrap()
        .is_none());
    let projection = rollover::render_installed_projection(&installed, None);
    assert!(projection.contains("legacy objective"));
}

// ── Representative eight-transition trajectory with epoch reset ─────────────

const M005_OBJECTIVE: &str = "migrate checkout flow without breaking discounts";
const M005_STEERING: &str = "correction: must keep parser strict and keep tests green";

struct TransitionRow {
    epoch: usize,
    phase: String,
    remaining: usize,
    steering_visible: bool,
}

#[tokio::test(flavor = "current_thread")]
async fn eight_transition_trajectory_with_epoch_reset() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-representative";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let checkpoints = ContinuationCheckpointStore::new(pool.clone());

    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            M005_OBJECTIVE,
            "phase one",
        ))
        .await
        .unwrap();
    let plan_id = plan.id.clone();

    // Large plan: three phases, dependency chains, a delegated child, and
    // passing test evidence plus one live-then-completing test job.
    let (_, item_a) = store
        .add_item(
            &plan.id,
            item_input("port discount validation", "edit discount validator"),
        )
        .await
        .unwrap();
    let live_test_job = insert_test_job(&pool, session_id, None).await;
    let (_, item_b) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::TestJob,
                    ref_id: live_test_job.clone(),
                    detail: None,
                }],
                ..item_input("update checkout tests", "run checkout tests")
            },
        )
        .await
        .unwrap();
    let live_child_job = insert_subagent_job(&pool, session_id, None).await;
    let (_, item_c) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                evidence: vec![WorkEvidenceRef {
                    kind: WorkEvidenceKind::DelegatedRun,
                    ref_id: live_child_job.clone(),
                    detail: None,
                }],
                owner_run_id: Some("run-child-1".to_string()),
                ..item_input("delegated checkout audit", "await child audit")
            },
        )
        .await
        .unwrap();
    let (_, item_d) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                dependencies: vec![item_a.id.clone(), item_b.id.clone(), item_c.id.clone()],
                ..item_input(
                    "fix discount test with strict parser",
                    "apply strict parser",
                )
            },
        )
        .await
        .unwrap();
    let (_, item_e) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                dependencies: vec![item_d.id.clone()],
                ..item_input("migrate to snapshot tests", "write snapshot tests")
            },
        )
        .await
        .unwrap();
    let (_, item_f) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                dependencies: vec![item_d.id.clone()],
                ..item_input("remove new network calls", "drop network calls")
            },
        )
        .await
        .unwrap();
    let (_, item_g) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                dependencies: vec![item_e.id.clone(), item_f.id.clone()],
                ..item_input("ship migration", "ship phase three")
            },
        )
        .await
        .unwrap();
    let (_, item_h) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                dependencies: vec![item_g.id.clone()],
                ..item_input("verify rollout", "verify rollout")
            },
        )
        .await
        .unwrap();
    let (_, _item_i) = store
        .add_item(
            &plan.id,
            NewWorkItem {
                dependencies: vec![item_h.id.clone()],
                ..item_input("close out docs", "update docs")
            },
        )
        .await
        .unwrap();

    let mut messages: Vec<Message> = vec![
        Message::System {
            content: "system prompt".to_string().into(),
        },
        user(&format!(
            "{M005_OBJECTIVE}. Large plan with 9 steps across three phases."
        )),
    ];
    for n in 0..4 {
        messages.push(user(&format!(
            "background context filler {n} {}",
            "x".repeat(900)
        )));
        messages.push(assistant(&format!("acknowledged filler {n}")));
    }
    let mut ledger = ContextLedgerState::new();
    let mut previous: Option<ContinuationCheckpoint> = None;
    let mut rows: Vec<TransitionRow> = Vec::new();

    for epoch in 0..8usize {
        // ── Host-side evolution for this epoch ──
        match epoch {
            1 => {
                let items = store.list_items(&plan_id).await.unwrap();
                let target = items
                    .iter()
                    .find(|i| i.description == "port discount validation")
                    .unwrap()
                    .clone();
                host_complete_item(&store, &target).await;
            }
            2 => {
                // User steering after an early phase plus test completion.
                // The correction arrives as a user message (provider-visible
                // steering) and updates host plan state.
                messages.push(user(M005_STEERING));
                let current = store.get(&plan_id).await.unwrap().unwrap();
                store
                    .update_plan_meta(
                        &plan_id,
                        current.revision,
                        Some(Some("phase one (steered)".to_string())),
                        None,
                    )
                    .await
                    .unwrap();
                let items = store.list_items(&plan_id).await.unwrap();
                let target = items
                    .iter()
                    .find(|i| i.description.starts_with("fix discount"))
                    .unwrap()
                    .clone();
                store
                    .update_item(
                        &target.id,
                        target.revision,
                        WorkItemPatch {
                            next_action: Some(Some("apply strict parser per steering".to_string())),
                            ..Default::default()
                        },
                    )
                    .await
                    .unwrap();
                finish_job(&pool, &live_test_job, AttemptState::Completed).await;
                let items = store.list_items(&plan_id).await.unwrap();
                let target = items
                    .iter()
                    .find(|i| i.description == "update checkout tests")
                    .unwrap()
                    .clone();
                complete_item_with_evidence(&store, &target).await;
            }
            3 => {
                finish_job(&pool, &live_child_job, AttemptState::Completed).await;
                let items = store.list_items(&plan_id).await.unwrap();
                let target = items
                    .iter()
                    .find(|i| i.description == "delegated checkout audit")
                    .unwrap()
                    .clone();
                complete_item_with_evidence(&store, &target).await;
            }
            4 => {
                let current = store.get(&plan_id).await.unwrap().unwrap();
                store
                    .update_plan_meta(
                        &plan_id,
                        current.revision,
                        Some(Some("phase two".to_string())),
                        None,
                    )
                    .await
                    .unwrap();
                let items = store.list_items(&plan_id).await.unwrap();
                let target = items
                    .iter()
                    .find(|i| i.description.starts_with("fix discount"))
                    .unwrap()
                    .clone();
                host_complete_item(&store, &target).await;
            }
            5 => {
                let items = store.list_items(&plan_id).await.unwrap();
                let target = items
                    .iter()
                    .find(|i| i.description == "migrate to snapshot tests")
                    .unwrap()
                    .clone();
                host_complete_item(&store, &target).await;
            }
            6 => {
                let items = store.list_items(&plan_id).await.unwrap();
                let target = items
                    .iter()
                    .find(|i| i.description == "remove new network calls")
                    .unwrap()
                    .clone();
                host_complete_item(&store, &target).await;
            }
            7 => {
                for description in ["ship migration", "verify rollout", "close out docs"] {
                    let items = store.list_items(&plan_id).await.unwrap();
                    let target = items
                        .iter()
                        .find(|i| i.description == description)
                        .unwrap()
                        .clone();
                    host_complete_item(&store, &target).await;
                }
            }
            _ => {}
        }

        let plan = store.get(&plan_id).await.unwrap().unwrap();
        let items = store.list_items(&plan_id).await.unwrap();

        // Premature final at the start must continue, never close.
        if epoch == 0 {
            let decision = check_ordinary_completion(&pool, session_id)
                .await
                .unwrap()
                .expect("active plan");
            assert!(
                decision.should_continue(),
                "epoch 0: premature final must continue"
            );
        }

        // ── Context transition: snapshot → compact → prepare → install ──
        ledger
            .touched_files
            .push(format!("src/checkout_{epoch}.rs"));
        ledger
            .commands_run
            .push_back(format!("cargo test --test checkout_{epoch}"));
        ledger
            .test_results
            .push(format!("test result: epoch {epoch} ok"));
        let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
            session_id,
            origin_prompt: Some(M005_OBJECTIVE),
            current_user_message: None,
            messages: &messages,
            active_goal: None,
            todos: &[],
            ledger: &ledger,
            security_findings: &[],
            previous_checkpoint: previous.as_ref(),
            plan_path: None,
            plan_content: None,
            active_work_plan: Some((&plan, items.as_slice())),
        });
        assert!(
            snapshot.objective.contains("migrate checkout flow"),
            "epoch {epoch}: objective preserved"
        );

        let checkpoint_id = format!("m005-traj-{epoch:02}");
        let result = compact_context(ContextCompactionRequest {
            messages: &messages,
            context_limit: 800,
            threshold: 0.4,
            reserved_output_tokens: 80,
            max_tool_result_tokens: 300,
            auto: true,
            prune: true,
            compaction_config: None,
            active_model: Some("test-model"),
            provider: None,
            provider_context: ProviderRequestContext::default(),
            cancellation: None,
            baseline: Some(&snapshot),
            proposed_checkpoint_id: Some(checkpoint_id.as_str()),
        })
        .await;
        assert!(
            matches!(
                result.status,
                CompactionStatus::Compacted
                    | CompactionStatus::CompactionRequired
                    | CompactionStatus::ProviderFailure
            ),
            "epoch {epoch}: expected compaction, got {:?}",
            result.status
        );
        let candidate = result.continuation_candidate.expect("candidate");
        let selected = select_materializable_evidence(&candidate.evidence, &checkpoint_id);
        let body =
            rollover::build_checkpoint_payload_body(&candidate.snapshot, &selected, &checkpoint_id);
        let payload = ContinuationCheckpointPayload::new(body).expect("payload");
        let prepared = checkpoints
            .prepare_with_id(
                session_id,
                &checkpoint_id,
                previous.as_ref().map(|c| c.id.as_str()),
                payload,
            )
            .await
            .expect("prepare");
        assert_eq!(prepared.status, ContinuationCheckpointStatus::Prepared);
        let read_back = checkpoints
            .get(session_id, &checkpoint_id)
            .await
            .expect("get")
            .expect("row");
        read_back.verify_digest().expect("digest");
        assert!(validate_message_invariants(&result.messages).is_ok());
        assert_eq!(
            count_continuation_frames(&result.messages),
            1,
            "epoch {epoch}: exactly one frame"
        );
        messages = result.messages.clone();
        let event = read_back
            .build_compacted_event(
                5,
                messages.len(),
                Some(result.tokens_before),
                Some(result.tokens_after),
                vec![format!("checkpoint:{checkpoint_id}")],
                vec![format!("epoch:{epoch}")],
                vec![],
            )
            .expect("event");
        let installed = checkpoints
            .install_with_compaction_event(session_id, &checkpoint_id, event)
            .await
            .expect("install");
        assert_eq!(installed.status, ContinuationCheckpointStatus::Installed);

        // ── Per-transition state/evidence table ──
        let provenance = provenance_from_body(&installed.payload.body)
            .expect("provenance parses")
            .expect("provenance present");
        assert_eq!(provenance.plan_id, plan.id.as_str());
        assert_eq!(provenance.revision, plan.revision);
        assert!(
            revalidate_against_current(Some(&provenance), Some((&plan, &items))).is_ok(),
            "epoch {epoch}: installed provenance must be current"
        );
        let remaining = items.iter().filter(|i| !i.status.is_terminal()).count();
        let expected_remaining = [9usize, 8, 7, 6, 5, 4, 3, 0][epoch];
        assert_eq!(
            remaining, expected_remaining,
            "epoch {epoch}: remaining required work"
        );
        let expected_phase = match epoch {
            0 | 1 => "phase one",
            2 | 3 => "phase one (steered)",
            _ => "phase two",
        };
        assert_eq!(plan.current_phase.as_deref(), Some(expected_phase));
        if remaining > 0 {
            let next = actionable_items(&items);
            assert!(!next.is_empty(), "epoch {epoch}: next action must exist");
            assert!(
                next[0]
                    .next_action
                    .as_deref()
                    .is_some_and(|a| !a.trim().is_empty()),
                "epoch {epoch}: next action must be set"
            );
        }
        let body_text = installed.payload.body.to_string();
        let steering_visible = body_text.contains("must keep parser strict")
            || messages.iter().any(|m| match m {
                Message::User { content } => content.iter().any(|p| match p {
                    ContentPart::Text { text } => text.contains("must keep parser strict"),
                    _ => false,
                }),
                _ => false,
            });
        assert_eq!(
            steering_visible,
            epoch >= 2,
            "epoch {epoch}: steering visibility"
        );
        // Evidence identity is stable: the same canonical job ids resolve
        // after every reset, with no relaunch.
        let snapshot_evidence = work_plan_evidence::assemble(&pool, &items).await.unwrap();
        if epoch >= 2 {
            assert_eq!(
                snapshot_evidence.lookup(WorkEvidenceKind::TestJob, &live_test_job),
                HostEvidenceStatus::Passed
            );
        }
        if epoch >= 3 {
            assert_eq!(
                snapshot_evidence.lookup(WorkEvidenceKind::DelegatedRun, &live_child_job),
                HostEvidenceStatus::Passed
            );
        }
        rows.push(TransitionRow {
            epoch,
            phase: plan.current_phase.clone().unwrap_or_default(),
            remaining,
            steering_visible,
        });

        // ── Verified phase boundary: fresh epoch where policy permits ──
        if epoch == 4 {
            let policy = ContextEpochPolicy::enabled_for_handoff(Some(8));
            let decision = decide_epoch(
                &policy,
                &ContextEpochInputs::new(4, true, false, false, true),
            );
            assert!(decision.should_reset);
            assert_eq!(decision.trigger, Some(ContextEpochTrigger::PhaseBoundary));
            let profile = infer_builtin_profile("openai/gpt-5");
            assert!(epoch_supported_for_profile(&profile));
            let todos: Vec<String> = vec![];
            let handles = vec![format!("ctx://tool/{session_id}/0/c0")];
            let steering = vec![M005_STEERING.to_string()];
            let frame = rollover::render_installed_projection(&installed, None);
            let fresh_inputs = FreshEpochInputs {
                system_instructions: "system: canonical instructions",
                objective: M005_OBJECTIVE,
                goal: None,
                work_plan: Some(&provenance),
                todos: &todos,
                continuation_frame_text: frame.as_str(),
                recovery_handles: &handles,
                steering: &steering,
                checkpoint_id: installed.id.as_str(),
                checkpoint_sequence: installed.sequence,
            };
            let fresh = build_fresh_epoch_messages(&fresh_inputs, &profile).expect("fresh");
            assert!(validate_fresh_epoch_messages(&fresh).is_ok());
            assert_eq!(count_continuation_frames(&fresh), 1);
            assert!(validate_message_invariants(&fresh).is_ok());
            let visible = format!("{fresh:?}");
            assert!(
                visible.contains("snapshot tests"),
                "fresh epoch keeps next work"
            );
            assert!(visible.contains("must keep parser strict"));
        }

        previous = Some(installed);
        messages.push(user(&format!(
            "follow-up work {epoch} {}",
            "z".repeat(1200)
        )));
        messages.push(user(&format!("more context {epoch} {}", "y".repeat(1200))));
    }

    // ── Trajectory closure ──
    assert_eq!(rows.len(), 8);
    assert_eq!(
        rows.iter().map(|r| r.remaining).collect::<Vec<_>>(),
        vec![9, 8, 7, 6, 5, 4, 3, 0]
    );
    assert!(rows.iter().skip(2).all(|r| r.steering_visible));
    assert_eq!(rows[7].phase, "phase two");
    let _ = rows[0].epoch;

    // No side effect was relaunched solely because context was reset: exactly
    // the two canonical jobs exist and completed work kept one attempt each.
    assert_eq!(count_session_jobs(&pool, session_id).await, 2);
    for item in store.list_items(&plan_id).await.unwrap() {
        assert_eq!(item.status, WorkItemStatus::Completed);
        assert_eq!(
            item.attempts, 1,
            "completed work is never re-executed across resets"
        );
    }
    let decision = check_ordinary_completion(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert_eq!(decision, ArbiterDecision::AllowCompletion);
    let (final_plan, _, assessment) = assess_active_plan(&pool, session_id)
        .await
        .unwrap()
        .expect("active plan");
    assert!(matches!(
        assessment,
        WorkPlanCompletionAssessment::Complete { .. }
    ));
    assert!(
        maybe_complete_plan_on_turn_end(&pool, &final_plan, &assessment, false)
            .await
            .unwrap()
    );
}

// ── Bounds: projections and diagnostics stay bounded ────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn projections_and_diagnostics_stay_bounded() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-bounds";
    seed_session(&pool, session_id, "proj-m005").await;
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input(
            session_id,
            "proj-m005",
            "large bounded plan",
            "bulk",
        ))
        .await
        .unwrap();
    for n in 0..12 {
        store
            .add_item(
                &plan.id,
                item_input(&format!("bulk work {n:02}"), "proceed"),
            )
            .await
            .unwrap();
    }
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    assert_eq!(items.len(), 12);

    // Model projection is bounded independently of total plan size.
    let default_view = project_work_plan(&plan, &items, &WorkPlanProjectionParams::default());
    assert!(default_view.items.len() <= 5);
    let wide = project_work_plan(
        &plan,
        &items,
        &WorkPlanProjectionParams {
            offset: 0,
            limit: 100,
            include_completed: false,
        },
    );
    assert!(wide.items.len() <= MAX_PROJECTION_ITEMS);
    let bytes = serde_json::to_string(&wide).unwrap().len();
    assert!(
        bytes <= MAX_PROJECTION_BYTES,
        "projection must stay bounded"
    );

    // Checkpoint provenance and handoff blocks stay bounded too.
    let provenance = build_checkpoint_provenance(&plan, &items);
    assert!(provenance.actionable.len() <= 5);
    assert!(serde_json::to_string(&provenance).unwrap().len() <= 4096);
    let todos: Vec<String> = vec!["bulk work 00".to_string()];
    let handles: Vec<String> = vec![];
    let steering: Vec<String> = vec![];
    let handoff = build_handoff_text(&FreshEpochInputs {
        system_instructions: "system",
        objective: "large bounded plan",
        goal: None,
        work_plan: Some(&provenance),
        todos: &todos,
        continuation_frame_text: "frame",
        recovery_handles: &handles,
        steering: &steering,
        checkpoint_id: "ckpt-bounds",
        checkpoint_sequence: 1,
    });
    assert!(handoff.len() <= MAX_EPOCH_HANDOFF_CHARS);

    // Assessment needs no extra model call: it is pure over host state.
    let assessment = assess_work_plan(&plan, &items, &WorkPlanEvidenceSnapshot::empty());
    assert!(matches!(
        assessment,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    let decision = decide_from_assessment(&assessment);
    assert!(decision.should_continue());
}

// ── Recovery: missing artifact degrades boundedly ───────────────────────────

#[tokio::test(flavor = "current_thread")]
async fn missing_recovery_artifact_degrades_boundedly() {
    let pool = common::pool::isolated_pool().await;
    let session_id = "sess-m005-degraded";
    seed_session(&pool, session_id, "proj-m005").await;
    let artifacts = InMemoryArtifactStore::new();
    let ledger = ContextLedgerState::new();
    let mut messages = vec![
        user("degraded objective with enough text to force compaction "),
        user(&"x".repeat(4000)),
        assistant(&"y".repeat(4000)),
    ];
    messages.extend(tool_pair(
        "t-pass",
        "bash",
        "test result: ok. 5 passed; 0 failed",
    ));
    let snapshot = assemble_continuation_snapshot(ContinuationAssemblyInput {
        session_id,
        origin_prompt: Some("degraded objective"),
        current_user_message: None,
        messages: &messages,
        active_goal: None,
        todos: &[],
        ledger: &ledger,
        security_findings: &[],
        previous_checkpoint: None,
        plan_path: None,
        plan_content: None,
        active_work_plan: None,
    });
    let result = compact_context(ContextCompactionRequest {
        messages: &messages,
        context_limit: 1000,
        threshold: 0.5,
        reserved_output_tokens: 100,
        max_tool_result_tokens: 200,
        auto: true,
        prune: false,
        compaction_config: None,
        active_model: Some("test-model"),
        provider: None,
        provider_context: ProviderRequestContext::default(),
        cancellation: None,
        baseline: Some(&snapshot),
        proposed_checkpoint_id: Some("m005-degraded-00"),
    })
    .await;
    let candidate = result.continuation_candidate.expect("candidate");
    let mut selected = select_materializable_evidence(&candidate.evidence, "m005-degraded-00");
    assert!(!selected.is_empty());
    let _persisted = persist_selected_evidence(
        &artifacts,
        session_id,
        "m005-degraded-00",
        0,
        &result.messages,
        &mut selected,
        &[],
    )
    .await;
    // Simulate loss of one backing artifact: verification degrades that ref
    // to summary-only instead of invalidating the checkpoint.
    if let Some(first) = selected.iter_mut().find(|r| r.recovery_handle.is_some()) {
        let handle = first.recovery_handle.clone().unwrap();
        first.recovery_handle = Some(format!("{handle}-missing"));
    }
    let outcome = verify_evidence_artifacts(&artifacts, session_id, &mut selected).await;
    assert_eq!(outcome.verified + outcome.degraded, selected.len());
    let body =
        rollover::build_checkpoint_payload_body(&candidate.snapshot, &selected, "m005-degraded-00");
    let store = ContinuationCheckpointStore::new(pool.clone());
    let prepared = store
        .prepare(
            session_id,
            None,
            ContinuationCheckpointPayload::new(body).expect("payload"),
        )
        .await
        .unwrap();
    let event = prepared
        .build_compacted_event(2, result.messages.len(), None, None, vec![], vec![], vec![])
        .unwrap();
    let installed = store
        .install_with_compaction_event(session_id, &prepared.id, event)
        .await
        .unwrap();
    assert_eq!(installed.status, ContinuationCheckpointStatus::Installed);
}

// ── Static guard: no second compaction/workflow owner ───────────────────────

#[test]
fn no_second_compaction_or_workflow_owner() {
    let epoch_source = include_str!("../src/context/epoch.rs");
    for forbidden in [
        "fn compact_context",
        "fn needs_context_compaction",
        "fn context_tokens",
        "CREATE TABLE",
        "continuation_checkpoint(",
        "DELETE FROM session",
        "UPDATE session SET",
    ] {
        assert!(
            !epoch_source.contains(forbidden),
            "epoch module must not introduce '{forbidden}'"
        );
    }
    for required in [
        "validate_message_invariants",
        "count_continuation_frames",
        "decide_epoch",
        "epoch_supported_for_profile",
    ] {
        assert!(
            epoch_source.contains(required),
            "epoch module must reuse canonical owner '{required}'"
        );
    }
    for forbidden in ["cron", "workflow engine", "BPM", "fn compact_context"] {
        assert!(
            !epoch_source.contains(forbidden),
            "long-horizon work must not grow a workflow scheduler in '{forbidden}'"
        );
    }
    let assessment_source = include_str!("../crates/codegg-core/src/work_plan/assessment.rs");
    for forbidden in [
        "fn compact_context",
        "CREATE TABLE",
        "cron",
        "workflow engine",
    ] {
        assert!(
            !assessment_source.contains(forbidden),
            "work plan assessment must not own compaction or workflow ('{forbidden}')"
        );
    }
    let store_source = include_str!("../crates/codegg-core/src/work_plan/store.rs");
    for forbidden in ["fn compact_context", "cron", "workflow engine"] {
        assert!(
            !store_source.contains(forbidden),
            "work plan store must not own compaction or workflow ('{forbidden}')"
        );
    }
}
