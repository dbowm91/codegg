//! M006 — the scratch usage census and operator visibility.
//!
//! Boundary proof for the plan: an operator can see what the daemon's own
//! scratch tree is using, and the answer is honest about what it does not
//! know.
//!
//! - The census is **read-only**. It measures; it never creates, renames,
//!   unlinks, or writes anything, and it never reclaims.
//! - The reclaim verdict comes from the **same rule** the reclaim validator
//!   applies, so a report cannot disagree with the behaviour it describes.
//! - The report is **bounded**. A budget-truncated walk says so, and a
//!   partial walk never claims a tree is reclaimable.
//! - The report discloses **no** absolute host path, no session identity,
//!   and no file content.
//! - The walk runs on the daemon's own cadence, **off the hot path**: a
//!   read never creates a job and never touches the queue.
//! - The request is scoped to no project, so a team principal fails closed
//!   before the census is reached.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::core::runtime_deps::CoreRuntimeDeps;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::dto::ScratchUsageReportDto;
use codegg_core::identity::ProjectId;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

const LOCAL_CLIENT: &str = "client-local";

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

/// A sessions root holding one managed tree, with a file and a directory.
fn fixture_root() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp sessions root");
    let tree = root.path().join("a".repeat(64));
    std::fs::create_dir_all(tree.join("nested")).expect("fixture tree");
    std::fs::write(tree.join("a.txt"), b"hello").expect("fixture file");
    std::fs::write(tree.join("nested/b.txt"), b"worldly").expect("fixture file");
    root
}

/// A daemon that measures `root` as its scratch sessions root.
fn daemon_over(root: &std::path::Path) -> CoreDaemon {
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(None, None, None).with_scratch_usage_root(root.to_path_buf()),
    );
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "local".to_owned(),
        None,
        AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
    daemon
}

async fn call(daemon: &CoreDaemon, client: &str, request: CoreRequest) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(
        new_request(format!("req-{}-{}", client, uuid::Uuid::new_v4()), request),
        client,
    ))
    .await
    .unwrap()
}

async fn read_report(daemon: &CoreDaemon, refresh: bool) -> ScratchUsageReportDto {
    match call(daemon, LOCAL_CLIENT, CoreRequest::ScratchUsage { refresh }).await {
        CoreResponse::ScratchUsage { report } => report,
        other => panic!("expected a scratch usage report, got {other:?}"),
    }
}

fn error_code(response: &CoreResponse) -> String {
    match response {
        CoreResponse::Error { code, .. } => code.clone(),
        other => panic!("expected an error response, got {other:?}"),
    }
}

#[tokio::test]
async fn a_refresh_walks_the_tree_and_reports_sizes_counts_and_the_verdict() {
    let root = fixture_root();
    let daemon = daemon_over(root.path());

    let report = read_report(&daemon, true).await;

    assert_eq!(report.tree_count, 1, "one managed tree: {report:?}");
    let tree = &report.trees[0];
    assert_eq!(tree.digest, "a".repeat(64));
    assert_eq!(tree.file_count, 2);
    assert_eq!(tree.dir_count, 2);
    assert_eq!(tree.total_bytes, 5 + 7);
    assert_eq!(report.total_bytes, 12);
    assert_eq!(tree.reclaimable, Some(true));
    assert!(!report.truncated);
    assert!(report.anomalies.is_empty(), "{:?}", report.anomalies);
}

#[tokio::test]
async fn a_read_without_a_refresh_serves_the_daemon_held_report() {
    let root = fixture_root();
    let daemon = daemon_over(root.path());

    // The daemon's own refresh cadence has not necessarily fired yet, so
    // the held report starts empty rather than being measured on demand.
    let held = read_report(&daemon, false).await;
    assert_eq!(held.tree_count, 0);

    let refreshed = read_report(&daemon, true).await;
    assert_eq!(refreshed.tree_count, 1);

    // The refreshed measurement is now the held report.
    let after = read_report(&daemon, false).await;
    assert_eq!(after.tree_count, 1);
    assert_eq!(after.total_bytes, refreshed.total_bytes);
}

#[tokio::test]
async fn a_multiply_linked_tree_is_reported_as_not_reclaimable_with_the_reason() {
    let root = fixture_root();
    let tree = root.path().join("a".repeat(64));
    std::fs::hard_link(tree.join("a.txt"), tree.join("alias.txt")).expect("hard link");
    let daemon = daemon_over(root.path());

    let report = read_report(&daemon, true).await;

    let tree = &report.trees[0];
    assert_eq!(tree.multiply_linked_count, 2);
    assert_eq!(
        tree.reclaimable,
        Some(false),
        "the census shares the reclaim validator's rule: {tree:?}"
    );
    assert!(!tree.refusal_rules.is_empty());
    assert!(
        tree.refusal_paths.iter().any(|path| path.contains("a.txt")),
        "the offending path is reported: {:?}",
        tree.refusal_paths
    );
}

