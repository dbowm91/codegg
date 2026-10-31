//! M007-I (work package A): the daemon's own diff read.
//!
//! The diff view used to be a frontend that spawned `git` and read the
//! working tree itself. It is now a daemon effect: the daemon resolves the
//! checkout from its own durable binding, reads the bytes through the
//! read-only git fact layer, and answers with the diff's own bytes.
//!
//! Three properties are proved here:
//!
//! 1. the daemon's answer carries the **same bytes** the frontend's own read
//!    produced, for **both** branches — `/diff <path>` and the bare `/diff`;
//! 2. every diff invocation opens a **canonical `Run`** that is readable back
//!    from the durable run store;
//! 3. the frontend's arm is **gone** — the catalog form is daemon-served and
//!    the frontend no longer constructs the read.

use super::common;

use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg_core::run_store::RunStore;
use codegg_protocol::core::{
    CommandCatalogScopeDto, CommandExecuteRequestDto, CoreRequest, CoreResponse, RequestEnvelope,
    PROTOCOL_VERSION,
};
use codegg_protocol::git::GitDiffRequestDto;

const LOCAL_CLIENT: &str = "m007i-local-client";

fn register_local_client(daemon: &CoreDaemon) {
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "m007i-local".to_owned(),
        None,
        codegg_core::transport_auth::AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
}

/// A daemon with a durable run store, so a diff invocation can be audited.
fn daemon_with_run_store(pool: sqlx::SqlitePool, run_root: std::path::PathBuf) -> CoreDaemon {
    let store = Arc::new(codegg_core::run_store::FsRunStore::new(run_root));
    let deps = CoreRuntimeDeps::new(Some(pool), None, None).with_run_store(store);
    let daemon = CoreDaemon::with_deps(deps);
    register_local_client(&daemon);
    daemon
}

async fn request(daemon: &CoreDaemon, payload: CoreRequest) -> CoreResponse {
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

/// Bind `project_id` to `root` so the daemon resolves the checkout itself.
///
/// The binding is the daemon's own durable authority: the request names a
/// project, never a directory.
async fn bind_project(pool: &sqlx::SqlitePool, project_id: &str, root: &std::path::Path) {
    let now = chrono::Utc::now().timestamp_millis();
    sqlx::query(
        "INSERT OR IGNORE INTO logical_project (id, display_name, lifecycle, time_created, \
         time_updated, archived_at) VALUES (?, ?, 'active', ?, ?, NULL)",
    )
    .bind(project_id)
    .bind(project_id)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("seed the logical project");
    sqlx::query(
        "INSERT INTO workspace (id, canonical_root, display_name, time_created, time_last_opened, \
         time_archived) VALUES (?, ?, ?, ?, ?, NULL)",
    )
    .bind("m007i-workspace")
    .bind(root.to_string_lossy().as_ref())
    .bind("m007i")
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("seed the workspace");
    sqlx::query(
        "INSERT INTO workspace_project_binding (workspace_id, project_id, repository_id, \
         worktree_id, node_id, locator, status, source, revision, time_created, time_updated) \
         VALUES (?, ?, NULL, NULL, NULL, ?, 'resolved', 'test', 1, ?, ?)",
    )
    .bind("m007i-workspace")
    .bind(project_id)
    .bind(root.to_string_lossy().as_ref())
    .bind(now)
    .bind(now)
    .execute(pool)
    .await
    .expect("bind the workspace to the project");
}

/// A checkout with one committed file whose working-tree bytes differ from
/// `HEAD`, so both diff branches have something to report.
fn checkout_with_a_change() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a checkout root");
    let path = root.path();
    common::blackboard::git(&["init", "--quiet", "--initial-branch=main"], path);
    common::blackboard::git(&["config", "user.email", "test@example.com"], path);
    common::blackboard::git(&["config", "user.name", "Test"], path);
    std::fs::write(path.join("README"), "base\n").expect("write the base file");
    common::blackboard::git(&["add", "."], path);
    common::blackboard::git(&["commit", "--quiet", "-m", "base"], path);
    std::fs::write(path.join("README"), "changed\n").expect("change the working tree");
    root
}

/// The daemon's diff answer for one request.
async fn diff(
    daemon: &CoreDaemon,
    project_id: &str,
    workspace_id: &str,
    path: Option<&str>,
) -> codegg_protocol::git::GitDiffResultDto {
    let response = request(
        daemon,
        CoreRequest::GitDiff {
            request: GitDiffRequestDto {
                project_id: project_id.to_owned(),
                workspace_id: workspace_id.to_owned(),
                session_id: None,
                path: path.map(str::to_owned),
            },
        },
    )
    .await;
    match response {
        CoreResponse::GitDiff { result } => result,
        other => panic!("expected a diff answer, got {other:?}"),
    }
}

/// The bytes the frontend's own read produced, for the `/diff <path>` branch.
///
/// The frontend read `git show HEAD:<path>` for the old side and the working
/// tree's own bytes for the new side. The daemon must answer with the same
/// two strings.
fn frontend_path_branch_bytes(root: &std::path::Path, path: &str) -> (String, String) {
    let old_content = String::from_utf8_lossy(
        &common::blackboard::git(&["show", &format!("HEAD:{path}")], root).stdout,
    )
    .to_string();
    let new_content = std::fs::read_to_string(root.join(path)).expect("read the working-tree file");
    (old_content, new_content)
}

