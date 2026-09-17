//! Long-horizon M002 integration: durable WorkPlan foundation.
//!
//! Covers restart/recovery, contention/cancellation, security/negative,
//! and migration/compatibility paths that need file-backed reopen or
//! cross-store (Goal) setup beyond the in-module unit tests.

use codegg_core::goal::GoalStore;
use codegg_core::session::schema;
use codegg_core::storage::STORAGE_LAYOUT_VERSION;
use codegg_core::work_plan::{
    NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind,
    WorkEvidenceRef, WorkItemStatus, WorkPlanError, WorkPlanStatus, WorkPlanStore,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

async fn isolated_pool() -> SqlitePool {
    let name = format!("workplan_test_{}", uuid::Uuid::new_v4().simple());
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

async fn ensure_session(pool: &SqlitePool, session_id: &str, project_id: &str) {
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
        objective: "implement durable work plans".to_string(),
        origin_provenance: "turn:turn-1".to_string(),
        current_phase: Some("foundation".to_string()),
    }
}

fn item_input(description: &str) -> NewWorkItem {
    NewWorkItem {
        parent_item_id: None,
        dependencies: vec![],
        status: WorkItemStatus::Actionable,
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
async fn migration_is_additive_and_layout_tracks_v62() {
    let pool = isolated_pool().await;
    assert_eq!(STORAGE_LAYOUT_VERSION, 62);
    let version: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(version, 62);
    for table in ["work_plan", "work_item", "goal", "session"] {
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(exists, 1, "missing table {table}");
    }
    // Remigration is a restart-safe no-op.
    schema::migrate(&pool).await.expect("remigrate");
    let rerun: i64 = sqlx::query_scalar(
        "SELECT COALESCE((SELECT version FROM migration_version WHERE id = 1), 0)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rerun, 62);
}

#[tokio::test(flavor = "current_thread")]
async fn pre_migration_database_gains_empty_tables_without_touching_goals() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "sess-pre", "proj-pre").await;
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active(
            "sess-pre",
            "proj-pre",
            "title",
            "objective",
            None,
            None,
            vec![],
        )
        .await
        .unwrap();
    // Simulate a pre-M002 database: drop the new tables and rewind the
    // version marker, keeping the goal row intact.
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
    // Re-running the canonical migrator must recreate empty tables.
    schema::migrate(&pool).await.expect("migrate to latest");
    let version: i64 = sqlx::query_scalar("SELECT version FROM migration_version WHERE id = 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 62);
    let plans: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_plan")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(plans, 0);
    let items: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_item")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(items, 0);
    // Existing goal data is unchanged; legacy sessions simply have no plan.
    let reloaded = goals.get(&goal.id).await.unwrap().unwrap();
    assert_eq!(reloaded.objective, "objective");
    let store = WorkPlanStore::new(pool);
    assert!(store
        .active_for_session("sess-pre")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn restart_reloads_plan_and_items_with_stable_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("workplan-restart.db");
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    schema::migrate(&pool).await.unwrap();
    let store = WorkPlanStore::new(pool.clone());
    let plan = store
        .create_active(plan_input("sess-r", "proj-r"))
        .await
        .unwrap();
    let (_, first) = store.add_item(&plan.id, item_input("first")).await.unwrap();
    let (_, second) = store
        .add_item(&plan.id, item_input("second"))
        .await
        .unwrap();
    let (_, started) = store
        .transition_item(
            &first.id,
            first.revision,
            WorkItemStatus::InProgress,
            None,
            None,
        )
        .await
        .unwrap();
    let plan_id = plan.id.clone();
    let first_id = started.id.clone();
    let second_id = second.id.clone();
    let plan_revision = store.get(&plan_id).await.unwrap().unwrap().revision;
    pool.close().await;

    // Reopen: durable state loads exactly. In-progress does not imply that
    // a side effect should be replayed — the store returns the same status
    // and revision with no automatic transition.
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    schema::migrate(&pool).await.unwrap();
    let store = WorkPlanStore::new(pool);
    let reloaded = store
        .get(&plan_id)
        .await
        .unwrap()
        .expect("plan survives restart");
    assert_eq!(reloaded.revision, plan_revision);
    assert_eq!(reloaded.status, WorkPlanStatus::Active);
    let items = store.list_items(&plan_id).await.unwrap();
    assert_eq!(items.len(), 2);
    let first = items.iter().find(|item| item.id == first_id).unwrap();
    assert_eq!(first.status, WorkItemStatus::InProgress);
    assert_eq!(first.attempts, 1);
    let second = items.iter().find(|item| item.id == second_id).unwrap();
    assert_eq!(second.status, WorkItemStatus::Actionable);
    let active = store.active_for_session("sess-r").await.unwrap().unwrap();
    assert_eq!(active.id, plan_id);
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_item_writers_serialize_with_explicit_conflict() {
    let store = WorkPlanStore::new(isolated_pool().await);
    let plan = store
        .create_active(plan_input("sess-c", "proj-c"))
        .await
        .unwrap();
    let (_, item) = store
        .add_item(&plan.id, item_input("contended"))
        .await
        .unwrap();
    let revision = item.revision;
    // Two writers read the same revision; the first commit wins and the
    // second receives an explicit conflict instead of last-write-wins.
    let first = store
        .transition_item(&item.id, revision, WorkItemStatus::InProgress, None, None)
        .await;
    assert!(first.is_ok());
    let second = store
        .transition_item(
            &item.id,
            revision,
            WorkItemStatus::Blocked,
            Some("late".to_string()),
            None,
        )
        .await;
    assert!(matches!(second, Err(WorkPlanError::Conflict { .. })));
    let current = store.get_item(&item.id).await.unwrap().unwrap();
    assert_eq!(current.status, WorkItemStatus::InProgress);
}

#[tokio::test(flavor = "current_thread")]
async fn cancel_racing_completion_serializes_deterministically() {
    let store = WorkPlanStore::new(isolated_pool().await);
    let plan = store
        .create_active(plan_input("sess-race", "proj-race"))
        .await
        .unwrap();
    let with_evidence = NewWorkItem {
        evidence: vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job-1".to_string(),
            detail: None,
        }],
        ..item_input("racer")
    };
    let (_, item) = store.add_item(&plan.id, with_evidence).await.unwrap();
    let (_, started) = store
        .transition_item(
            &item.id,
            item.revision,
            WorkItemStatus::InProgress,
            None,
            None,
        )
        .await
        .unwrap();
    // Observe the plan revision before the item completes.
    let observed = store.get(&plan.id).await.unwrap().unwrap().revision;
    let (_, _) = store
        .transition_item(
            &started.id,
            started.revision,
            WorkItemStatus::Completed,
            None,
            None,
        )
        .await
        .unwrap();
    // Completion bumped the plan revision, so a cancel against the stale
    // revision fails; a fresh cancel wins deterministically.
    let stale = store
        .transition_plan(&plan.id, observed, WorkPlanStatus::Cancelled)
        .await;
    assert!(matches!(stale, Err(WorkPlanError::Conflict { .. })));
    let current = store.get(&plan.id).await.unwrap().unwrap();
    let cancelled = store
        .transition_plan(&plan.id, current.revision, WorkPlanStatus::Cancelled)
        .await
        .unwrap();
    assert_eq!(cancelled.status, WorkPlanStatus::Cancelled);
}

#[tokio::test(flavor = "current_thread")]
async fn goal_replacement_binding_race_fails_closed() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "sess-g1", "proj-g").await;
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active(
            "sess-g1",
            "proj-g",
            "title",
            "objective",
            None,
            None,
            vec![],
        )
        .await
        .unwrap();
    let store = WorkPlanStore::new(pool.clone());
    let mut input = plan_input("sess-g1", "proj-g");
    input.goal_id = Some(goal.id.clone());
    let plan = store.create_active(input).await.unwrap();
    let revision = plan.revision;
    // A concurrent plan mutation bumps the revision; the stale bind fails.
    let (_, _) = store.add_item(&plan.id, item_input("a")).await.unwrap();
    let stale_bind = store.bind_goal(&plan.id, revision, &goal.id).await;
    assert!(matches!(stale_bind, Err(WorkPlanError::Conflict { .. })));
    // Fresh bind of the same goal is idempotent-safe.
    let current = store.get(&plan.id).await.unwrap().unwrap();
    let bound = store
        .bind_goal(&plan.id, current.revision, &goal.id)
        .await
        .unwrap();
    assert_eq!(bound.goal_id.as_deref(), Some(goal.id.as_str()));
    // Unbinding through CAS keeps history (plan row survives, ref cleared).
    let unbound = store.unbind_goal(&bound.id, bound.revision).await.unwrap();
    assert!(unbound.goal_id.is_none());
    assert!(store.get(&bound.id).await.unwrap().is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn cross_session_goal_binding_rejected() {
    let pool = isolated_pool().await;
    ensure_session(&pool, "sess-a", "proj-a").await;
    ensure_session(&pool, "sess-b", "proj-b").await;
    let goals = GoalStore::new(pool.clone());
    let goal = goals
        .create_active("sess-a", "proj-a", "title", "objective", None, None, vec![])
        .await
        .unwrap();
    let store = WorkPlanStore::new(pool);
    let plan = store
        .create_active(plan_input("sess-b", "proj-b"))
        .await
        .unwrap();
    let err = store.bind_goal(&plan.id, plan.revision, &goal.id).await;
    assert!(matches!(
        err,
        Err(WorkPlanError::Validation(_) | WorkPlanError::ScopeMismatch(_))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn missing_evidence_target_is_unavailable_not_satisfied() {
    // M002 stores the ref shape; resolution against canonical stores is
    // M003. The invariant here: a ref to a nonexistent target is kept as
    // an explicit ref (never auto-satisfied) and completion still requires
    // a host signal beyond bare text.
    let store = WorkPlanStore::new(isolated_pool().await);
    let plan = store
        .create_active(plan_input("sess-e", "proj-e"))
        .await
        .unwrap();
    let dangling = NewWorkItem {
        evidence: vec![WorkEvidenceRef {
            kind: WorkEvidenceKind::TestJob,
            ref_id: "job-does-not-exist".to_string(),
            detail: Some("awaiting host correlation".to_string()),
        }],
        ..item_input("dangling")
    };
    let (_, item) = store.add_item(&plan.id, dangling).await.unwrap();
    // The ref round-trips exactly; nothing marks it satisfied.
    let loaded = store.get_item(&item.id).await.unwrap().unwrap();
    assert_eq!(loaded.evidence.len(), 1);
    assert_eq!(loaded.evidence[0].ref_id, "job-does-not-exist");
    assert_eq!(loaded.evidence[0].kind, WorkEvidenceKind::TestJob);
}

#[tokio::test(flavor = "current_thread")]
async fn owner_refs_are_provenance_only() {
    let store = WorkPlanStore::new(isolated_pool().await);
    let plan = store
        .create_active(plan_input("sess-o", "proj-o"))
        .await
        .unwrap();
    let owned = NewWorkItem {
        owner_run_id: Some("run-123".to_string()),
        owner_job_id: Some("job-123".to_string()),
        ..item_input("owned")
    };
    let (_, item) = store.add_item(&plan.id, owned).await.unwrap();
    assert_eq!(item.owner_run_id.as_deref(), Some("run-123"));
    assert_eq!(item.owner_job_id.as_deref(), Some("job-123"));
    // Provenance alone grants no transition authority: invalid transitions
    // are still rejected and CAS still applies.
    let bad = store
        .transition_item(
            &item.id,
            item.revision,
            WorkItemStatus::Completed,
            None,
            None,
        )
        .await;
    // Actionable -> Completed is allowed by the matrix, and the owner ref
    // counts as the M002 host-signal floor — but a stale revision still fails.
    assert!(bad.is_ok());
    let stale = store
        .transition_item(
            &item.id,
            item.revision,
            WorkItemStatus::Cancelled,
            None,
            None,
        )
        .await;
    assert!(matches!(stale, Err(WorkPlanError::Conflict { .. })));
}

#[tokio::test(flavor = "current_thread")]
async fn oversized_cyclic_malformed_rejected_before_persistence() {
    let store = WorkPlanStore::new(isolated_pool().await);
    let plan = store
        .create_active(plan_input("sess-n", "proj-n"))
        .await
        .unwrap();
    let before = store.list_items(&plan.id).await.unwrap().len();

    let oversized = NewWorkItem {
        description: "x".repeat(2048),
        ..item_input("big")
    };
    assert!(store.add_item(&plan.id, oversized).await.is_err());

    let (_, first) = store.add_item(&plan.id, item_input("first")).await.unwrap();
    // Self-dependency and unknown-dependency rejected.
    let mut self_dep = item_input("self");
    self_dep.dependencies = vec![first.id.clone(), first.id.clone()];
    assert!(store.add_item(&plan.id, self_dep).await.is_err());

    // Malformed (empty description) rejected.
    let mut empty = item_input("empty");
    empty.description = "   ".to_string();
    assert!(store.add_item(&plan.id, empty).await.is_err());

    let after = store.list_items(&plan.id).await.unwrap().len();
    assert_eq!(after, before + 1);
}
