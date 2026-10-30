//! M007-G (ADR-0057): the daemon's plugin catalogue and its four mutating
//! plugin forms. The daemon's plugin registry is the single authority for the
//! catalogue; these tests cover the daemon half: the four mutating forms are
//! served and audited as canonical `Run`s, and the narrow `plugin.manage`
//! capability authorises them.

use super::common;

use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg_core::authorization::{
    operation_descriptor, AuthorizationRequest, AuthorizationService, ProjectRole, ScopeKind,
};
use codegg_core::identity::ProjectId;
use codegg_core::run_store::RunStore;
use codegg_core::team::{PrincipalKind, TeamStore};
use codegg_protocol::core::{
    CommandCatalogScopeDto, CommandExecuteRequestDto, CoreRequest, CoreResponse, RequestEnvelope,
    PROTOCOL_VERSION,
};
use codegg_protocol::plugin_catalogue::{PluginManageActionDto, PluginManageRequestDto};

const LOCAL_CLIENT: &str = "local-client";

/// A daemon with a durable run store, so a plugin mutation can be audited.
fn daemon_with_plugin_run_store(
    pool: sqlx::SqlitePool,
    run_root: std::path::PathBuf,
) -> CoreDaemon {
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

async fn execute(
    daemon: &CoreDaemon,
    command_id: &str,
    argument: &str,
) -> codegg_protocol::core::CommandExecuteResultDto {
    let version = codegg_core::command_catalog::catalog_version();
    let response = Box::pin(daemon.handle_request_for_client(
        RequestEnvelope {
            protocol_version: PROTOCOL_VERSION,
            request_id: uuid::Uuid::new_v4().to_string(),
            payload: CoreRequest::CommandExecute {
                request: CommandExecuteRequestDto {
                    scope: scope(),
                    command_id: command_id.to_string(),
                    form: String::new(),
                    argument: argument.to_string(),
                    catalog_version: version,
                },
            },
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

/// M007-G (ADR-0057): each of the four mutating plugin forms is served by
/// the daemon and audited as a canonical `Run`. The plugin does not exist, so
/// the effect fails; the run is still opened and closed, which is the property
/// under test.
#[tokio::test(flavor = "current_thread")]
async fn the_four_mutating_plugin_forms_are_served_and_audited_as_runs() {
    let pool = migrated_pool().await;
    let run_root = std::env::temp_dir().join("m007g-plugin-run");
    let daemon = daemon_with_plugin_run_store(pool, run_root.clone());

    for command_id in [
        "plugin_enable",
        "plugin_disable",
        "plugin_remove",
        "plugin_install",
    ] {
        let result = execute(&daemon, command_id, "probe-plugin").await;
        assert!(
            !result.run_id.is_empty(),
            "{command_id} must be served and audited as a run, got {result:?}"
        );
        let store = codegg_core::run_store::FsRunStore::new(run_root.clone());
        let run_id = codegg_core::run_store::RunId::new_unchecked(result.run_id.clone());
        let manifest = store
            .get_run(&run_id)
            .await
            .expect("read the run")
            .expect("the run must be recorded");
        assert_eq!(
            manifest.kind,
            codegg_core::run_store::RunKind::Command,
            "{command_id} must be recorded as a command run"
        );
    }
}

/// M007-G (ADR-0057): the narrow `plugin.manage` capability authorises the
/// mutating plugin forms, and a role without it is refused. A Contributor
/// holds `command.execute` and is still refused: running a command is not
/// authority to install code.
#[tokio::test(flavor = "current_thread")]
async fn the_mutating_plugin_forms_require_the_plugin_manage_capability() {
    let pool = common::pool::isolated_pool().await;
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate the team pool");
    let team = TeamStore::new(pool);
    let service = AuthorizationService::new(team.clone());
    let project = ProjectId::new();

    let descriptor = operation_descriptor(&CoreRequest::PluginManage {
        request: PluginManageRequestDto {
            session_id: "session-1".to_string(),
            action: PluginManageActionDto::Install,
            target: "/tmp/plugin".to_string(),
        },
    });
    assert_eq!(descriptor.scope_kind, ScopeKind::ViaSession);

    for (role, allowed) in [
        (ProjectRole::Viewer, false),
        (ProjectRole::Contributor, false),
        (ProjectRole::Maintainer, true),
        (ProjectRole::Owner, true),
    ] {
        // The principal is a **human**: the local owner's broad policy
        // bypasses the capability check, so it would pass every role.
        let tokens = codegg_core::transport_auth::PersonalTokenStore::with_team(
            team.pool().clone(),
            team.clone(),
        );
        let record = team
            .create_principal(PrincipalKind::Human, role.as_str())
            .await
            .expect("create the principal");
        let (plaintext, _) = tokens
            .create_personal_token(&record.id, "device", None)
            .await
            .expect("create the personal token");
        let principal = tokens
            .verify_for_client(&plaintext, "client-plugin")
            .await
            .expect("verify the personal token");
        team.create_membership(&project, &record.id, role)
            .await
            .expect("grant the membership");
        let decision = service
            .authorize(&AuthorizationRequest::new(
                principal,
                descriptor,
                Some(project.clone()),
                "corr-plugin",
            ))
            .await;
        assert_eq!(
            decision.is_ok(),
            allowed,
            "{} must {} the plugin management operation",
            role.as_str(),
            if allowed { "be allowed" } else { "be refused" }
        );
    }
}
