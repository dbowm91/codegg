//! M002 — the authorized delegated-run transcript page read.
//!
//! Boundary proof for the plan: the read is gated twice, and the two gates
//! are different facts.
//!
//! - The **authorization** gate resolves the session's project and checks
//!   the caller's capability over it. A principal without a grant is
//!   refused before the handler runs, and the refusal says nothing about
//!   any run.
//! - The **ownership** gate re-resolves the run's own durable owner. A
//!   run the named session does not own gets the *same* answer as a run id
//!   that does not exist, so a guessed run id reveals neither the run's
//!   existence nor its size.
//!
//! The page itself is bounded and cursor-addressed: the tail past the
//! initial preview is reachable without re-fetching the session, and a
//! page never claims to be more complete than it is.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg_core::agent_run::{AgentRunBudget, NewAgentRun, NewAgentTask};
use codegg_core::agent_run_transcript::{
    AgentRunTranscriptStore, SqliteAgentRunTranscriptStore, TranscriptKind, TranscriptRole,
};
use codegg_core::authorization::ProjectRole;
use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};
use codegg_core::team::{PrincipalKind, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};
use codegg_core::workspace::WorkspaceId;
use codegg_protocol::projection::dto::AgentRunTranscriptPageDto;

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

/// A daemon whose durable run store and transcript store share one pool, so
/// the transcript's foreign key to the run row is satisfied.
async fn test_daemon(pool: sqlx::SqlitePool) -> CoreDaemon {
    CoreDaemon::with_deps(CoreRuntimeDeps::with_jobs(pool, None, None))
}

async fn human_token(team: &TeamStore, name: &str, client_id: &str) -> AuthenticatedPrincipal {
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .unwrap();
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .unwrap();
    tokens
        .verify_for_client(&plaintext, client_id)
        .await
        .unwrap()
}

fn register_client(daemon: &CoreDaemon, client_id: &str, principal: AuthenticatedPrincipal) {
    daemon.clients.register_with_principal(
        client_id.to_owned(),
        format!("{client_id}-name"),
        None,
        principal,
    );
}

fn error_code(response: &CoreResponse) -> &str {
    match response {
        CoreResponse::Error { code, .. } => code,
        other => panic!("expected error response, got {other:?}"),
    }
}

/// Give a session a project, so the authorization gate has a scope to
/// resolve.
async fn session_with_project(pool: &sqlx::SqlitePool, session_id: &str) -> ProjectId {
    let project = ProjectId::new();
    sqlx::query(
        "INSERT INTO project (id, worktree, time_created, time_updated, sandboxes) \
         VALUES (?, '/tmp', 1, 1, '[]')",
    )
    .bind(project.as_str())
    .execute(pool)
    .await
    .expect("insert the project row");
    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) \
         VALUES (?, ?, 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(session_id)
    .bind(project.as_str())
    .execute(pool)
    .await
    .expect("insert the session row");
    project
}

/// Start one delegated run owned by `session_id`.
async fn delegated_run(
    daemon: &CoreDaemon,
    session_id: &str,
    workspace_id: &WorkspaceId,
) -> AgentRunId {
    let run_id = AgentRunId::new();
    let task = NewAgentTask {
        task_id: AgentTaskId::new(),
        parent_task_id: None,
        originating_session_id: session_id.to_string(),
        originating_turn_id: Some(format!("turn-{session_id}")),
        project_id: ProjectId::new(),
        repository_id: None,
        workspace_id: workspace_id.clone(),
        requested_agent: "explore".into(),
        delegation_key: format!("delegation-{session_id}-{run_id}"),
        request_fingerprint: format!("fingerprint-{session_id}-{run_id}"),
        description: "a delegated run".into(),
    };
    let run = NewAgentRun {
        run_id: run_id.clone(),
        parent_run_id: None,
        depth: 1,
        workspace_id: workspace_id.clone(),
        agent_name: "explore".into(),
        agent_digest: Some("digest".into()),
        provider: "openai".into(),
        model: "model".into(),
        authority_digest: "authority".into(),
        budget: AgentRunBudget::default(),
        shared_scratch: Vec::new(),
    };
    daemon
        .deps
        .agent_run_store
        .create_or_get(task, run)
        .await
        .expect("start a delegated run");
    run_id
}

