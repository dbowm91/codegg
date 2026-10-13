//! C005 (ADR-0022, accepted): daemon-owned workspace directory listing.
//!
//! These tests drive the real daemon dispatch (`handle_request_for_client`,
//! so server-side authorization runs exactly as in production) through the
//! exact request the TUI issues. They prove:
//!
//! - local and simulated-remote clients share the one daemon path (the TUI
//!   never walks its own filesystem; the TUI-layer decoy test in
//!   `src/tui/commands/file_tree.rs` proves the client side with a fake
//!   transport, and these tests prove the daemon reads only its resolved
//!   root);
//! - traversal/symlink/wrong-project prefixes are denied before content;
//! - team principals without a grant are masked as not-found, while a
//!   Viewer (which carries `file.read`) may list;
//! - walk bounds surface as explicit truncation, the wire cap trims with a
//!   reason, and single-flight admission answers `busy` instead of piling
//!   up workers;
//! - a listed entry still opens through the unchanged `DocumentOpen` path.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::document::{
    WorkspaceDirectoryListRequestDto, WorkspaceDirectoryListResultDto,
    WorkspaceDirectoryListStatusDto,
};
use codegg::protocol::dto::ProjectRegisterRequestDto;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

const TUI_CLIENT: &str = "codegg-tui";

async fn local_tui_daemon(pool: sqlx::SqlitePool) -> CoreDaemon {
    let daemon = CoreDaemon::new(Some(pool), None, None);
    daemon.clients.register_with_principal(
        TUI_CLIENT.to_owned(),
        TUI_CLIENT.to_owned(),
        None,
        AuthenticatedPrincipal::local_owner(TUI_CLIENT),
    );
    daemon
}

async fn call_as(
    daemon: &CoreDaemon,
    client: &str,
    request_id: &str,
    request: CoreRequest,
) -> CoreResponse {
    Box::pin(daemon.handle_request_for_client(new_request(request_id.to_owned(), request), client))
        .await
        .expect("daemon handles request")
}

async fn call(daemon: &CoreDaemon, request_id: &str, request: CoreRequest) -> CoreResponse {
    call_as(daemon, TUI_CLIENT, request_id, request).await
}

fn error_of(response: &CoreResponse) -> (String, String) {
    match response {
        CoreResponse::Error { code, message } => (code.clone(), message.clone()),
        other => panic!("expected an error response, got {other:?}"),
    }
}

fn listing_of(response: &CoreResponse) -> WorkspaceDirectoryListResultDto {
    match response {
        CoreResponse::WorkspaceDirectoryList { result } => result.clone(),
        other => panic!("expected a directory listing, got {other:?}"),
    }
}

