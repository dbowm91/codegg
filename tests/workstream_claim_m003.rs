use std::str::FromStr;

use codegg_core::identity::{AgentRunGroupId, AgentRunId, AgentTaskId, ProjectId, WorkspaceId};
use codegg_core::workstream_claim::{
    ClaimActor, ClaimCas, ClaimPrerequisite, ClaimTarget, ClaimTransition, NewWorkstreamClaim,
    WorkstreamClaimError, WorkstreamClaimStore,
};
use codegg_protocol::projection::{
    AgentRunGroupSummaryProjection, AgentRunSummary, SessionProjectionSnapshot,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

async fn fixture() -> (
    tempfile::TempDir,
    SqlitePool,
    AgentRunGroupId,
    Vec<(AgentRunId, AgentTaskId)>,
) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("claims.db");
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&pool).await.unwrap();

    let project = ProjectId::new();
    let workspace = WorkspaceId::new();
    let group = AgentRunGroupId::new();
    let mut members = Vec::new();
    for ordinal in 0..8 {
        let task = AgentTaskId::new();
        let run = AgentRunId::new();
        sqlx::query(
            "INSERT INTO agent_task (task_id, root_task_id, session_id, project_id, workspace_id, \
             requested_agent, delegation_key, request_fingerprint, description, status, created_at, updated_at) \
             VALUES (?, ?, 'session-test', ?, ?, 'worker', ?, 'fingerprint', 'redacted', 'running', 1, 1)",
        )
        .bind(task.as_str())
        .bind(task.as_str())
        .bind(project.as_str())
        .bind(workspace.as_str())
        .bind(format!("delegate-{ordinal}"))
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO agent_run (run_id, task_id, root_run_id, workspace_id, agent_name, provider, model, \
             authority_digest, budget_json, status, cancellation_requested, created_at, updated_at) \
             VALUES (?, ?, ?, ?, 'worker', 'test', 'test', 'test', '{}', 'running', 0, 1, 1)",
        )
        .bind(run.as_str())
        .bind(task.as_str())
        .bind(run.as_str())
        .bind(workspace.as_str())
        .execute(&pool)
        .await
        .unwrap();
        members.push((run, task));
    }
    let root_run = &members[0].0;
    sqlx::query(
        "INSERT INTO agent_run_group (group_id, root_run_id, owner_run_id, join_policy, cancel_remaining, \
         status, created_at, idempotency_key, owner_kind) VALUES (?, ?, ?, 'all', 0, 'running', 1, 'group-key', 'run')",
    )
    .bind(group.as_str())
    .bind(root_run.as_str())
    .bind(root_run.as_str())
    .execute(&pool)
    .await
    .unwrap();
    for (ordinal, (run, _)) in members.iter().enumerate() {
        sqlx::query(
            "INSERT INTO agent_run_group_member (group_id, ordinal, run_id) VALUES (?, ?, ?)",
        )
        .bind(group.as_str())
        .bind(ordinal as i64)
        .bind(run.as_str())
        .execute(&pool)
        .await
        .unwrap();
    }
    (dir, pool, group, members)
}

fn input(group: &AgentRunGroupId, run: &AgentRunId, key: &str, path: &str) -> NewWorkstreamClaim {
    NewWorkstreamClaim {
        project_id: ProjectId::new(),
        workspace_id: WorkspaceId::new(),
        group_id: group.clone(),
        actor: ClaimActor::Run(run.clone()),
        target: ClaimTarget::Path(path.into()),
        idempotency_key: key.into(),
        lease_expires_at: Some(chrono::Utc::now().timestamp_millis() + 60_000),
    }
}

