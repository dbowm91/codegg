//! C006 (post-C005 corrective): daemon listing worker-permit lifetime and
//! exact 512 KiB serialized-wire bound.
//!
//! Proves the two concrete C005 gaps are closed without changing the
//! daemon-owned, read-only architecture:
//!
//! - F1: the admission permit is owned by the actual blocking worker.
//!   Aborting the async waiter cannot free a scanner that is still alive;
//!   a second request while the worker lives answers `Busy` with no second
//!   scanner started (peak live workers stays 1).
//! - F2: every successful `CoreResponse::WorkspaceDirectoryList` fits
//!   `MAX_DIRECTORY_LISTING_RESPONSE_BYTES` as a **complete serialized
//!   response** (discriminator + ids + status + examined + truncation +
//!   nodes), trimming trailing whole top-level nodes to zero if needed.
//!   A sole oversized nested root is dropped, never emitted.
//!
//! Also re-exercises daemon dispatch + authorization (no weakened gates)
//! and records the ADR-0022 provenance note by reference (see closure).

use codegg::core::daemon::CoreDaemon;
use codegg::core::daemon_documents::{
    build_bounded_listing_response_counted, listing_response_fits_budget,
    serialized_listing_response_len,
};
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::document::{
    WorkspaceDirectoryListRequestDto, WorkspaceDirectoryListResultDto,
    WorkspaceDirectoryListStatusDto, WorkspaceDirectoryNodeDto, WorkspaceDirectoryNodeKindDto,
    MAX_DIRECTORY_LISTING_RESPONSE_BYTES,
};
use codegg::protocol::dto::ProjectRegisterRequestDto;
use codegg_core::team::{PrincipalKind, ProjectRole, TeamStore};
use codegg_core::transport_auth::{AuthenticatedPrincipal, PersonalTokenStore};
use std::sync::Arc;
use std::time::Duration;

const TUI_CLIENT: &str = "codegg-tui";
const WAIT: Duration = Duration::from_secs(5);