/// Register `root` as a workspace plus one project bound to it, the way
/// the TUI launch bootstrap does. Returns `(project_id, workspace_id)`.
async fn bootstrap_project(
    daemon: &CoreDaemon,
    root: &std::path::Path,
    name: &str,
) -> (String, String) {
    let workspace_id = match call(
        daemon,
        &format!("{name}-workspace"),
        CoreRequest::WorkspaceRegister {
            root: root.to_string_lossy().into_owned(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace registration failed: {other:?}"),
    };
    let project_id = match call(
        daemon,
        &format!("{name}-project"),
        CoreRequest::ProjectRegister {
            request: ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name: name.to_owned(),
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "c005-test".to_owned(),
            },
        },
    )
    .await
    {
        CoreResponse::ProjectRegistered { project } => project.project_id,
        other => panic!("project registration failed: {other:?}"),
    };
    (project_id, workspace_id)
}

fn list_request(project_id: &str, workspace_id: &str, prefix: &str) -> CoreRequest {
    CoreRequest::WorkspaceDirectoryList {
        request: WorkspaceDirectoryListRequestDto {
            project_id: project_id.to_owned(),
            workspace_id: workspace_id.to_owned(),
            prefix: prefix.to_owned(),
        },
    }
}

fn write(root: &std::path::Path, relative: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    std::fs::write(path, b"x").expect("write");
}

fn top_names(result: &WorkspaceDirectoryListResultDto) -> Vec<&str> {
    result.nodes.iter().map(|n| n.name.as_str()).collect()
}

/// A team human with no membership anywhere, bound to its own client
/// through a personal token — the same shape a real team device presents.
async fn outsider_client(daemon: &CoreDaemon, pool: &sqlx::SqlitePool, name: &str) -> String {
    let team = TeamStore::new(pool.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .expect("create principal");
    let client_id = format!("client-{name}");
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .expect("create token");
    let principal = tokens
        .verify_for_client(&plaintext, &client_id)
        .await
        .expect("verify token");
    daemon.clients.register_with_principal(
        client_id.clone(),
        format!("{name}-display"),
        None,
        principal,
    );
    client_id
}

/// A team human granted `role` on `project_id`, bound to its own client.
async fn member_client(
    daemon: &CoreDaemon,
    pool: &sqlx::SqlitePool,
    name: &str,
    project_id: &str,
    role: ProjectRole,
) -> String {
    let team = TeamStore::new(pool.clone());
    let record = team
        .create_principal(PrincipalKind::Human, name)
        .await
        .expect("create principal");
    let project = codegg_core::identity::ProjectId::parse(project_id).expect("project id");
    team.create_membership(&project, &record.id, role)
        .await
        .expect("grant membership");
    let client_id = format!("client-{name}");
    let tokens = PersonalTokenStore::with_team(team.pool().clone(), team.clone());
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .expect("create token");
    let principal = tokens
        .verify_for_client(&plaintext, &client_id)
        .await
        .expect("verify token");
    daemon.clients.register_with_principal(
        client_id.clone(),
        format!("{name}-display"),
        None,
        principal,
    );
    client_id
}

#[tokio::test]
async fn listing_shows_daemon_entries_with_explicit_bounds() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "src/lib.rs");
    write(root.path(), "README.md");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "alpha").await;

    let result = listing_of(
        &call(
            &daemon,
            "list-root",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );

    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert_eq!(result.workspace_id, workspace_id);
    assert_eq!(result.prefix, "");
    assert_eq!(top_names(&result), vec!["src", "README.md"]);
    assert!(!result.truncated);
    assert!(
        result.examined >= 3,
        "examined counts entries, got {}",
        result.examined
    );
}

#[tokio::test]
async fn decoy_tree_with_same_shape_never_leaks_into_the_daemon_listing() {
    // The daemon workspace holds `daemon-only.txt`. A second directory with
    // the same *relative* shape but different content exists on this host
    // (the stand-in for a same-spelled remote-TUI directory). The listing
    // must contain only daemon-host entries: the daemon reads exactly its
    // resolved root, and the TUI layer (proven with a fake transport in
    // `src/tui/commands/file_tree.rs`) never reads its own tree at all.
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let daemon_root = tempfile::tempdir().expect("tempdir");
    write(daemon_root.path(), "daemon-only.txt");
    write(daemon_root.path(), "shared/name.txt");
    let decoy = tempfile::tempdir().expect("tempdir");
    write(decoy.path(), "local-only.txt");
    write(decoy.path(), "shared/decoy.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, daemon_root.path(), "decoy").await;

    let result = listing_of(
        &call(
            &daemon,
            "list-decoy",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );

    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    let names = top_names(&result);
    assert!(names.contains(&"daemon-only.txt"));
    assert!(
        !names.contains(&"local-only.txt"),
        "decoy must never leak: {names:?}"
    );
    let shared = result
        .nodes
        .iter()
        .find(|n| n.name == "shared")
        .expect("shared dir listed");
    let children: Vec<&str> = shared
        .children
        .as_deref()
        .unwrap_or(&[])
        .iter()
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(children, vec!["name.txt"]);
}

#[tokio::test]
async fn subtree_prefix_lists_relative_paths() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "docs/a.txt");
    write(root.path(), "top.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "prefix").await;

    let result = listing_of(
        &call(
            &daemon,
            "list-prefix",
            list_request(&project_id, &workspace_id, "docs"),
        )
        .await,
    );

    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert_eq!(result.prefix, "docs");
    assert_eq!(top_names(&result), vec!["a.txt"]);
    assert_eq!(result.nodes[0].path, "docs/a.txt");
}

#[tokio::test]
async fn traversal_prefixes_are_denied_before_io() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "ok.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "traversal").await;

    for (id, prefix) in [
        ("t-dotdot", "../escape"),
        ("t-nested", "a/../../escape"),
        ("t-absolute", "/etc"),
        ("t-empty-seg", "a//b"),
        ("t-dot", "a/./b"),
    ] {
        let (code, _) = error_of(
            &call(
                &daemon,
                id,
                list_request(&project_id, &workspace_id, prefix),
            )
            .await,
        );
        assert_eq!(
            code, "document_invalid_path",
            "prefix {prefix:?} must be denied"
        );
    }
}

