//! M007-D: the daemon-owned command catalog and the daemon-effect command
//! executor.
//!
//! ADR-0052 (accepted 2026-10-24) moved command ownership into the daemon.
//! These tests cover the daemon half of that decision:
//!
//! * the daemon serves its own catalog, and the catalog it serves is the
//!   catalog the domain module holds;
//! * a command execution is **refused** — with a typed reason and no run —
//!   when the command is unknown, the form is a frontend effect, the
//!   catalog version is stale, or the argument is over its bound;
//! * a daemon-effect execution is audited as a canonical `Run`.
//!
//! The TUI half — that the TUI's commands keep their identity, their labels
//! and their keys — is covered by the TUI's own regression tests.

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
        session_id: Some("test-session".to_string()),
    }
}

/// A scope that names no session, for the commands that act on one.
fn sessionless_scope() -> CommandCatalogScopeDto {
    CommandCatalogScopeDto {
        project_id: "test-proj".to_string(),
        workspace_id: "test-workspace".to_string(),
        session_id: None,
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

/// The catalog's classification is the reviewed one: 161 commands, 167
/// forms, 124 daemon-effect forms and 43 frontend-effect forms.
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
        167,
        "the catalog holds 167 command forms"
    );
    assert_eq!(daemon_effect, 124, "124 forms are daemon effects");
    assert_eq!(frontend_effect, 43, "43 forms are frontend effects");
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

/// The catalog's families, as the domain names the catalog carries.
///
/// The regression coverage is organised by family, and the family list is
/// this constant: a family's coverage is one test, and the list is the
/// closure artifact's family column.
const FAMILIES: &[&str] = &[
    "Agent",
    "Collaboration",
    "Diagnostics",
    "Execution",
    "Git/Review",
    "Memory",
    "Project",
    "Provider",
    "Research",
    "Session",
    "System",
];

/// The probe argument a form needs, for the rows whose effect reads the
/// argument rather than the scope.
///
/// A row that acts on a session the **caller** named reads it from the
/// argument, so the probe has to name one. A row that reads only the scope
/// needs no argument.
fn probe_argument(command_id: &str) -> &'static str {
    match command_id {
        "share" | "unshare" | "export" | "observe" => "probe-session",
        _ => "",
    }
}

/// The catalog's daemon-effect forms in one family, with the flag that says
/// whether the daemon's effect table serves them.
async fn family_forms(daemon: &CoreDaemon, family: &str) -> Vec<(String, String, bool)> {
    let catalog = read_catalog(daemon).await;
    let mut forms = Vec::new();
    for command in &catalog.commands {
        for form in &command.forms {
            if form.kind != codegg_protocol::core::CommandKindDto::DaemonEffect {
                continue;
            }
            if command.domain == family {
                forms.push((
                    command.command_id.clone(),
                    form.form.clone(),
                    form.daemon_served,
                ));
            }
        }
    }
    forms
}

/// The catalog names a family for every command.
#[tokio::test]
async fn command_catalog_names_a_family_for_every_command() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-families"));
    let catalog = read_catalog(&daemon).await;

    let mut seen: Vec<String> = Vec::new();
    for command in &catalog.commands {
        let domain = command.domain.clone();
        assert!(
            !domain.is_empty(),
            "{} must name a family",
            command.command_id
        );
        seen.push(domain.clone());
    }
    for family in FAMILIES {
        assert!(
            seen.iter().any(|domain| domain == family),
            "the {family} family must hold at least one command"
        );
    }
}

/// Every daemon-effect form the catalog marks as served is served by the
/// daemon's effect table.
///
/// This is the property the palette depends on: a form the catalog marks
/// served must not be refused with `command_not_served`. The test is
/// organised by family, and it asserts the family boundary in both
/// directions — the served set the catalog publishes is the set the daemon
/// serves.
#[tokio::test]
async fn command_catalog_serves_every_form_it_marks_served() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-served"));
    let version = codegg_core::command_catalog::catalog_version();

    let mut checked = 0usize;
    for family in FAMILIES {
        for (command_id, form, daemon_served) in family_forms(&daemon, family).await {
            let result = execute(
                &daemon,
                &command_id,
                &form,
                probe_argument(&command_id),
                version,
            )
            .await;
            let refused_as_unserved = result
                .lines
                .iter()
                .any(|line| line.contains("command_not_served"));
            if daemon_served {
                assert!(
                    !refused_as_unserved,
                    "the {family} family marks {command_id}/{form} as served, \
                     but the daemon refused it as not served"
                );
                // A row that reads a number the empty probe did not supply
                // refuses the *call*, with its own reason. Serve-ability is
                // a property of the form, so that refusal must not be
                // reported as "not served" — and, like every refusal, it
                // opens no run.
                let refused_as_unreadable = result
                    .lines
                    .iter()
                    .any(|line| line.contains("argument_unreadable"));
                if refused_as_unreadable {
                    assert!(
                        result.run_id.is_empty(),
                        "a refusal must not open a run, got {result:?}"
                    );
                } else {
                    assert!(
                        !result.run_id.is_empty(),
                        "the {family} family's {command_id}/{form} must open a run"
                    );
                }
                checked += 1;
            } else {
                assert!(
                    refused_as_unserved,
                    "the {family} family does not mark {command_id}/{form} as served, \
                     so the daemon must refuse it as not served, got {result:?}"
                );
                assert!(
                    result.run_id.is_empty(),
                    "a refusal must not open a run, got {result:?}"
                );
            }
        }
    }
    assert!(checked > 0, "the served set must not be empty");
}

