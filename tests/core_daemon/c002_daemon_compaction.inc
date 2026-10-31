//! C002 (ADR-0060): the daemon compacts on a frontend's request.
//!
//! The daemon owns compaction: a frontend's request reaches the daemon, the
//! daemon records it against the session, and the daemon's answer names the
//! session it compacts. The only refusal is factual — a session that cannot
//! be resolved has nothing to compact (ADR-0060 §3).

use super::common;

use codegg::core::daemon::CoreDaemon;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg_core::audit::{AuditQueryFilter, AuditStore};
use codegg_protocol::core::{
    CommandCatalogScopeDto, CommandExecuteRequestDto, CoreRequest, CoreResponse, RequestEnvelope,
    PROTOCOL_VERSION,
};

const LOCAL_CLIENT: &str = "c002-local-client";
const WORKSPACE_ID: &str = "c002-workspace";
const PROJECT_ID: &str = "c002-project";

fn register_local_client(daemon: &CoreDaemon) {
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "c002-local".to_owned(),
        None,
        codegg_core::transport_auth::AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
}

/// A daemon over the given pool, with a durable run store so a command
/// invocation can be audited.
fn daemon_with_pool(pool: sqlx::SqlitePool, run_root: std::path::PathBuf) -> CoreDaemon {
    let store = std::sync::Arc::new(codegg_core::run_store::FsRunStore::new(run_root));
    let deps = CoreRuntimeDeps::new(Some(pool), None, None).with_run_store(store);
    let daemon = CoreDaemon::with_deps(deps);
    register_local_client(&daemon);
    daemon
}

/// Seed a workspace, a project bound to it, and a session bound to both.
async fn seed_session(pool: &sqlx::SqlitePool, session_id: &str) {
    let now = chrono::Utc::now().timestamp_millis();
    let root = std::env::temp_dir().join(format!("c002-{session_id}"));
    std::fs::create_dir_all(&root).expect("the workspace root must exist");

    sqlx::query(
        "INSERT OR IGNORE INTO logical_project (id, display_name, lifecycle, time_created, \
         time_updated, archived_at) VALUES (?, ?, 'active', ?, ?, NULL)",
    )
    .bind(PROJECT_ID)
    .bind(PROJECT_ID)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("the logical project must be seeded");

    sqlx::query(
        "INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened, \
         time_archived) VALUES (?, ?, ?, ?, ?, NULL)",
    )
    .bind(WORKSPACE_ID)
    .bind(root.to_string_lossy().to_string())
    .bind("c002 workspace")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("the workspace must be registered");

    sqlx::query(
        "INSERT INTO workspace_project_binding (workspace_id, project_id, repository_id, \
         worktree_id, node_id, locator, status, source, revision, time_created, time_updated) \
         VALUES (?, ?, NULL, NULL, NULL, ?, 'resolved', 'c002-test', 1, ?, ?)",
    )
    .bind(WORKSPACE_ID)
    .bind(PROJECT_ID)
    .bind(root.to_string_lossy().to_string())
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("the workspace binding must be seeded");

    // The session row's project is the legacy project the session was
    // created under; the logical project is the one the daemon's binding
    // resolves through.
    sqlx::query(
        "INSERT INTO project (id, worktree, vcs, name, time_created, time_updated, sandboxes) \
         VALUES (?, ?, 'git', 'c002 project', ?, ?, '[]')",
    )
    .bind(PROJECT_ID)
    .bind(root.to_string_lossy().to_string())
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("the legacy project must be seeded");

    sqlx::query(
        "INSERT INTO session (id, project_id, slug, directory, title, version, time_created, time_updated) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(session_id)
    .bind(PROJECT_ID)
    .bind("c002-session")
    .bind(root.to_string_lossy().to_string())
    .bind("c002 session")
    .bind("c002-version")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("the session must be seeded");

    sqlx::query(
        "INSERT INTO session_project_binding \
         (session_id, project_id, workspace_id, status, source, revision, time_created, time_updated) \
         VALUES (?, ?, ?, 'resolved', 'c002-test', 1, ?, ?)",
    )
    .bind(session_id)
    .bind(PROJECT_ID)
    .bind(WORKSPACE_ID)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("the session binding must be seeded");
}

async fn send(daemon: &CoreDaemon, payload: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload,
        },
        LOCAL_CLIENT,
    ))
    .await
    .expect("the daemon must answer")
}

