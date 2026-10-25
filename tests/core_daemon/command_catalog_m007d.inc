//! M007-D: the daemon-owned command catalog and the daemon-effect command
//! executor.
//!
//! ADR-0052 (accepted 2026-10-24) moved command ownership into the daemon.
//! These tests cover the daemon half of that decision:
//!
//! * the daemon serves its own catalog, and the catalog it serves is the
//!   catalog the domain module holds;
//! * a command execution is **refused** — with a typed reason and no run —
//!   when the command is unknown, when the form is a frontend effect, when
//!   the catalog version is stale, or when the argument is over its bound;
//! * a daemon-effect execution is audited as a canonical `Run`, and the run
//!   survives the execution's outcome.
//!
//! The TUI half — that the TUI's commands keep their identity, their labels
//! and their keys — is covered by the TUI's own regression tests and by
//! `scripts/check_command_catalog_ownership.py`.

use super::common;

use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg_core::run_store::RunStore;
use codegg_protocol::core::{
    CommandCatalogScopeDto, CommandExecuteRequestDto, CoreRequest, CoreResponse, RequestEnvelope,
    PROTOCOL_VERSION,
};

const LOCAL_CLIENT: &str = "local-client";

/// A daemon with a durable run store, so a command execution can be audited.
fn daemon_with_run_store(pool: sqlx::SqlitePool, run_root: std::path::PathBuf) -> CoreDaemon {
    let store = Arc::new(codegg_core::run_store::FsRunStore::new(run_root));
    let deps = CoreRuntimeDeps::new(Some(pool), None, None).with_run_store(store);
    let daemon = CoreDaemon::with_deps(deps);
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "local".to_owned(),
        None,
        codegg_core::transport_auth::AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
    daemon
}

async fn migrated_pool() -> sqlx::SqlitePool {
    let pool = common::pool::isolated_pool().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    sqlx::query(
        r#"INSERT OR IGNORE INTO project (id, name, time_created, time_updated, sandboxes)
           VALUES (?, ?, ?, ?, ?)"#,
    )
    .bind("test-proj")
    .bind("test-proj")
    .bind(now)
    .bind(now)
    .bind("[]")
    .execute(&pool)
    .await
    .expect("seed project");
    pool
}

fn scope() -> CommandCatalogScopeDto {
    CommandCatalogScopeDto {
        project_id: "test-proj".to_string(),
        workspace_id: "test-workspace".to_string(),
    }
}

async fn read_catalog(daemon: &CoreDaemon) -> codegg_protocol::core::CommandCatalogDto {
    let response = Box::pin(daemon.handle_request_for_client(
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::CommandCatalog { scope: scope() },
        },
        LOCAL_CLIENT,
    ))
    .await
    .expect("the daemon must answer");
    match response {
        CoreResponse::CommandCatalog { catalog } => catalog,
        other => panic!("expected the command catalog, got {other:?}"),
    }
}

fn execute_request(command_id: &str, form: &str, argument: &str, version: u64) -> CoreRequest {
    CoreRequest::CommandExecute {
        request: CommandExecuteRequestDto {
            scope: scope(),
            command_id: command_id.to_string(),
            form: form.to_string(),
            argument: argument.to_string(),
            catalog_version: version,
        },
    }
}

async fn execute(
    daemon: &CoreDaemon,
    command_id: &str,
    form: &str,
    argument: &str,
    version: u64,
) -> codegg_protocol::core::CommandExecuteResultDto {
    // The daemon's request future is deep enough that one more `async fn`
    // layer over it costs the crate its default recursion budget, so the
    // response is matched here rather than through another helper.
    let response = Box::pin(daemon.handle_request_for_client(
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: execute_request(command_id, form, argument, version),
        },
        LOCAL_CLIENT,
    ))
    .await
    .expect("the daemon must answer");
    match response {
        CoreResponse::CommandExecute { result } => result,
        other => panic!("expected a command execution result, got {other:?}"),
    }
}

/// The daemon serves the catalog the domain module holds.
#[tokio::test]
async fn command_catalog_serves_the_daemon_catalog() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-catalog"));
    let catalog = read_catalog(&daemon).await;

    let expected = codegg_core::command_catalog::the_catalog();
    assert_eq!(
        catalog.catalog_version, expected.version,
        "the daemon must serve the catalog version the domain module holds"
    );
    assert_eq!(
        catalog.commands.len(),
        expected.commands.len(),
        "the daemon must serve every command the catalog defines"
    );
    for (served, defined) in catalog.commands.iter().zip(expected.commands.iter()) {
        assert_eq!(served.command_id, defined.command_id);
        assert_eq!(served.name, defined.name);
        assert_eq!(served.description, defined.description);
        assert_eq!(served.forms.len(), defined.forms.len());
    }
}