#[tokio::test]
async fn the_report_discloses_no_absolute_path_no_content_and_no_session_identity() {
    let root = fixture_root();
    let tree = root.path().join("a".repeat(64));
    std::fs::write(tree.join("secret.txt"), b"SUPER-SECRET-CONTENT").expect("fixture file");
    let daemon = daemon_over(root.path());

    let report = read_report(&daemon, true).await;
    let rendered = format!("{report:?}");

    assert!(
        !rendered.contains("SUPER-SECRET-CONTENT"),
        "file content never enters the report: {rendered}"
    );
    assert!(
        !rendered.contains(&root.path().to_string_lossy().to_string()),
        "an absolute host path never enters the report: {rendered}"
    );
    assert!(
        !rendered.contains("secret.txt"),
        "a file name only appears when it refused the tree: {rendered}"
    );
}

#[tokio::test]
async fn the_census_leaves_the_tree_byte_identical() {
    let root = fixture_root();
    let before = snapshot(root.path());
    let daemon = daemon_over(root.path());

    let report = read_report(&daemon, true).await;

    assert_eq!(report.tree_count, 1);
    assert_eq!(before, snapshot(root.path()), "the census mutates nothing");
}

#[tokio::test]
async fn a_read_creates_no_job_and_touches_no_queue() {
    let root = fixture_root();
    let daemon = daemon_over(root.path());

    read_report(&daemon, true).await;

    match call(
        &daemon,
        LOCAL_CLIENT,
        CoreRequest::JobList {
            query: codegg::protocol::dto::JobQueryDto::default(),
        },
    )
    .await
    {
        CoreResponse::JobList { jobs, .. } => {
            assert!(jobs.is_empty(), "a census read creates no job");
        }
        other => panic!("expected a job list, got {other:?}"),
    }
}

#[tokio::test]
async fn a_team_principal_fails_closed_on_the_missing_scope() {
    let pool = test_pool().await;
    let root = fixture_root();
    let daemon = CoreDaemon::with_deps(
        CoreRuntimeDeps::new(Some(pool.clone()), None, None)
            .with_scratch_usage_root(root.path().to_path_buf()),
    );
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "local".to_owned(),
        None,
        AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );

    let team = TeamStore::new(pool);
    let workspace = tempfile::tempdir().expect("temp workspace");
    let project = register_project(&daemon, workspace.path()).await;
    let contributor = member_in_project(
        &team,
        &project,
        "Contributor",
        "client-contributor",
        ProjectRole::Contributor,
    )
    .await;
    daemon.clients.register_with_principal(
        "client-contributor".to_owned(),
        "contributor".to_owned(),
        None,
        contributor,
    );

    let response = call(
        &daemon,
        "client-contributor",
        CoreRequest::ScratchUsage { refresh: true },
    )
    .await;

    assert_eq!(
        error_code(&response),
        "authorization_scope_required",
        "the census is scoped to no project, so a team principal fails closed"
    );
}

/// Snapshot a tree's shape and contents for a before/after comparison.
fn snapshot(root: &std::path::Path) -> Vec<(String, u64, Vec<u8>)> {
    let mut entries = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable fixture") {
            let entry = entry.expect("readable fixture entry");
            let metadata = std::fs::symlink_metadata(entry.path()).expect("entry metadata");
            let name = entry.file_name().to_string_lossy().into_owned();
            if metadata.is_dir() {
                entries.push((format!("{name}/"), 0, Vec::new()));
                stack.push(entry.path());
            } else {
                let content = std::fs::read(entry.path()).unwrap_or_default();
                entries.push((name, metadata.len(), content));
            }
        }
    }
    entries.sort();
    entries
}

/// Register a workspace and a project, returning the project id.
async fn register_project(daemon: &CoreDaemon, workspace_root: &std::path::Path) -> ProjectId {
    let workspace_id = match call(
        daemon,
        LOCAL_CLIENT,
        CoreRequest::WorkspaceRegister {
            root: workspace_root.to_string_lossy().into_owned(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("expected workspace snapshot, got {other:?}"),
    };
    match call(
        daemon,
        LOCAL_CLIENT,
        CoreRequest::ProjectRegister {
            request: codegg::protocol::dto::ProjectRegisterRequestDto {
                workspace_id,
                display_name: "scratch-census-boundary".to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => {
            ProjectId::parse(&project.project_id).expect("catalog project id parses")
        }
        other => panic!("expected project registration, got {other:?}"),
    }
}

/// Mint a human principal for `client_id`.
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

/// Create a team principal holding `role` in `project`.
async fn member_in_project(
    team: &TeamStore,
    project: &ProjectId,
    name: &str,
    client_id: &str,
    role: ProjectRole,
) -> AuthenticatedPrincipal {
    let principal = human_token(team, name, client_id).await;
    let record = team
        .get_principal(principal.principal_id())
        .await
        .unwrap()
        .unwrap();
    team.create_membership(project, &record.id, role)
        .await
        .unwrap();
    principal
}