/// The daemon accepts the request and records it against the session.
#[tokio::test]
async fn the_daemon_accepts_a_compaction_request_and_records_it() {
    let pool = common::pool::isolated_pool().await;
    seed_session(&pool, "c002-session").await;
    let daemon = daemon_with_pool(pool, std::env::temp_dir().join("c002-runs-accept"));

    let answer = send(
        &daemon,
        CoreRequest::SessionCompact {
            session_id: "c002-session".to_string(),
        },
    )
    .await;

    assert!(
        matches!(
            &answer,
            CoreResponse::SessionCompact { session_id, .. } if session_id == "c002-session"
        ),
        "the daemon must answer with the session it compacts: {answer:?}"
    );

    let runtime = daemon
        .sessions
        .get("c002-session")
        .expect("the session must have a runtime");
    assert!(
        runtime
            .compaction_requested
            .load(std::sync::atomic::Ordering::Acquire),
        "the request must be recorded against the session's runtime"
    );
}

/// The daemon refuses a request it cannot answer, and says so.
#[tokio::test]
async fn the_daemon_refuses_a_compaction_request_for_an_unknown_session() {
    let pool = common::pool::isolated_pool().await;
    let daemon = daemon_with_pool(pool, std::env::temp_dir().join("c002-runs-refuse"));

    let answer = send(
        &daemon,
        CoreRequest::SessionCompact {
            session_id: "c002-missing".to_string(),
        },
    )
    .await;

    match &answer {
        CoreResponse::Error { code, message } => {
            assert_eq!(code, "session_unavailable", "the refusal must be named");
            assert!(
                !message.is_empty(),
                "the refusal must carry the daemon's reason"
            );
        }
        other => panic!("the daemon must refuse an unknown session: {other:?}"),
    }
}

/// The catalog's own command reaches the same handler (ADR-0052).
#[tokio::test]
async fn the_compact_command_reaches_the_daemons_compaction_handler() {
    let pool = common::pool::isolated_pool().await;
    seed_session(&pool, "c002-session").await;
    let daemon = daemon_with_pool(pool, std::env::temp_dir().join("c002-runs-command"));

    let answer = send(
        &daemon,
        CoreRequest::CommandExecute {
            request: CommandExecuteRequestDto {
                scope: CommandCatalogScopeDto {
                    project_id: PROJECT_ID.to_string(),
                    workspace_id: WORKSPACE_ID.to_string(),
                    session_id: Some("c002-session".to_string()),
                },
                command_id: "compact".to_string(),
                form: String::new(),
                argument: String::new(),
                catalog_version: codegg_core::command_catalog::catalog_version(),
            },
        },
    )
    .await;

    let CoreResponse::CommandExecute { result } = &answer else {
        panic!("the command must answer with its result: {answer:?}");
    };
    assert_eq!(
        result.status,
        codegg_protocol::core::CommandOutcomeStatusDto::Completed,
        "the command must complete: {result:?}"
    );
    let inner = result
        .response
        .as_ref()
        .expect("the command's effect must answer");
    assert_eq!(
        inner["type"], "session_compact",
        "the command must reach the compaction handler: {inner}"
    );
    assert_eq!(
        inner["session_id"], "c002-session",
        "the command must name the session it compacts: {inner}"
    );
}

/// The request is audited under its own action, on the route a frontend
/// takes: the catalog's command.
///
/// The row is written by the handler, not by the generic seam, because the
/// command reaches the handler without passing that seam. One request leaves
/// one row.
#[tokio::test]
async fn the_compaction_request_is_audited_under_its_own_action() {
    let pool = common::pool::isolated_pool().await;
    seed_session(&pool, "c002-session").await;
    let daemon = daemon_with_pool(pool.clone(), std::env::temp_dir().join("c002-runs-audit"));

    let answer = send(
        &daemon,
        CoreRequest::CommandExecute {
            request: CommandExecuteRequestDto {
                scope: CommandCatalogScopeDto {
                    project_id: PROJECT_ID.to_string(),
                    workspace_id: WORKSPACE_ID.to_string(),
                    session_id: Some("c002-session".to_string()),
                },
                command_id: "compact".to_string(),
                form: String::new(),
                argument: String::new(),
                catalog_version: codegg_core::command_catalog::catalog_version(),
            },
        },
    )
    .await;
    assert!(
        matches!(answer, CoreResponse::CommandExecute { .. }),
        "the command must be served: {answer:?}"
    );

    let page = AuditStore::new(pool)
        .query(&AuditQueryFilter::new(None).with_action("session_compact"))
        .await
        .expect("the audit store must answer");
    let event = page
        .events
        .iter()
        .find(|event| event.action == "session_compact")
        .expect("the compaction request must be audited under its own action");
    assert_eq!(
        event.metadata.get("session.id").map(String::as_str),
        Some("c002-session"),
        "the audited row must name the session that was asked to compact"
    );
    // The local client is bound as the local owner, so the row names that
    // principal — never a client-supplied identity.
    assert_eq!(
        event.actor_principal.as_str(),
        "local-owner",
        "the audited row must name the principal that asked"
    );
}