async fn scoped_input(
    pool: &SqlitePool,
    group: &AgentRunGroupId,
    run: &AgentRunId,
    key: &str,
    path: &str,
) -> NewWorkstreamClaim {
    let row = sqlx::query("SELECT t.project_id, t.workspace_id FROM agent_run_group_member gm \
                           JOIN agent_run r ON r.run_id=gm.run_id JOIN agent_task t ON t.task_id=r.task_id \
                           WHERE gm.group_id=? LIMIT 1")
        .bind(group.as_str())
        .fetch_one(pool)
        .await
        .unwrap();
    use sqlx::Row;
    NewWorkstreamClaim {
        project_id: ProjectId::parse(&row.get::<String, _>("project_id")).unwrap(),
        workspace_id: WorkspaceId::parse(&row.get::<String, _>("workspace_id")).unwrap(),
        ..input(group, run, key, path)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn simultaneous_overlapping_claims_have_one_winner_and_stale_cas_fails() {
    let (_dir, pool, group, members) = fixture().await;
    let store = WorkstreamClaimStore::new(pool.clone());
    let mut contenders = Vec::new();
    for ordinal in 0..8 {
        let store = store.clone();
        let input = scoped_input(
            &pool,
            &group,
            &members[ordinal].0,
            &format!("claim-{ordinal}"),
            "src/work.rs",
        )
        .await;
        contenders.push(tokio::spawn(async move { store.claim(input).await }));
    }
    let mut won = None;
    for contender in contenders {
        match contender.await.unwrap() {
            Ok(claim) => {
                assert!(won.replace(claim).is_none());
            }
            Err(WorkstreamClaimError::Overlap) => {}
            Err(error) => panic!("unexpected contention result: {error}"),
        }
    }
    let claim = won.expect("one contender must win");
    let actor = claim.owner.clone();
    let renewed = store
        .transition(ClaimCas {
            claim_id: claim.claim_id.clone(),
            actor: actor.clone(),
            expected_version: 1,
            expected_lease_generation: 1,
            transition: ClaimTransition::Renew,
            new_owner: None,
            lease_expires_at: Some(chrono::Utc::now().timestamp_millis() + 120_000),
        })
        .await
        .unwrap();
    assert_eq!(renewed.version, 2);
    assert!(matches!(
        store
            .transition(ClaimCas {
                claim_id: claim.claim_id,
                actor,
                expected_version: 1,
                expected_lease_generation: 1,
                transition: ClaimTransition::Release,
                new_owner: None,
                lease_expires_at: None,
            })
            .await,
        Err(WorkstreamClaimError::StaleRevision)
    ));

    let handoff_source = members[0].0.clone();
    let handoff_target = members[1].0.clone();
    let handoff = store
        .claim(scoped_input(&pool, &group, &handoff_source, "handoff", "src/handoff.rs").await)
        .await
        .unwrap();
    let offered = store
        .transition(ClaimCas {
            claim_id: handoff.claim_id.clone(),
            actor: ClaimActor::Run(handoff_source),
            expected_version: 1,
            expected_lease_generation: 1,
            transition: ClaimTransition::Handoff,
            new_owner: Some(ClaimActor::Run(handoff_target.clone())),
            lease_expires_at: Some(chrono::Utc::now().timestamp_millis() + 120_000),
        })
        .await
        .unwrap();
    assert_eq!(
        offered.state,
        codegg_core::workstream_claim::ClaimState::Offered
    );
    let accepted = store
        .transition(ClaimCas {
            claim_id: offered.claim_id.clone(),
            actor: ClaimActor::Run(handoff_target),
            expected_version: offered.version,
            expected_lease_generation: offered.lease_generation,
            transition: ClaimTransition::AcceptOffer,
            new_owner: None,
            lease_expires_at: None,
        })
        .await
        .unwrap();
    assert_eq!(
        accepted.state,
        codegg_core::workstream_claim::ClaimState::Claimed
    );
    assert_eq!(accepted.version, 3);

    let mut snapshot = SessionProjectionSnapshot::empty(
        "session-test",
        accepted.project_id.as_str(),
        accepted.workspace_id.as_str(),
    );
    snapshot.event_seq = 99;
    snapshot
        .run_groups
        .push_back(AgentRunGroupSummaryProjection {
            group_id: group.to_string(),
            member_run_ids: members.iter().map(|(run, _)| run.to_string()).collect(),
            ..AgentRunGroupSummaryProjection::default()
        });
    for (run, task) in &members {
        snapshot.agent_runs.push_back(AgentRunSummary {
            run_id: run.to_string(),
            task_id: task.to_string(),
            project_id: accepted.project_id.to_string(),
            workspace_id: accepted.workspace_id.to_string(),
            status: "running".into(),
            ..AgentRunSummary::default()
        });
    }
    let board = store
        .project_group(&snapshot, &group, &ClaimActor::Run(members[1].0.clone()))
        .await
        .unwrap();
    assert_eq!(board.revision, 99);
    assert_eq!(board.claims.len(), 2);
    assert_eq!(
        board
            .claims
            .iter()
            .find(|claim| claim.claim_id == accepted.claim_id.to_string())
            .unwrap()
            .state,
        "claimed"
    );

    let mut task_contenders = Vec::new();
    for (ordinal, (run, _)) in members.iter().enumerate() {
        let store = store.clone();
        let mut input = scoped_input(
            &pool,
            &group,
            run,
            &format!("task-claim-{ordinal}"),
            "ignored.rs",
        )
        .await;
        input.target = ClaimTarget::Task(members[0].1.clone());
        task_contenders.push(tokio::spawn(async move { store.claim(input).await }));
    }
    let mut task_winners = 0;
    for contender in task_contenders {
        match contender.await.unwrap() {
            Ok(_) => task_winners += 1,
            Err(WorkstreamClaimError::OwnedByAnother | WorkstreamClaimError::Overlap) => {}
            Err(error) => panic!("unexpected task-claim contention result: {error}"),
        }
    }
    assert_eq!(task_winners, 1);

    let idempotent = scoped_input(
        &pool,
        &group,
        &members[0].0,
        "retry-task-claim",
        "ignored.rs",
    )
    .await;
    let first = store.claim(idempotent.clone()).await.unwrap();
    let retry = store.claim(idempotent.clone()).await.unwrap();
    assert_eq!(retry.claim_id, first.claim_id);
    let mut conflicting = idempotent;
    conflicting.target = ClaimTarget::Path("src/other.rs".into());
    assert!(matches!(
        store.claim(conflicting).await,
        Err(WorkstreamClaimError::IdempotencyConflict)
    ));
}

#[tokio::test]
async fn dependencies_reject_cycles_and_expiry_replacement_fences_old_claim() {
    let (_dir, pool, group, members) = fixture().await;
    let run = members[0].0.clone();
    let store = WorkstreamClaimStore::new(pool.clone());
    let first = store
        .claim(scoped_input(&pool, &group, &run, "first", "src/first.rs").await)
        .await
        .unwrap();
    let second = store
        .claim(scoped_input(&pool, &group, &run, "second", "src/second.rs").await)
        .await
        .unwrap();
    let first = store
        .add_dependency(
            &first.claim_id,
            ClaimPrerequisite::Claim(second.claim_id.clone()),
            &ClaimActor::Run(run.clone()),
            1,
        )
        .await
        .unwrap();
    assert_eq!(first.version, 2);
    assert!(matches!(
        store
            .add_dependency(
                &second.claim_id,
                ClaimPrerequisite::Claim(first.claim_id),
                &ClaimActor::Run(run.clone()),
                1,
            )
            .await,
        Err(WorkstreamClaimError::DependencyCycle)
    ));

    sqlx::query("UPDATE workstream_claim SET lease_expires_at=? WHERE claim_id=?")
        .bind(chrono::Utc::now().timestamp_millis() - 1)
        .bind(second.claim_id.as_str())
        .execute(&pool)
        .await
        .unwrap();
    let replacement = store
        .claim(scoped_input(&pool, &group, &run, "replacement", "src/second.rs").await)
        .await
        .unwrap();
    assert_ne!(replacement.claim_id, second.claim_id);
    assert_eq!(replacement.lease_generation, 1);
    assert!(matches!(
        store
            .transition(ClaimCas {
                claim_id: second.claim_id,
                actor: ClaimActor::Run(run.clone()),
                expected_version: 1,
                expected_lease_generation: 1,
                transition: ClaimTransition::Renew,
                new_owner: None,
                lease_expires_at: Some(chrono::Utc::now().timestamp_millis() + 60_000),
            })
            .await,
        Err(WorkstreamClaimError::StaleRevision)
    ));
    let dependencies = store
        .list_dependencies(
            &group,
            &replacement.project_id,
            &replacement.workspace_id,
            &ClaimActor::Run(run),
        )
        .await
        .unwrap();
    assert_eq!(dependencies.len(), 1);
}

#[tokio::test]
async fn nonmember_actor_is_denied_and_schema_survives_reopen() {
    let (dir, pool, group, members) = fixture().await;
    let run = members[0].0.clone();
    let store = WorkstreamClaimStore::new(pool.clone());
    let mut unauthorized = scoped_input(&pool, &group, &run, "unauthorized", "src/file.rs").await;
    unauthorized.actor = ClaimActor::Run(AgentRunId::new());
    assert!(matches!(
        store.claim(unauthorized).await,
        Err(WorkstreamClaimError::Unauthorized)
    ));

    let db_path = dir.path().join("claims.db");
    pool.close().await;
    let options = SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))
        .unwrap()
        .foreign_keys(true);
    let reopened = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&reopened)
        .await
        .unwrap();
    let version: i64 = sqlx::query_scalar("SELECT version FROM migration_version WHERE id=1")
        .fetch_one(&reopened)
        .await
        .unwrap();
    assert_eq!(version, codegg_core::storage::STORAGE_LAYOUT_VERSION as i64);
    let tables: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN \
         ('workstream_claim','workstream_claim_dependency','workstream_claim_event')",
    )
    .fetch_one(&reopened)
    .await
    .unwrap();
    assert_eq!(tables, 3);
}