/// Append `count` transcript rows to a run.
async fn append_rows(pool: &sqlx::SqlitePool, run_id: &AgentRunId, count: usize, body: &str) {
    let store = SqliteAgentRunTranscriptStore::new(pool.clone());
    for index in 0..count {
        store
            .append(
                run_id,
                TranscriptRole::Orchestrator,
                TranscriptKind::ModelMessage,
                &format!("{body}-{index}"),
            )
            .await
            .expect("append a transcript row");
    }
}

fn workspace_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!(
        "codegg-m002-{name}-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&root).expect("create the workspace root");
    root
}

#[tokio::test(flavor = "current_thread")]
async fn the_owning_principal_reads_the_transcript_page() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = session_with_project(&pool, "session-owned").await;

    let workspace = daemon
        .workspaces
        .get_or_register(&workspace_root("owned"))
        .await
        .expect("register workspace");
    let run_id = delegated_run(&daemon, "session-owned", &workspace.id).await;
    append_rows(&pool, &run_id, 3, "hello").await;

    let viewer = human_token(&team, "Viewer", "client-viewer").await;
    let viewer_id = viewer.principal_id().clone();
    team.create_membership(&project, &viewer_id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-viewer", viewer);

    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-owned".to_owned(),
            CoreRequest::AgentRunTranscriptRead {
                session_id: "session-owned".to_owned(),
                run_id: run_id.to_string(),
                from_ordinal: 0,
                limit: 64,
            },
        ),
        "client-viewer",
    ))
    .await
    .unwrap();
    let CoreResponse::AgentRunTranscriptRead { page } = response else {
        panic!("expected a transcript page, got {response:?}");
    };
    assert_eq!(page.entries.len(), 3, "the page carries the recorded rows");
    assert_eq!(page.next_ordinal, 3);
    assert!(!page.has_more);
    assert_eq!(page.run_id, run_id.to_string());
}

