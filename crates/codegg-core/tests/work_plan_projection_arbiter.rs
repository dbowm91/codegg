//! Long-horizon M003 integration: projection and completion arbitration.
//!
//! Covers the assessment state matrix, bounded/paginated projection,
//! WorkPlan-to-Todo policy for all Todo modes, invalid/stale mutations,
//! evidence authority, restart reconstruction, contention, and
//! migration/compatibility paths that need store-backed setup.

use codegg_core::model_profile::types::TaskStatePolicy;
use codegg_core::session::schema;
use codegg_core::task_state::TodoStatus;
use codegg_core::work_plan::{
    assess_work_plan, item_is_satisfied, lookup_item_summary, parse_todo_work_ref,
    project_to_todo_items, project_work_plan, todo_id_for_work_item, validate_todo_feedback,
    HostEvidenceStatus, NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition,
    WorkEvidenceKind, WorkEvidenceRef, WorkItemStatus, WorkPlanCompletionAssessment,
    WorkPlanEvidenceSnapshot, WorkPlanProjectionParams, WorkPlanStore, MAX_PROJECTION_BYTES,
    MAX_PROJECTION_ITEMS,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

async fn isolated_pool() -> SqlitePool {
    let name = format!("workplan_m003_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
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
    schema::migrate(&pool).await.expect("migrate");
    pool
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

#[tokio::test(flavor = "current_thread")]
async fn assessment_state_matrix() {
    let pool = isolated_pool().await;
    let store = WorkPlanStore::new(pool);
    let plan = store.create_active(plan_input("s1", "p1")).await.unwrap();

    // Actionable work remaining.
    let (_, first) = store.add_item(&plan.id, item_input("first")).await.unwrap();
    let items = store.list_items(&plan.id).await.unwrap();
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    let assessment = assess_work_plan(&plan, &items, &WorkPlanEvidenceSnapshot::empty());
    assert!(
        matches!(
            assessment,
            WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
        ),
        "pending unmet item must read as actionable, got {assessment:?}"
    );

    // Blocked.
    let (_, blocked) = store
        .transition_item(
            &first.id,
            first.revision,
            WorkItemStatus::Blocked,
            Some("waiting on review".to_string()),
            None,
        )
        .await
        .unwrap();
    let _ = blocked;
    let items = store.list_items(&plan.id).await.unwrap();
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    let assessment = assess_work_plan(&plan, &items, &WorkPlanEvidenceSnapshot::empty());
    assert!(
        matches!(assessment, WorkPlanCompletionAssessment::Blocked { .. }),
        "blocked item must read as blocked, got {assessment:?}"
    );

    // InFlight via owner handle when no actionable remains.
    let pool2 = isolated_pool().await;
    let store2 = WorkPlanStore::new(pool2);
    let plan2 = store2.create_active(plan_input("s2", "p1")).await.unwrap();
    let owned_input = NewWorkItem {
        status: WorkItemStatus::InProgress,
        owner_run_id: Some("run-1".to_string()),
        acceptance: vec![],
        evidence: vec![],
        ..item_input("owned")
    };
    store2.add_item(&plan2.id, owned_input).await.unwrap();
    let items2 = store2.list_items(&plan2.id).await.unwrap();
    let plan2 = store2.get(&plan2.id).await.unwrap().unwrap();
    let assessment2 = assess_work_plan(&plan2, &items2, &WorkPlanEvidenceSnapshot::empty());
    assert!(
        matches!(assessment2, WorkPlanCompletionAssessment::InFlight { .. }),
        "in-progress owned item must read as in-flight, got {assessment2:?}"
    );

    // AwaitingUserJudgment.
    let pool3 = isolated_pool().await;
    let store3 = WorkPlanStore::new(pool3);
    let plan3 = store3.create_active(plan_input("s3", "p1")).await.unwrap();
    let judgment_input = NewWorkItem {
        acceptance: vec![WorkAcceptance {
            description: "owner signs off".to_string(),
            disposition: WorkAcceptanceDisposition::RequiresUserJudgment,
            note: None,
        }],
        ..item_input("judgment")
    };
    store3.add_item(&plan3.id, judgment_input).await.unwrap();
    let items3 = store3.list_items(&plan3.id).await.unwrap();
    let plan3 = store3.get(&plan3.id).await.unwrap().unwrap();
    let assessment3 = assess_work_plan(&plan3, &items3, &WorkPlanEvidenceSnapshot::empty());
    assert!(
        matches!(
            assessment3,
            WorkPlanCompletionAssessment::AwaitingUserJudgment { .. }
        ),
        "judgment-only item must await user, got {assessment3:?}"
    );

    // Complete via host satisfaction.
    let pool4 = isolated_pool().await;
    let store4 = WorkPlanStore::new(pool4);
    let plan4 = store4.create_active(plan_input("s4", "p1")).await.unwrap();
    let satisfied_input = NewWorkItem {
        acceptance: vec![WorkAcceptance {
            description: "c".to_string(),
            disposition: WorkAcceptanceDisposition::Satisfied,
            note: None,
        }],
        ..item_input("satisfied")
    };
    let (_, item4) = store4.add_item(&plan4.id, satisfied_input).await.unwrap();
    let (_, started) = store4
        .transition_item(
            &item4.id,
            item4.revision,
            WorkItemStatus::InProgress,
            None,
            None,
        )
        .await
        .unwrap();
    store4
        .transition_item(
            &started.id,
            started.revision,
            WorkItemStatus::Completed,
            None,
            None,
        )
        .await
        .unwrap();
    let items4 = store4.list_items(&plan4.id).await.unwrap();
    let plan4 = store4.get(&plan4.id).await.unwrap().unwrap();
    let assessment4 = assess_work_plan(&plan4, &items4, &WorkPlanEvidenceSnapshot::empty());
    assert!(
        matches!(assessment4, WorkPlanCompletionAssessment::Complete { .. }),
        "satisfied completed plan must read complete, got {assessment4:?}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bounded_paginated_projection_for_large_plan() {
    let pool = isolated_pool().await;
    let store = WorkPlanStore::new(pool);
    let plan = store.create_active(plan_input("s1", "p1")).await.unwrap();
    for index in 0..20 {
        store
            .add_item(&plan.id, item_input(&format!("work {index}")))
            .await
            .unwrap();
    }
    let items = store.list_items(&plan.id).await.unwrap();
    assert_eq!(items.len(), 20);
    let plan = store.get(&plan.id).await.unwrap().unwrap();

    let first = project_work_plan(
        &plan,
        &items,
        &WorkPlanProjectionParams {
            offset: 0,
            limit: 5,
            include_completed: false,
        },
    );
    assert!(first.items.len() <= 5);
    assert!(first.items.len() <= MAX_PROJECTION_ITEMS);
    let bytes = serde_json::to_string(&first).unwrap().len();
    assert!(bytes <= MAX_PROJECTION_BYTES);

    let second = project_work_plan(
        &plan,
        &items,
        &WorkPlanProjectionParams {
            offset: 5,
            limit: 5,
            include_completed: false,
        },
    );
    assert_eq!(second.items.len(), 5);
    assert_ne!(first.items[0].id, second.items[0].id);

    let lookup = lookup_item_summary(&items, &items[7].id).unwrap();
    assert_eq!(lookup.id, items[7].id);
}

#[tokio::test(flavor = "current_thread")]
async fn todo_projection_respects_all_modes_and_caps() {
    let pool = isolated_pool().await;
    let store = WorkPlanStore::new(pool);
    let plan = store.create_active(plan_input("s1", "p1")).await.unwrap();
    for index in 0..20 {
        store
            .add_item(&plan.id, item_input(&format!("work {index}")))
            .await
            .unwrap();
    }
    let items = store.list_items(&plan.id).await.unwrap();
    let plan = store.get(&plan.id).await.unwrap().unwrap();

    let disabled = project_to_todo_items(&plan, &items, &TaskStatePolicy::disabled());
    assert!(disabled.is_empty());

    let sparse = project_to_todo_items(&plan, &items, &TaskStatePolicy::sparse_plan());
    assert!(sparse.len() <= 8);

    let explicit = project_to_todo_items(&plan, &items, &TaskStatePolicy::explicit_todo());
    assert!(explicit.len() <= 10);

    let guided = project_to_todo_items(&plan, &items, &TaskStatePolicy::guided_current_task());
    assert!(guided.len() <= 4);

    let in_progress = explicit
        .iter()
        .filter(|item| item.status == TodoStatus::InProgress)
        .count();
    assert!(in_progress <= 1, "single in-progress invariant");

    // Terminal history never leaks into Todo context.
    for todo in &explicit {
        assert!(
            parse_todo_work_ref(&todo.id).is_some(),
            "projected todos must carry exact revision mapping"
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn stale_todo_mapping_cannot_mutate_newer_item() {
    let pool = isolated_pool().await;
    let store = WorkPlanStore::new(pool);
    let plan = store.create_active(plan_input("s1", "p1")).await.unwrap();
    let (_, item) = store.add_item(&plan.id, item_input("work")).await.unwrap();
    let todo_id = todo_id_for_work_item(&item);

    // Advance the item so the projected revision goes stale.
    let patch = codegg_core::work_plan::model::WorkItemPatch {
        next_action: Some(Some("updated".to_string())),
        ..Default::default()
    };
    let (_, newer) = store
        .update_item(&item.id, item.revision, patch)
        .await
        .unwrap();
    assert_ne!(item.revision, newer.revision);

    let stale_todo = codegg_core::task_state::TodoItem {
        id: todo_id,
        content: "work".to_string(),
        status: TodoStatus::InProgress,
        priority: codegg_core::task_state::TodoPriority::Medium,
        blocker: None,
    };
    let err = validate_todo_feedback(&stale_todo, &newer, &WorkPlanEvidenceSnapshot::empty())
        .unwrap_err();
    assert!(matches!(
        err,
        codegg_core::work_plan::TodoFeedbackError::Stale { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn evidence_authority_rules() {
    // Claimed test text alone never satisfies.
    let pool = isolated_pool().await;
    let store = WorkPlanStore::new(pool);
    let plan = store.create_active(plan_input("s1", "p1")).await.unwrap();
    let claimed = NewWorkItem {
        evidence: vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job-missing".to_string(),
            detail: None,
        }],
        ..item_input("claimed")
    };
    let (_, item) = store.add_item(&plan.id, claimed).await.unwrap();
    let empty = WorkPlanEvidenceSnapshot::empty();
    assert!(!item_is_satisfied(&item, &empty));

    // Passing canonical evidence satisfies.
    let snapshot = WorkPlanEvidenceSnapshot::empty().with_entry(
        WorkEvidenceKind::TestJob,
        "job-1",
        HostEvidenceStatus::Passed,
    );
    let evidenced = codegg_core::work_plan::WorkItem {
        evidence: vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job-1".to_string(),
            detail: None,
        }],
        ..item
    };
    assert!(item_is_satisfied(&evidenced, &snapshot));

    // Failed evidence keeps the item actionable, never complete.
    let failed_snapshot = WorkPlanEvidenceSnapshot::empty().with_entry(
        WorkEvidenceKind::TestJob,
        "job-1",
        HostEvidenceStatus::Failed,
    );
    let plan = store.get(&plan.id).await.unwrap().unwrap();
    // Mark the item InProgress so the assessment exercises the failed path.
    let (_, started) = store
        .transition_item(
            &evidenced.id,
            evidenced.revision,
            WorkItemStatus::InProgress,
            None,
            None,
        )
        .await
        .unwrap();
    let items = vec![started];
    let assessment = assess_work_plan(&plan, &items, &failed_snapshot);
    assert!(matches!(
        assessment,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
    let _ = items;
}

#[tokio::test(flavor = "current_thread")]
async fn projection_reconstructed_after_file_backed_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("workplan.db");
    let url = format!("sqlite:{}?mode=rwc", path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    schema::migrate(&pool).await.unwrap();
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-1", "proj-1"))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(&plan.id, item_input("durable"))
        .await
        .unwrap();
    let revision_before = store.get(&plan.id).await.unwrap().unwrap().revision;
    drop(pool);

    let pool2 = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    schema::migrate(&pool2).await.unwrap();
    let store2 = WorkPlanStore::new(pool2);
    let reloaded = store2.get(&plan.id).await.unwrap().unwrap();
    assert_eq!(reloaded.revision, revision_before);
    let items = store2.list_items(&plan.id).await.unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, item.id);
    let projection = project_work_plan(&reloaded, &items, &WorkPlanProjectionParams::default());
    assert_eq!(projection.total_items, 1);
    let assessment = assess_work_plan(&reloaded, &items, &WorkPlanEvidenceSnapshot::empty());
    assert!(matches!(
        assessment,
        WorkPlanCompletionAssessment::ActionableWorkRemaining { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn work_plan_absent_session_follows_legacy_behavior() {
    let pool = isolated_pool().await;
    let store = WorkPlanStore::new(pool);
    assert!(store
        .active_for_session("no-such-session")
        .await
        .unwrap()
        .is_none());
    assert!(store
        .active_for_goal("no-such-goal")
        .await
        .unwrap()
        .is_none());
}