#[tokio::test]
async fn terminal_owner_cannot_transition_a_live_claim() {
    let (_dir, pool, group, members) = fixture().await;
    let (run, _) = &members[0];
    let store = WorkstreamClaimStore::new(pool.clone());
    let claim = store
        .claim(scoped_input(&pool, &group, run, "terminal-owner", "src/terminal.rs").await)
        .await
        .unwrap();
    sqlx::query("UPDATE agent_run SET status='completed' WHERE run_id=?")
        .bind(run.as_str())
        .execute(&pool)
        .await
        .unwrap();
    assert!(matches!(
        store
            .transition(ClaimCas {
                claim_id: claim.claim_id,
                actor: ClaimActor::Run(run.clone()),
                expected_version: 1,
                expected_lease_generation: 1,
                transition: ClaimTransition::Renew,
                new_owner: None,
                lease_expires_at: Some(chrono::Utc::now().timestamp_millis() + 60_000),
            })
            .await,
        Err(WorkstreamClaimError::Unauthorized)
    ));
}

#[tokio::test]
async fn renewed_claim_survives_reopen_and_rejects_pre_restart_cas() {
    let (dir, pool, group, members) = fixture().await;
    let (run, _) = &members[0];
    let store = WorkstreamClaimStore::new(pool.clone());
    let claim = store
        .claim(scoped_input(&pool, &group, run, "restart", "src/restart.rs").await)
        .await
        .unwrap();
    store
        .transition(ClaimCas {
            claim_id: claim.claim_id.clone(),
            actor: ClaimActor::Run(run.clone()),
            expected_version: 1,
            expected_lease_generation: 1,
            transition: ClaimTransition::Renew,
            new_owner: None,
            lease_expires_at: Some(chrono::Utc::now().timestamp_millis() + 120_000),
        })
        .await
        .unwrap();
    pool.close().await;

    let db_path = dir.path().join("claims.db");
    let reopened = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(
            SqliteConnectOptions::from_str(&format!("sqlite://{}", db_path.display()))
                .unwrap()
                .foreign_keys(true),
        )
        .await
        .unwrap();
    codegg_core::session::schema::migrate(&reopened)
        .await
        .unwrap();
    let reopened_store = WorkstreamClaimStore::new(reopened.clone());
    assert!(matches!(
        reopened_store
            .transition(ClaimCas {
                claim_id: claim.claim_id.clone(),
                actor: ClaimActor::Run(run.clone()),
                expected_version: 1,
                expected_lease_generation: 1,
                transition: ClaimTransition::Release,
                new_owner: None,
                lease_expires_at: None,
            })
            .await,
        Err(WorkstreamClaimError::StaleRevision)
    ));
    let current = reopened_store
        .list_group(
            &group,
            &claim.project_id,
            &claim.workspace_id,
            &ClaimActor::Run(run.clone()),
        )
        .await
        .unwrap();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].version, 2);
}