#[tokio::test(flavor = "current_thread")]
async fn a_principal_without_a_grant_is_denied_before_the_run_is_consulted() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let _project = session_with_project(&pool, "session-ungranted").await;

    let workspace = daemon
        .workspaces
        .get_or_register(&workspace_root("ungranted"))
        .await
        .expect("register workspace");
    let run_id = delegated_run(&daemon, "session-ungranted", &workspace.id).await;
    append_rows(&pool, &run_id, 1, "secret").await;

    // A principal with no membership on the session's project.
    let outsider = human_token(&team, "Outsider", "client-outsider").await;
    register_client(&daemon, "client-outsider", outsider);

    let response = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-ungranted".to_owned(),
            CoreRequest::AgentRunTranscriptRead {
                session_id: "session-ungranted".to_owned(),
                run_id: run_id.to_string(),
                from_ordinal: 0,
                limit: 64,
            },
        ),
        "client-outsider",
    ))
    .await
    .unwrap();
    assert_eq!(
        error_code(&response),
        "authorization_denied",
        "a principal without a grant over the session's project is refused"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_granted_principal_reading_another_session_run_gets_the_not_found_refusal() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = session_with_project(&pool, "session-reader").await;
    let _other_project = session_with_project(&pool, "session-other").await;

    let workspace = daemon
        .workspaces
        .get_or_register(&workspace_root("reader"))
        .await
        .expect("register workspace");
    // A run owned by the *other* session.
    let foreign_run = delegated_run(&daemon, "session-other", &workspace.id).await;
    append_rows(&pool, &foreign_run, 2, "secret").await;

    let reader = human_token(&team, "Reader", "client-reader").await;
    let reader_id = reader.principal_id().clone();
    team.create_membership(&project, &reader_id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-reader", reader);

    let read_foreign = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-foreign".to_owned(),
            CoreRequest::AgentRunTranscriptRead {
                session_id: "session-reader".to_owned(),
                run_id: foreign_run.to_string(),
                from_ordinal: 0,
                limit: 64,
            },
        ),
        "client-reader",
    ))
    .await
    .unwrap();
    let read_missing = Box::pin(daemon.handle_request_for_client(
        new_request(
            "req-missing".to_owned(),
            CoreRequest::AgentRunTranscriptRead {
                session_id: "session-reader".to_owned(),
                run_id: AgentRunId::new().to_string(),
                from_ordinal: 0,
                limit: 64,
            },
        ),
        "client-reader",
    ))
    .await
    .unwrap();

    assert_eq!(
        error_code(&read_foreign),
        "agent_run_not_found",
        "a run the session does not own is refused"
    );
    assert_eq!(
        error_code(&read_foreign),
        error_code(&read_missing),
        "a guessed run id must not reveal whether the run exists"
    );
    let CoreResponse::Error { message, .. } = &read_foreign else {
        panic!("expected an error response");
    };
    let CoreResponse::Error {
        message: missing_message,
        ..
    } = &read_missing
    else {
        panic!("expected an error response");
    };
    assert_eq!(
        message, missing_message,
        "a guessed run id must not reveal whether the run exists"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_tail_past_the_preview_ceiling_is_reachable_by_cursor() {
    let pool = test_pool().await;
    let daemon = test_daemon(pool.clone()).await;
    let team = TeamStore::new(pool.clone());
    let project = session_with_project(&pool, "session-tail").await;

    let workspace = daemon
        .workspaces
        .get_or_register(&workspace_root("tail"))
        .await
        .expect("register workspace");
    let run_id = delegated_run(&daemon, "session-tail", &workspace.id).await;
    // More rows than one page may carry.
    append_rows(&pool, &run_id, 70, "row").await;

    let viewer = human_token(&team, "Viewer", "client-tail").await;
    let viewer_id = viewer.principal_id().clone();
    team.create_membership(&project, &viewer_id, ProjectRole::Viewer)
        .await
        .unwrap();
    register_client(&daemon, "client-tail", viewer);

    let read_page = |from_ordinal: i64| {
        Box::pin(daemon.handle_request_for_client(
            new_request(
                format!("req-tail-{from_ordinal}"),
                CoreRequest::AgentRunTranscriptRead {
                    session_id: "session-tail".to_owned(),
                    run_id: run_id.to_string(),
                    from_ordinal,
                    limit: 64,
                },
            ),
            "client-tail",
        ))
    };

    let first = read_page(0).await.unwrap();
    let CoreResponse::AgentRunTranscriptRead { page } = first else {
        panic!("expected a transcript page");
    };
    assert_eq!(
        page.entries.len(),
        AgentRunTranscriptPageDto::MAX_ROWS,
        "one page carries at most the declared row ceiling"
    );
    assert!(page.has_more, "the store holds rows beyond the first page");

    let second = read_page(page.next_ordinal).await.unwrap();
    let CoreResponse::AgentRunTranscriptRead { page: second_page } = second else {
        panic!("expected a transcript page");
    };
    assert!(
        !second_page.has_more,
        "the last page does not claim more rows"
    );
    assert!(
        !second_page.source.recording_stopped,
        "a run that recorded to the end did not stop recording"
    );

    let mut ordinals: Vec<i64> = page
        .entries
        .iter()
        .chain(second_page.entries.iter())
        .map(|entry| entry.ordinal)
        .collect();
    ordinals.sort_unstable();
    ordinals.dedup();
    assert_eq!(
        ordinals.len(),
        70,
        "the two pages must cover every recorded row exactly once, got {ordinals:?}"
    );
}