/// The catalog's classification is the reviewed one: 161 commands, 166
/// forms, 134 daemon-effect forms and 32 frontend-effect forms.
#[tokio::test]
async fn command_catalog_carries_the_reviewed_classification() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-classification"));
    let catalog = read_catalog(&daemon).await;

    use codegg_protocol::core::CommandKindDto;
    let daemon_effect = catalog
        .commands
        .iter()
        .flat_map(|command| command.forms.iter())
        .filter(|form| form.kind == CommandKindDto::DaemonEffect)
        .count();
    let frontend_effect = catalog
        .commands
        .iter()
        .flat_map(|command| command.forms.iter())
        .filter(|form| form.kind == CommandKindDto::FrontendEffect)
        .count();

    assert_eq!(
        catalog.commands.len(),
        161,
        "the catalog holds 161 commands"
    );
    assert_eq!(
        daemon_effect + frontend_effect,
        166,
        "the catalog holds 166 command forms"
    );
    assert_eq!(daemon_effect, 134, "134 forms are daemon effects");
    assert_eq!(frontend_effect, 32, "32 forms are frontend effects");
}

/// An unknown command identity is refused, with a reason and no run.
#[tokio::test]
async fn command_execute_refuses_an_unknown_command() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-unknown"));
    let version = codegg_core::command_catalog::catalog_version();
    let result = execute(&daemon, "no_such_command", "", "", version).await;

    assert_eq!(
        result.status,
        codegg_protocol::core::CommandOutcomeStatusDto::Refused,
        "an unknown command must be refused"
    );
    assert!(result.run_id.is_empty(), "a refusal must not open a run");
    assert!(
        result
            .lines
            .iter()
            .any(|line| line.contains("unknown_command")),
        "the refusal must name its reason, got {:?}",
        result.lines
    );
}

/// A frontend-effect form is refused: the daemon does not execute what the
/// frontend owns, and the refusal produces no run.
#[tokio::test]
async fn command_execute_refuses_a_frontend_effect() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-frontend"));
    let version = codegg_core::command_catalog::catalog_version();
    let result = execute(&daemon, "projects", "", "", version).await;

    assert_eq!(
        result.status,
        codegg_protocol::core::CommandOutcomeStatusDto::Refused,
        "a frontend effect must be refused by the daemon"
    );
    assert!(result.run_id.is_empty(), "a refusal must not open a run");
    assert!(
        result
            .lines
            .iter()
            .any(|line| line.contains("not_daemon_effect")),
        "the refusal must name its reason, got {:?}",
        result.lines
    );
}

/// A stale catalog version is refused with the current version, so the
/// frontend refetches rather than guessing.
#[tokio::test]
async fn command_execute_refuses_a_stale_catalog() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-stale"));
    let result = execute(&daemon, "reload", "", "", 0).await;

    assert_eq!(
        result.status,
        codegg_protocol::core::CommandOutcomeStatusDto::Refused,
        "a stale catalog version must be refused"
    );
    assert!(result.run_id.is_empty(), "a refusal must not open a run");
    assert!(
        result
            .lines
            .iter()
            .any(|line| line.contains("stale_catalog")),
        "the refusal must name its reason, got {:?}",
        result.lines
    );
    assert!(
        result
            .summary
            .contains(&codegg_core::command_catalog::catalog_version().to_string()),
        "the refusal must state the current catalog version, got {:?}",
        result.summary
    );
}

/// An over-long argument is refused before anything runs.
#[tokio::test]
async fn command_execute_refuses_an_over_long_argument() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-argument"));
    let version = codegg_core::command_catalog::catalog_version();
    let argument = "x".repeat(codegg_core::command_catalog::bounds::MAX_ARGUMENT_BYTES + 1);
    let result = execute(&daemon, "reload", "", &argument, version).await;

    assert_eq!(
        result.status,
        codegg_protocol::core::CommandOutcomeStatusDto::Refused,
        "an over-long argument must be refused"
    );
    assert!(result.run_id.is_empty(), "a refusal must not open a run");
    assert!(
        result
            .lines
            .iter()
            .any(|line| line.contains("argument_too_long")),
        "the refusal must name its reason, got {:?}",
        result.lines
    );
}

/// A daemon-effect execution is audited as a canonical `Run`: the run
/// exists even when the effect itself cannot be served, because the audit
/// records the attempt.
#[tokio::test]
async fn command_execute_audits_a_daemon_effect_as_a_run() {
    let pool = migrated_pool().await;
    let run_root = std::env::temp_dir().join("m007d-run");
    let daemon = daemon_with_run_store(pool, run_root.clone());
    let version = codegg_core::command_catalog::catalog_version();
    let result = execute(&daemon, "reload", "", "", version).await;

    assert!(
        !result.run_id.is_empty(),
        "a daemon-effect execution must be audited as a run, got {result:?}"
    );
    let store = codegg_core::run_store::FsRunStore::new(run_root);
    let run_id = codegg_core::run_store::RunId::new_unchecked(result.run_id.clone());
    let manifest = store
        .get_run(&run_id)
        .await
        .expect("read the run")
        .expect("the run must be recorded");
    assert_eq!(
        manifest.kind,
        codegg_core::run_store::RunKind::Command,
        "the run must be recorded as a command run"
    );
}