/// The bytes the frontend's own read produced, for the bare `/diff` branch.
///
/// The frontend read one `git diff` and put it on the new side; the old side
/// was empty.
fn frontend_bare_branch_bytes(root: &std::path::Path) -> (String, String) {
    let diff_text =
        String::from_utf8_lossy(&common::blackboard::git(&["diff"], root).stdout).to_string();
    (String::new(), diff_text)
}

/// The daemon's diff answer carries the same bytes the frontend's read did.
///
/// The comparison is made against the frontend's own two reads, so a change
/// in either side's bytes fails the test rather than passing it.
#[tokio::test(flavor = "current_thread")]
async fn the_daemons_diff_answer_carries_the_frontends_own_bytes() {
    let pool = common::pool::isolated_pool().await;
    let checkout = checkout_with_a_change();
    bind_project(&pool, "m007i-project", checkout.path()).await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007i-bytes"));

    // The `/diff <path>` branch.
    let (expected_old, expected_new) = frontend_path_branch_bytes(checkout.path(), "README");
    let result = diff(&daemon, "m007i-project", "m007i-workspace", Some("README")).await;
    assert_eq!(
        result.old_content, expected_old,
        "the HEAD side must be the bytes the frontend read"
    );
    assert_eq!(
        result.new_content, expected_new,
        "the working-tree side must be the bytes the frontend read"
    );
    assert_eq!(
        result.path.as_deref(),
        Some("README"),
        "the answer names the path that was diffed"
    );

    // The bare `/diff` branch.
    let (expected_old, expected_new) = frontend_bare_branch_bytes(checkout.path());
    let result = diff(&daemon, "m007i-project", "m007i-workspace", None).await;
    assert_eq!(
        result.old_content, expected_old,
        "the bare branch has no HEAD side"
    );
    assert_eq!(
        result.new_content, expected_new,
        "the bare branch carries the whole-tree diff the frontend read"
    );
    assert!(
        result.path.is_none(),
        "the bare branch names no path, got {:?}",
        result.path
    );
}

/// Every diff invocation opens a canonical `Run`.
///
/// The run is read back from the durable run store, so this proves the audit
/// record exists rather than that the daemon merely reported an id.
#[tokio::test(flavor = "current_thread")]
async fn a_diff_invocation_opens_a_canonical_run() {
    let pool = common::pool::isolated_pool().await;
    let checkout = checkout_with_a_change();
    bind_project(&pool, "m007i-project", checkout.path()).await;
    let run_root = std::env::temp_dir().join("m007i-run");
    let daemon = daemon_with_run_store(pool, run_root.clone());
    let version = codegg_core::command_catalog::catalog_version();

    let response = request(
        &daemon,
        CoreRequest::CommandExecute {
            request: CommandExecuteRequestDto {
                scope: CommandCatalogScopeDto {
                    project_id: "m007i-project".to_owned(),
                    workspace_id: "m007i-workspace".to_owned(),
                    session_id: None,
                },
                command_id: "diff".to_owned(),
                form: String::new(),
                argument: "README".to_owned(),
                catalog_version: version,
            },
        },
    )
    .await;
    let result = match response {
        CoreResponse::CommandExecute { result } => result,
        other => panic!("expected a command execution result, got {other:?}"),
    };
    assert!(
        !result.run_id.is_empty(),
        "a diff invocation must be audited as a run, got {result:?}"
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

/// The frontend's arm is gone.
///
/// Two properties: the catalog publishes the form as daemon-served, and the
/// frontend's own source no longer constructs the read — the bespoke
/// `handle_diff_command` path is gone, and the frontend dispatches the form
/// to the daemon.
#[tokio::test(flavor = "current_thread")]
async fn the_frontends_diff_arm_is_gone() {
    // The catalog form is daemon-served.
    let pool = common::pool::isolated_pool().await;
    let daemon = daemon_with_run_store(pool, std::env::temp_dir().join("m007i-arm"));
    let response = request(
        &daemon,
        CoreRequest::CommandCatalog {
            scope: CommandCatalogScopeDto {
                project_id: "m007i-project".to_owned(),
                workspace_id: "m007i-workspace".to_owned(),
                session_id: None,
            },
        },
    )
    .await;
    let catalog = match response {
        CoreResponse::CommandCatalog { catalog } => catalog,
        other => panic!("expected the command catalog, got {other:?}"),
    };
    let diff_form = catalog
        .commands
        .iter()
        .find(|command| command.command_id == "diff")
        .expect("the catalog holds the diff command")
        .forms
        .iter()
        .find(|form| form.form.is_empty())
        .expect("the diff command holds its empty form");
    assert!(
        diff_form.daemon_served,
        "the diff form must be published as daemon-served"
    );

    // The frontend no longer constructs the read.
    let frontend = include_str!("../../src/tui/app/mod.rs");
    assert!(
        !frontend.contains("handle_diff_command"),
        "the frontend's bespoke diff arm must be gone"
    );
    assert!(
        !frontend.contains("\"git\".into(), \"diff\".into()"),
        "the frontend must not spawn git for the diff view"
    );
    let route = codegg::tui::command_route::route_for("diff", "");
    assert!(
        matches!(
            route,
            codegg::tui::command_route::CommandRoute::DaemonEffect { .. }
        ),
        "the frontend must route the diff form to the daemon, got {route:?}"
    );
}