#[tokio::test]
async fn symlink_prefix_and_symlink_entries_never_escape() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "real/inner.txt");
    write(root.path(), "top.txt");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path().join("real"), root.path().join("linked"))
            .expect("symlink");
        std::os::unix::fs::symlink("/etc", root.path().join("linked-etc")).expect("symlink");
    }
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "symlink").await;

    // A symlinked prefix is refused on the blocking thread and surfaces as
    // an unavailable listing — never followed, never an empty success.
    #[cfg(unix)]
    for (id, prefix) in [("s-link", "linked"), ("s-etc", "linked-etc")] {
        let result = listing_of(
            &call(
                &daemon,
                id,
                list_request(&project_id, &workspace_id, prefix),
            )
            .await,
        );
        assert_eq!(
            result.status,
            WorkspaceDirectoryListStatusDto::Unavailable,
            "symlink prefix {prefix:?} must not resolve"
        );
        assert!(result.nodes.is_empty());
    }

    // Symlink entries inside the tree are skipped, never listed.
    let result = listing_of(
        &call(
            &daemon,
            "s-root",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    let names = top_names(&result);
    assert!(names.contains(&"top.txt"));
    assert!(names.contains(&"real"));
    #[cfg(unix)]
    {
        assert!(
            !names.contains(&"linked"),
            "symlinks are never listed: {names:?}"
        );
        assert!(
            !names.contains(&"linked-etc"),
            "symlinks are never listed: {names:?}"
        );
    }
}

#[tokio::test]
async fn wrong_project_or_unknown_workspace_fails_closed() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root_a = tempfile::tempdir().expect("tempdir");
    write(root_a.path(), "a.txt");
    let root_b = tempfile::tempdir().expect("tempdir");
    write(root_b.path(), "b.txt");
    let (project_a, workspace_a) = bootstrap_project(&daemon, root_a.path(), "proj-a").await;
    let (project_b, workspace_b) = bootstrap_project(&daemon, root_b.path(), "proj-b").await;

    // Cross-wiring project and workspace is refused: the binding is
    // verified server-side, never trusted from the request alone.
    let (code, _) = error_of(
        &call(
            &daemon,
            "x-cross",
            list_request(&project_b, &workspace_a, ""),
        )
        .await,
    );
    assert_eq!(code, "document_not_found");
    let (code, _) = error_of(
        &call(
            &daemon,
            "x-cross-rev",
            list_request(&project_a, &workspace_b, ""),
        )
        .await,
    );
    assert_eq!(code, "document_not_found");
    // Unknown workspace is indistinguishable from a wrong binding.
    let (code, _) = error_of(
        &call(
            &daemon,
            "x-unknown",
            list_request(&project_a, "ws_does_not_exist", ""),
        )
        .await,
    );
    assert_eq!(code, "document_not_found");
}