/// A command that acts on a session refuses when the request names no
/// session, with a reason distinct from "not served".
#[tokio::test]
async fn command_execute_refuses_a_session_command_without_a_session() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-nosession"));
    let version = codegg_core::command_catalog::catalog_version();

    let response = Box::pin(daemon.handle_request_for_client(
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::CommandExecute {
                request: CommandExecuteRequestDto {
                    scope: sessionless_scope(),
                    command_id: "share".to_string(),
                    form: String::new(),
                    argument: String::new(),
                    catalog_version: version,
                },
            },
        },
        LOCAL_CLIENT,
    ))
    .await
    .expect("the daemon must answer");
    let result = match response {
        CoreResponse::CommandExecute { result } => result,
        other => panic!("expected a command execution result, got {other:?}"),
    };

    assert_eq!(
        result.status,
        codegg_protocol::core::CommandOutcomeStatusDto::Refused,
        "a session command without a session must be refused"
    );
    assert!(
        result.lines.iter().any(|line| line.contains("no_session")),
        "the refusal must name its reason, got {:?}",
        result.lines
    );
    assert!(
        !result
            .lines
            .iter()
            .any(|line| line.contains("command_not_served")),
        "the daemon serves the form; it must not claim otherwise"
    );
    assert!(result.run_id.is_empty(), "a refusal must not open a run");
}

/// The catalog publishes the daemon's own serve-ability, so a frontend can
/// show a command the daemon does not serve as unavailable instead of
/// dispatching into a refusal.
#[tokio::test]
async fn command_catalog_publishes_the_daemons_serve_ability() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-ability"));
    let catalog = read_catalog(&daemon).await;

    let mut served = 0usize;
    let mut frontend = 0usize;
    for command in &catalog.commands {
        for form in &command.forms {
            if form.kind == codegg_protocol::core::CommandKindDto::FrontendEffect {
                // The frontend serves a frontend effect itself, so the
                // daemon serves none of them.
                assert!(
                    !form.daemon_served,
                    "{} is a frontend effect and the daemon must not claim to serve it",
                    command.command_id
                );
                frontend += 1;
            } else if form.daemon_served {
                served += 1;
            }
        }
    }
    assert_eq!(frontend, 43, "43 forms are frontend effects");
    assert!(served > 0, "the daemon must publish the forms it serves");
}

/// The catalog publishes the action that serves each frontend-effect form:
/// the kind says **who** owns the effect, the action says **what** to do.
#[tokio::test]
async fn command_catalog_publishes_the_action_of_every_frontend_effect() {
    let pool = migrated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007d-actions"));
    let catalog = read_catalog(&daemon).await;

    let mut frontend = 0usize;
    for command in &catalog.commands {
        for form in &command.forms {
            match form.kind {
                codegg_protocol::core::CommandKindDto::FrontendEffect => {
                    let published = form.action.as_deref().unwrap_or_else(|| {
                        panic!(
                            "{}/{} is a frontend effect and must name the action that serves it",
                            command.command_id, form.form
                        )
                    });
                    let declared = codegg_core::frontend_action::action_for_form(
                        &command.command_id,
                        &form.form,
                    )
                    .map(|action| action.as_str().to_string());
                    assert_eq!(
                        Some(published.to_string()),
                        declared,
                        "{}/{} must publish the declared action's own wire name",
                        command.command_id,
                        form.form
                    );
                    frontend += 1;
                }
                codegg_protocol::core::CommandKindDto::DaemonEffect => {
                    assert!(
                        form.action.is_none(),
                        "{}/{} is a daemon effect and names no frontend action",
                        command.command_id,
                        form.form
                    );
                }
            }
        }
    }
    assert_eq!(frontend, 43, "43 forms are frontend effects");
}