async fn test_pool() -> sqlx::SqlitePool {
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;
    let name = format!("c006_{}", uuid::Uuid::new_v4().simple());
    let url = format!("file:{name}?mode=memory&cache=shared");
    let opts = SqliteConnectOptions::from_str(&url)
        .expect("valid sqlite options")
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(5))
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .expect("connect test pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

async fn local_tui_daemon(pool: sqlx::SqlitePool) -> Arc<CoreDaemon> {
    let daemon = CoreDaemon::new(Some(pool), None, None);
    daemon.clients.register_with_principal(
        TUI_CLIENT.to_owned(),
        TUI_CLIENT.to_owned(),
        None,
        AuthenticatedPrincipal::local_owner(TUI_CLIENT),
    );
    Arc::new(daemon)
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
                source: "c006-test".to_owned(),
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

// ---- DTO builders for wire-budget tests (no filesystem) ----

fn file_node(name: &str, path: &str, depth: usize) -> WorkspaceDirectoryNodeDto {
    WorkspaceDirectoryNodeDto {
        name: name.to_owned(),
        path: path.to_owned(),
        depth,
        kind: WorkspaceDirectoryNodeKindDto::File,
        children: None,
    }
}

fn dir_node(
    name: &str,
    path: &str,
    depth: usize,
    children: Vec<WorkspaceDirectoryNodeDto>,
) -> WorkspaceDirectoryNodeDto {
    WorkspaceDirectoryNodeDto {
        name: name.to_owned(),
        path: path.to_owned(),
        depth,
        kind: WorkspaceDirectoryNodeKindDto::Directory,
        children: Some(children),
    }
}

fn listing_with_nodes(
    nodes: Vec<WorkspaceDirectoryNodeDto>,
    truncated: bool,
    reason: Option<String>,
) -> codegg::directory_listing::DirectoryListing {
    codegg::directory_listing::DirectoryListing {
        nodes,
        truncated,
        truncated_reason: reason,
        examined: 10_000,
    }
}

fn full_response_len(
    workspace_id: &str,
    prefix: &str,
    nodes: &[WorkspaceDirectoryNodeDto],
    truncated: bool,
    reason: Option<String>,
) -> usize {
    let response = CoreResponse::WorkspaceDirectoryList {
        result: WorkspaceDirectoryListResultDto {
            workspace_id: workspace_id.to_owned(),
            prefix: prefix.to_owned(),
            nodes: nodes.to_vec(),
            truncated,
            truncated_reason: reason,
            examined: 10_000,
            status: WorkspaceDirectoryListStatusDto::Ready,
        },
    };
    serialized_listing_response_len(&response)
}

// One nested top-level root whose serialized subtree alone exceeds the
// budget, within legal depth (<=12) and node count (<=4096).
fn oversized_nested_root() -> WorkspaceDirectoryNodeDto {
    // 100 subdirs x 20 files with 120-byte names: ~2000 leaves plus
    // intermediate dirs, deeply nested paths inflate JSON past 512 KiB.
    let mut subdirs = Vec::new();
    for s in 0..100 {
        let mut files = Vec::new();
        for f in 0..20 {
            let name = format!("file-{s:03}-{f:03}-{}.txt", "n".repeat(100));
            files.push(file_node(&name, &format!("big/sub-{s:03}/{name}"), 3));
        }
        subdirs.push(dir_node(
            &format!("sub-{s:03}"),
            &format!("big/sub-{s:03}"),
            2,
            files,
        ));
    }
    dir_node("big", "big", 1, subdirs)
}

// ---- F1: worker-owned admission ----

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn aborted_handler_does_not_release_permit_before_blocking_worker_exit() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "a.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "abort").await;

    daemon.directory_listing_probe.reset_peak();
    assert_eq!(daemon.directory_listing_probe.active_workers(), 0);
    assert_eq!(daemon.directory_listing_permits.available_permits(), 1);

    // Block the worker after entry.
    let gate = daemon.directory_listing_probe.install_blocking_gate();
    let first_req = list_request(&project_id, &workspace_id, "");
    let daemon_clone = daemon.clone();
    let first = tokio::spawn(async move { call(&daemon_clone, "first-abort", first_req).await });

    assert!(
        daemon
            .directory_listing_probe
            .wait_for_active(1, WAIT)
            .await,
        "worker must enter and block"
    );
    assert_eq!(daemon.directory_listing_probe.peak_workers(), 1);

    // Abort the async waiter while the blocking worker is still alive.
    first.abort();
    let aborted = first.await;
    assert!(
        aborted.is_err(),
        "aborted handler must not produce a response"
    );

    // The actual worker still owns admission: still active, no permit.
    assert_eq!(
        daemon.directory_listing_probe.active_workers(),
        1,
        "aborting the waiter must not free the live worker"
    );
    assert_eq!(daemon.directory_listing_probe.peak_workers(), 1);
    assert_eq!(daemon.directory_listing_permits.available_permits(), 0);

    // A second authorized listing while the first worker lives answers
    // Busy and starts no second scanner.
    let second = listing_of(
        &call(
            &daemon,
            "second-while-blocked",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(second.status, WorkspaceDirectoryListStatusDto::Busy);
    assert!(second.nodes.is_empty());
    assert_eq!(second.examined, 0);
    assert_eq!(
        daemon.directory_listing_probe.active_workers(),
        1,
        "second request must not start a second scanner"
    );
    assert_eq!(daemon.directory_listing_probe.peak_workers(), 1);

    // Release the first worker and await its actual exit.
    gate.release();
    assert!(
        daemon
            .directory_listing_probe
            .wait_for_active(0, WAIT)
            .await,
        "worker must exit after release"
    );
    daemon.directory_listing_probe.clear_gate();
    assert_eq!(daemon.directory_listing_permits.available_permits(), 1);

    // A third request after exit may serve a real listing.
    let third = listing_of(
        &call(
            &daemon,
            "third-after-exit",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(third.status, WorkspaceDirectoryListStatusDto::Ready);
    assert_eq!(top_names(&third), vec!["a.txt"]);
    assert_eq!(daemon.directory_listing_probe.peak_workers(), 1);
    assert!(
        listing_response_fits_budget(&CoreResponse::WorkspaceDirectoryList { result: third }),
        "third response must fit the wire budget"
    );
    assert_eq!(daemon.directory_listing_probe.active_workers(), 0);
}

#[tokio::test]
async fn panic_and_failure_release_permit_when_worker_exits() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "ok.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "panic").await;

    // Panic after entry surfaces as sanitized Unavailable and frees admission.
    daemon.directory_listing_probe.install_panicking_gate();
    let panicked = listing_of(
        &call(
            &daemon,
            "panicking-scan",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(
        panicked.status,
        WorkspaceDirectoryListStatusDto::Unavailable,
        "worker panic must fail closed, never Ready with fabricated entries"
    );
    assert!(panicked.nodes.is_empty());
    daemon.directory_listing_probe.clear_gate();
    assert_eq!(
        daemon.directory_listing_probe.active_workers(),
        0,
        "panicked worker must exit"
    );
    assert_eq!(daemon.directory_listing_permits.available_permits(), 1);

    // Injected failure behaves the same.
    daemon.directory_listing_probe.reset_peak();
    daemon.directory_listing_probe.install_failing_gate();
    let failed = listing_of(
        &call(
            &daemon,
            "failing-scan",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(failed.status, WorkspaceDirectoryListStatusDto::Unavailable);
    assert!(failed.nodes.is_empty());
    daemon.directory_listing_probe.clear_gate();
    assert_eq!(daemon.directory_listing_probe.active_workers(), 0);
    assert_eq!(daemon.directory_listing_permits.available_permits(), 1);

    // The daemon still serves after both abnormal exits.
    let after = listing_of(
        &call(
            &daemon,
            "after-abnormal",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(after.status, WorkspaceDirectoryListStatusDto::Ready);
    assert_eq!(top_names(&after), vec!["ok.txt"]);
}

#[tokio::test]
async fn abort_before_worker_starts_does_not_leak_permit() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "a.txt");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "nostart").await;

    // Hold admission externally so the spawned request takes the Busy path
    // (no worker ever starts), then abort it mid-flight.
    let held = daemon
        .directory_listing_permits
        .clone()
        .try_acquire_owned()
        .expect("permit starts free");
    daemon.directory_listing_probe.reset_peak();
    let daemon_clone = daemon.clone();
    let req = list_request(&project_id, &workspace_id, "");
    let handle = tokio::spawn(async move { call(&daemon_clone, "busy-abort", req).await });
    // Abort before awaiting: the Busy path spawns no worker, so aborting
    // cannot strand admission.
    handle.abort();
    let _ = handle.await;
    assert_eq!(
        daemon.directory_listing_probe.active_workers(),
        0,
        "Busy path must never start a worker"
    );
    assert_eq!(daemon.directory_listing_probe.peak_workers(), 0);
    assert_eq!(daemon.directory_listing_permits.available_permits(), 0);
    drop(held);
    assert_eq!(daemon.directory_listing_permits.available_permits(), 1);

    let after = listing_of(
        &call(
            &daemon,
            "after-abort-nostart",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(after.status, WorkspaceDirectoryListStatusDto::Ready);
    assert_eq!(top_names(&after), vec!["a.txt"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hundred_concurrent_clients_never_enter_second_blocking_scan() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    for i in 0..50 {
        write(root.path(), &format!("f{i:03}.txt"));
    }
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "hundred").await;

    daemon.directory_listing_probe.reset_peak();
    let gate = daemon.directory_listing_probe.install_blocking_gate();
    let daemon_clone = daemon.clone();
    let first_req = list_request(&project_id, &workspace_id, "");
    let first = tokio::spawn(async move { call(&daemon_clone, "hundred-first", first_req).await });
    assert!(
        daemon
            .directory_listing_probe
            .wait_for_active(1, WAIT)
            .await,
        "first worker must block"
    );

    // 100 concurrent contenders while the worker is held: all Busy, none
    // starts a second scan.
    let mut handles = Vec::new();
    for i in 0..100 {
        let daemon_clone = daemon.clone();
        let req = list_request(&project_id, &workspace_id, "");
        handles.push(tokio::spawn(async move {
            call(&daemon_clone, &format!("hundred-{i}"), req).await
        }));
    }
    let mut busy = 0usize;
    for handle in handles {
        let response = handle.await.expect("contender completes");
        let result = listing_of(&response);
        assert_eq!(
            result.status,
            WorkspaceDirectoryListStatusDto::Busy,
            "contended scan must answer Busy, never queue"
        );
        assert!(result.nodes.is_empty());
        busy += 1;
    }
    assert_eq!(busy, 100);
    assert_eq!(
        daemon.directory_listing_probe.active_workers(),
        1,
        "only the first worker may be alive"
    );
    assert_eq!(
        daemon.directory_listing_probe.peak_workers(),
        1,
        "peak live scanners must stay 1 across 100 contenders"
    );

    first.abort();
    let _ = first.await;
    gate.release();
    assert!(
        daemon
            .directory_listing_probe
            .wait_for_active(0, WAIT)
            .await,
        "blocked worker must exit"
    );
    daemon.directory_listing_probe.clear_gate();
    assert_eq!(daemon.directory_listing_permits.available_permits(), 1);
}

// ---- F2: full-response budget ----

#[test]
fn single_nested_root_over_512k_is_dropped_with_truncation() {
    let root = oversized_nested_root();
    // Negative proof: the pre-fix policy (keep one oversized root, measure
    // nodes only) would emit this subtree. Its full response exceeds the cap.
    let untrimmed_len = full_response_len("ws-1", "", std::slice::from_ref(&root), false, None);
    assert!(
        untrimmed_len > MAX_DIRECTORY_LISTING_RESPONSE_BYTES,
        "fixture must exceed the cap alone, got {untrimmed_len} bytes"
    );

    // Post-fix: the sole root is dropped, never emitted oversize.
    let listing = listing_with_nodes(vec![root], false, None);
    let (response, _) =
        build_bounded_listing_response_counted("ws-1".to_owned(), String::new(), listing);
    let result = listing_of(&response);
    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert!(
        result.nodes.is_empty(),
        "oversized sole root must be dropped, not retained"
    );
    assert!(
        result.truncated,
        "dropping everything must still report truncated, never silent empty"
    );
    assert!(
        result
            .truncated_reason
            .as_deref()
            .is_some_and(|r| r.contains("response budget")),
        "reason must name the budget, got {:?}",
        result.truncated_reason
    );
    assert!(
        listing_response_fits_budget(&response),
        "post-fix response must fit: {} bytes",
        serialized_listing_response_len(&response)
    );
}

#[test]
fn full_core_response_serialized_under_512k_with_metadata() {
    // Near-threshold flat roots plus maximal metadata: long ids, deep
    // prefix, large examined, and a walker truncation notice. The wire
    // notice must be counted on top, and the final CoreResponse — not a
    // nodes-only estimate — must fit.
    let workspace_id = format!("ws-{}", "w".repeat(64));
    let prefix = (0..12)
        .map(|i| format!("dir{i}"))
        .collect::<Vec<_>>()
        .join("/");
    let mut nodes = Vec::new();
    for i in 0..500 {
        let name = format!("file-{i:04}-{}.txt", "x".repeat(80));
        nodes.push(file_node(&name, &format!("{prefix}/{name}"), 13));
    }
    let walker_reason = Some("stopped at the maximum of 4096 entries".to_owned());
    let listing = listing_with_nodes(nodes, true, walker_reason);
    let (response, _) =
        build_bounded_listing_response_counted(workspace_id.clone(), prefix.clone(), listing);
    let result = listing_of(&response);
    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    let len = serialized_listing_response_len(&response);
    assert!(
        len <= MAX_DIRECTORY_LISTING_RESPONSE_BYTES,
        "full response with metadata must fit, got {len} bytes"
    );
    // Nodes-only size would undercount: the discriminator, ids, prefix,
    // examined, status, and combined reason are all inside `len`.
    let nodes_len = serde_json::to_string(&result.nodes)
        .expect("nodes serialize")
        .len();
    assert!(
        len > nodes_len,
        "full response ({len}) must exceed nodes-only ({nodes_len})"
    );
    if result.nodes.len() < 500 {
        assert!(result.truncated);
        assert!(
            result
                .truncated_reason
                .as_deref()
                .is_some_and(|r| r.contains("4096") && r.contains("response budget")),
            "combined walker + wire reasons must both be counted, got {:?}",
            result.truncated_reason
        );
    }
}

#[test]
fn near_limit_unicode_and_escape_payload() {
    // JSON escape amplification: `"` and `\` expand in `serde_json`
    // output, so a char count would undercount bytes. Names stay within
    // the 255-byte walker cap.
    let mut nodes = Vec::new();
    for i in 0..400 {
        // 100 quotes + 50 backslashes + padding: heavy escape expansion.
        let stem = format!(
            "q{}-{}-{}",
            "\"".repeat(60),
            "\\".repeat(40),
            "é".repeat(20)
        );
        let name = format!("{stem}-{i:04}.txt");
        assert!(
            name.len() <= 255,
            "fixture name must respect the walker cap"
        );
        nodes.push(file_node(&name, &format!("esc/{name}"), 2));
    }
    let listing = listing_with_nodes(nodes, false, None);
    let (response, _) =
        build_bounded_listing_response_counted("ws-esc".to_owned(), "esc".to_owned(), listing);
    let result = listing_of(&response);
    let len = serialized_listing_response_len(&response);
    assert!(
        len <= MAX_DIRECTORY_LISTING_RESPONSE_BYTES,
        "escape-heavy response must fit, got {len}"
    );
    // The escaped JSON is strictly larger than the raw char count, proving
    // the enforcer measures serialized bytes, not characters.
    let raw_chars: usize = result.nodes.iter().map(|n| n.name.len()).sum();
    let nodes_json = serde_json::to_string(&result.nodes)
        .expect("serialize")
        .len();
    assert!(
        nodes_json > raw_chars,
        "escaped JSON ({nodes_json}) must exceed raw name bytes ({raw_chars})"
    );
}

#[test]
fn combined_truncation_reasons_are_counted() {
    let mut nodes = Vec::new();
    for i in 0..600 {
        let name = format!("combined-{i:04}-{}.dat", "y".repeat(90));
        nodes.push(file_node(&name, &name, 1));
    }
    let walker_reason = Some("stopped after examining the maximum of 100000 entries".to_owned());
    let listing = listing_with_nodes(nodes, true, walker_reason.clone());
    let (response, _) =
        build_bounded_listing_response_counted("ws-comb".to_owned(), String::new(), listing);
    let result = listing_of(&response);
    let len = serialized_listing_response_len(&response);
    assert!(
        len <= MAX_DIRECTORY_LISTING_RESPONSE_BYTES,
        "combined-reason response must fit, got {len}"
    );
    if result.nodes.len() < 600 {
        let reason = result
            .truncated_reason
            .as_deref()
            .expect("reason when trimmed");
        assert!(
            reason.contains("100000"),
            "walker notice must survive, got {reason:?}"
        );
        assert!(
            reason.contains("response budget"),
            "wire notice must be appended, got {reason:?}"
        );
        // Re-serializing the exact emitted response (reason included)
        // fits; a checker that ignored the reason would undercount.
        assert!(listing_response_fits_budget(&response));
    } else {
        // Nothing trimmed: walker reason preserved verbatim.
        assert!(result.truncated);
        assert_eq!(result.truncated_reason, walker_reason);
    }
}

#[test]
fn all_roots_removed_is_visible_truncated_not_empty_success() {
    // A sole giant root forces removal of everything. The result is
    // Ready + empty + truncated with a reason — visibly distinct from a
    // genuinely empty directory (Ready + empty + not truncated).
    let root = oversized_nested_root();
    let listing = listing_with_nodes(vec![root], false, None);
    let (response, _) =
        build_bounded_listing_response_counted("ws-empty".to_owned(), String::new(), listing);
    let emptied = listing_of(&response);
    assert_eq!(emptied.status, WorkspaceDirectoryListStatusDto::Ready);
    assert!(emptied.nodes.is_empty());
    assert!(emptied.truncated);
    assert!(emptied.truncated_reason.is_some());

    let genuinely_empty = listing_with_nodes(Vec::new(), false, None);
    let (empty_response, _) = build_bounded_listing_response_counted(
        "ws-empty".to_owned(),
        String::new(),
        genuinely_empty,
    );
    let empty = listing_of(&empty_response);
    assert_eq!(empty.status, WorkspaceDirectoryListStatusDto::Ready);
    assert!(empty.nodes.is_empty());
    assert!(
        !empty.truncated,
        "genuinely empty must not claim truncation"
    );
    assert!(empty.truncated_reason.is_none());
    assert_ne!(
        emptied.truncated, empty.truncated,
        "wire-cap empty (truncated) must differ from empty success"
    );
}

#[test]
fn small_listing_keeps_original_order_content() {
    let nodes = vec![
        dir_node(
            "adir",
            "adir",
            1,
            vec![file_node("inner.txt", "adir/inner.txt", 2)],
        ),
        dir_node(
            "zdir",
            "zdir",
            1,
            vec![file_node("inner.txt", "zdir/inner.txt", 2)],
        ),
        file_node("alpha.txt", "alpha.txt", 1),
        file_node("zeta.txt", "zeta.txt", 1),
    ];
    let listing = listing_with_nodes(nodes.clone(), false, None);
    let (response, _) =
        build_bounded_listing_response_counted("ws-small".to_owned(), String::new(), listing);
    let result = listing_of(&response);
    assert_eq!(result.status, WorkspaceDirectoryListStatusDto::Ready);
    assert!(!result.truncated);
    assert!(result.truncated_reason.is_none());
    assert_eq!(result.nodes, nodes, "small listings keep identity/order");
    assert!(listing_response_fits_budget(&response));
}

#[test]
fn response_budget_enforcer_does_not_reserialize_quadratically() {
    // 4096 small nodes sized to force trimming: a naive pop-one-and-
    // reserialize loop would serialize ~4096 times (O(n^2) bytes). The
    // binary-search enforcer must use O(log n) full serializations.
    let mut nodes = Vec::new();
    for i in 0..4096 {
        let name = format!("quad-{i:04}-{}.txt", "z".repeat(120));
        nodes.push(file_node(&name, &name, 1));
    }
    let listing = listing_with_nodes(nodes, false, None);
    let (response, serializations) =
        build_bounded_listing_response_counted("ws-quad".to_owned(), String::new(), listing);
    assert!(
        listing_response_fits_budget(&response),
        "trimmed 4096-node response must fit"
    );
    assert!(
        serializations <= 32,
        "binary search must bound work to O(log n), got {serializations} serializations"
    );
}

#[test]
fn busy_and_unavailable_responses_respect_the_budget() {
    for status in [
        WorkspaceDirectoryListStatusDto::Busy,
        WorkspaceDirectoryListStatusDto::Unavailable,
    ] {
        let response = CoreResponse::WorkspaceDirectoryList {
            result: WorkspaceDirectoryListResultDto {
                workspace_id: "ws-budget".to_owned(),
                prefix: "some/prefix".to_owned(),
                nodes: Vec::new(),
                truncated: false,
                truncated_reason: None,
                examined: 0,
                status,
            },
        };
        assert!(
            listing_response_fits_budget(&response),
            "{status:?} must fit the budget"
        );
    }
}

// ---- Integration / regression: authorization + containment unchanged ----

#[tokio::test]
async fn authorization_and_containment_unchanged() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool.clone()).await;
    let root = tempfile::tempdir().expect("tempdir");
    write(root.path(), "secret.txt");
    write(root.path(), "real/inner.txt");
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path().join("real"), root.path().join("linked"))
        .expect("symlink");
    let (project_id, workspace_id) = bootstrap_project(&daemon, root.path(), "auth").await;

    // Valid scope serves Ready within budget.
    let ok = listing_of(
        &call(
            &daemon,
            "auth-ok",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(ok.status, WorkspaceDirectoryListStatusDto::Ready);
    assert!(listing_response_fits_budget(
        &CoreResponse::WorkspaceDirectoryList { result: ok.clone() }
    ));

    // Cross-wired project/workspace fails closed (no listing).
    let (code, _) = error_of(
        &call(
            &daemon,
            "auth-cross",
            list_request(&project_id, "ws_does_not_exist", ""),
        )
        .await,
    );
    assert_eq!(code, "document_not_found");

    // Outsider without a grant learns nothing (masked, like DocumentOpen).
    let outsider = outsider_client(&daemon, &pool, "mallory-c006").await;
    let (code, _) = error_of(
        &call_as(
            &daemon,
            &outsider,
            "auth-outsider",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(code, "project_not_found");

    // Viewer (file.read) may list.
    let viewer = member_client(
        &daemon,
        &pool,
        "victor-c006",
        &project_id,
        ProjectRole::Viewer,
    )
    .await;
    let viewed = listing_of(
        &call_as(
            &daemon,
            &viewer,
            "auth-viewer",
            list_request(&project_id, &workspace_id, ""),
        )
        .await,
    );
    assert_eq!(viewed.status, WorkspaceDirectoryListStatusDto::Ready);

    // Traversal prefixes denied before I/O.
    let (code, _) = error_of(
        &call(
            &daemon,
            "auth-traversal",
            list_request(&project_id, &workspace_id, "../escape"),
        )
        .await,
    );
    assert_eq!(code, "document_invalid_path");

    // Symlinked prefix never resolves (Unavailable, never Ready).
    #[cfg(unix)]
    {
        let linked = listing_of(
            &call(
                &daemon,
                "auth-symlink",
                list_request(&project_id, &workspace_id, "linked"),
            )
            .await,
        );
        assert_eq!(linked.status, WorkspaceDirectoryListStatusDto::Unavailable);
        assert!(linked.nodes.is_empty());
    }

    // A listed entry still opens only through the unchanged DocumentOpen.
    match call(
        &daemon,
        "auth-open",
        CoreRequest::DocumentOpen {
            project_id: project_id.clone(),
            workspace_id: workspace_id.clone(),
            relative_path: "secret.txt".to_owned(),
            scope: codegg::protocol::document::DocumentScope::Workspace,
        },
    )
    .await
    {
        CoreResponse::DocumentSnapshot { snapshot, .. } => {
            assert_eq!(snapshot.relative_path, "secret.txt");
        }
        other => panic!("DocumentOpen must still serve a listed file, got {other:?}"),
    }
}