#[tokio::test]
async fn outsider_is_masked_as_not_found_while_viewer_may_list() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool.clone()).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "secret.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "team").await;

    // A principal with no grant on the project learns nothing: the denial
    // is masked as not-found, exactly like `DocumentOpen`.
    let outsider = outsider_client(&daemon, &pool, "mallory").await;
    let (code, _) = error_of(
        &call_as(
            &daemon,
            &outsider,
            "team-outsider",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(code, "project_not_found");

    // A Viewer carries `file.read` and may list — `project.observe` alone
    // is never the gate, but the Viewer role bundles the read capability.
    let viewer_client =
        member_client(&daemon, &pool, "victor", &project_id, ProjectRole::Viewer).await;
    let result = listing_of(
        &call_as(
            &daemon,
            &viewer_client,
            "team-viewer",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert_eq!(top_names(&result), vec!["secret.txt"]);
}

#[tokio::test]
async fn deep_tree_truncates_with_an_explicit_depth_reason() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    let deep = (0..15)
        .map(|i| format!("d{i}"))
        .collect::<Vec<_>>()
        .join("/");
    write(root.path(), &format!("{deep}/bottom.txt"));
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "deep").await;

    let result = listing_of(
        &call(
            &daemon,
            "list-deep",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );

    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert!(result.truncated, "depth 15 exceeds the 12 cap");
    assert!(
        result
            .truncated_reason
            .as_deref()
            .is_some_and(|r| r.contains("depth")),
        "the reason must name the depth bound, got {:?}",
        result.truncated_reason
    );
}

#[tokio::test]
async fn node_budget_truncates_a_wide_tree() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    for i in 0..4200 {
        write(root.path(), &format!("f{i:05}.txt"));
    }
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "wide").await;

    let result = listing_of(
        &call(
            &daemon,
            "list-wide",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );

    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert!(result.truncated, "4200 files exceed the 4096 node cap");
    assert!(
        result
            .truncated_reason
            .as_deref()
            .is_some_and(|r| r.contains("4096")),
        "the reason must name the node bound, got {:?}",
        result.truncated_reason
    );
    assert_eq!(result.nodes.len(), 4096);
}

#[tokio::test]
async fn held_admission_answers_busy_and_releases_to_ready() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "a.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "admit").await;

    // Hold the single permit externally: every listing must answer `busy`
    // with no scan, proving no path bypasses admission.
    let held = daemon
        .directory_listing_permits
        .clone()
        .try_acquire_owned()
        .expect("permit starts free");
    for i in 0..10 {
        let response = call(
            &daemon,
            &format!("busy-{i}"),
            list_request(&project_id, &workspace_id, ""),
        )
        .await;
        let result = listing_of(&response);
        assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Busy);
        assert!(result.nodes.is_empty());
        assert_eq!(result.examined, 0, "a busy answer scans nothing");
    }
    drop(held);

    // Released: the same request serves a real listing.
    let result = listing_of(
        &call(
            &daemon,
            "after-release",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert_eq!(top_names(&result), vec!["a.txt"]);
}

#[tokio::test]
async fn concurrent_bursts_all_settle_without_piling_workers() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    for i in 0..200 {
        write(root.path(), &format!("f{i:03}.txt"));
    }
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "burst").await;

    // 100 concurrent requests: each answers Ready or Busy, none errors and
    // none hangs. At most one blocking scan runs at a time by construction
    // (semaphore of 1); the burst proves the bound holds under contention.
    let request_ids: Vec<String> = (0..100).map(|i| format!("burst-{i}")).collect();
    let mut handles = Vec::new();
    for id in &request_ids {
        handles.push(call(
            &daemon,
            id,
            list_request(&project_id, &workspace_id, ""),
        ));
    }
    let mut ready = 0usize;
    let mut busy = 0usize;
    for response in futures_util::future::join_all(handles).await {
        match listing_of(&response).status {
            WorkspaceDirectoryListStatusDto::Ready => ready += 1,
            WorkspaceDirectoryListStatusDto::Busy => busy += 1,
            WorkspaceDirectoryListStatusDto::Unavailable => {
                panic!("a healthy burst must never answer unavailable")
            }
        }
    }
    assert_eq!(ready + busy, 100);
    assert!(ready >= 1, "at least one scan must win admission");
    assert_eq!(
        daemon.directory_listing_permits.available_permits(),
        1,
        "the permit must be released after the burst"
    );
}

#[tokio::test]
async fn listed_entry_still_opens_through_document_open() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(root.path().join("open-me.txt"), b"hello").expect("write");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "reopen").await;

    let listed = listing_of(
        &call(
            &daemon,
            "list-reopen",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(top_names(&listed), vec!["open-me.txt"]);

    // The listing grants nothing by itself: opening reauthorizes through
    // the unchanged document path.
    match call(
        &daemon,
        "open-listed",
        CoreRequest::DocumentOpen {
            project_id: project_id.clone(),
            workspace_id: workspace_id.clone(),
            relative_path: "open-me.txt".to_owned(),
            scope: codegg::protocol::document::DocumentScope::Workspace,
        },
    )
    .await
    {
        CoreResponse::DocumentSnapshot { snapshot, .. } => {
            assert_eq!(snapshot.relative_path, "open-me.txt");
            assert_eq!(snapshot.text, "hello");
        }
        other => panic!("DocumentOpen must still serve a listed file, got {other:?}"),
    }
}
