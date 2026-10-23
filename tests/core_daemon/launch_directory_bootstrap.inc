//! Launch-directory project bootstrap regression coverage.
//!
//! Launching CodeGG in a directory it has never seen must be able to create a
//! session. The daemon resolves a directory locator through the project
//! catalog by lookup only and never registers on a miss, so an unregistered
//! directory failed every session-scoped request with
//! `project_context_required` — session create, the composer path, and the
//! session list alike. `App::bootstrap_launch_project_context` closes that by
//! registering the launch directory through the same idempotent operations the
//! project picker already used.
//!
//! These tests drive the daemon directly with the exact request sequence the
//! TUI bootstrap issues, so they stay valid whether the frontend or the daemon
//! owns the sequencing.

use codegg::core::daemon::CoreDaemon;
use codegg::core::new_request;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg::protocol::dto::ProjectRegisterRequestDto;
use codegg_core::transport_auth::AuthenticatedPrincipal;

async fn test_pool() -> sqlx::SqlitePool {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory pool");
    codegg_core::session::schema::migrate(&pool)
        .await
        .expect("migrate test pool");
    pool
}

/// Client id the TUI presents when it connects to the singleton daemon.
const TUI_CLIENT: &str = "codegg-tui";

/// A daemon with one connected local (socket-style) TUI client, which is how a
/// plain `codegg` launch reaches the singleton daemon.
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

/// Dispatch as a specific connected client. `handle_request` (no client id)
/// falls back to the `local-daemon` identity, which would make every request
/// LocalOwner regardless of who registered, so authorization assertions must
/// name their client explicitly.
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

/// Dispatch as the local TUI client established by [`local_tui_daemon`].
async fn call(daemon: &CoreDaemon, request_id: &str, request: CoreRequest) -> CoreResponse {
    call_as(daemon, TUI_CLIENT, request_id, request).await
}

fn error_of(response: &CoreResponse) -> (String, String) {
    match response {
        CoreResponse::Error { code, message } => (code.clone(), message.clone()),
        other => panic!("expected an error response, got {other:?}"),
    }
}

/// The two-step registration the TUI launch bootstrap performs, in order.
/// Both operations are idempotent by construction.
async fn launch_bootstrap(daemon: &CoreDaemon, root: &std::path::Path) -> (String, String) {
    let workspace_id = match call(
        daemon,
        "launch-workspace",
        CoreRequest::WorkspaceRegister {
            root: root.to_string_lossy().into_owned(),
        },
    )
    .await
    {
        CoreResponse::WorkspaceSnapshot { workspace } => workspace.workspace_id,
        other => panic!("workspace registration failed: {other:?}"),
    };
    let display_name = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let project_id = match call(
        daemon,
        "launch-project",
        CoreRequest::ProjectRegister {
            request: ProjectRegisterRequestDto {
                workspace_id: workspace_id.clone(),
                display_name,
                description: None,
                tags: Vec::new(),
                repository_id: None,
                source: "tui-launch".to_owned(),
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

fn session_create(
    directory: &str,
    project_id: Option<String>,
    workspace_id: Option<String>,
) -> CoreRequest {
    CoreRequest::SessionCreate {
        directory: directory.to_owned(),
        title: Some("bootstrap regression".to_owned()),
        project_id,
        workspace_id,
    }
}

/// The reported failure: a directory CodeGG has never registered cannot create
/// a session, because directory resolution is lookup-only. This pins the
/// premise the bootstrap exists to fix — if this ever starts succeeding
/// without registration, the bootstrap is unnecessary rather than harmful.
#[tokio::test(flavor = "current_thread")]
async fn unregistered_directory_cannot_create_a_session() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    let directory = root.path().to_string_lossy().into_owned();

    let response = call(
        &daemon,
        "create-unregistered",
        session_create(&directory, None, None),
    )
    .await;
    let (code, message) = error_of(&response);
    assert_eq!(
        code, "project_context_required",
        "an unregistered directory must still fail closed, got {response:?}"
    );
    assert!(
        message.contains("project/workspace context is not executable"),
        "unexpected message: {message}"
    );
}

/// The fix: after the launch bootstrap, the identical directory-scoped
/// session create succeeds with no identity fields at all.
#[tokio::test(flavor = "current_thread")]
async fn launch_bootstrap_makes_session_creation_work_in_a_new_directory() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    let directory = root.path().to_string_lossy().into_owned();

    // Premise: broken before registration.
    let before = call(
        &daemon,
        "create-before",
        session_create(&directory, None, None),
    )
    .await;
    assert_eq!(error_of(&before).0, "project_context_required");

    let (_project_id, _workspace_id) = launch_bootstrap(&daemon, root.path()).await;

    // The composer path: directory locator only, no typed identities.
    let after = call(
        &daemon,
        "create-after",
        session_create(&directory, None, None),
    )
    .await;
    match after {
        CoreResponse::Session { session } => {
            assert!(!session.id.is_empty(), "session must have an id");
        }
        other => panic!("session create after bootstrap must succeed: {other:?}"),
    }

    // The typed-pair path the new-session command now sends.
    let (project_id, workspace_id) = launch_bootstrap(&daemon, root.path()).await;
    let typed = call(
        &daemon,
        "create-typed",
        session_create(&directory, Some(project_id), Some(workspace_id)),
    )
    .await;
    assert!(
        matches!(typed, CoreResponse::Session { .. }),
        "typed session create must succeed: {typed:?}"
    );
}

/// The bootstrap runs on every launch, so it must be idempotent: the same ids
/// and no second project for the same workspace.
#[tokio::test(flavor = "current_thread")]
async fn launch_bootstrap_is_idempotent_across_repeated_launches() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");

    let first = launch_bootstrap(&daemon, root.path()).await;
    let second = launch_bootstrap(&daemon, root.path()).await;
    let third = launch_bootstrap(&daemon, root.path()).await;

    assert_eq!(
        first, second,
        "relaunch must re-resolve the same identities"
    );
    assert_eq!(second, third, "relaunch must not create a new project");

    // Exactly one project exists for this workspace.
    let listed = call(
        &daemon,
        "list-projects",
        CoreRequest::ProjectList {
            include_archived: false,
            limit: 0,
        },
    )
    .await;
    match listed {
        CoreResponse::ProjectList { projects, .. } => {
            let matching: Vec<_> = projects
                .iter()
                .filter(|p| p.project_id == first.0)
                .collect();
            assert_eq!(matching.len(), 1, "exactly one project per launch root");
        }
        other => panic!("project list failed: {other:?}"),
    }
}

/// The session list resolves a directory locator the same way, so it recovers
/// in a freshly bootstrapped directory too.
#[tokio::test(flavor = "current_thread")]
async fn session_list_recovers_in_a_bootstrap_directory() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    let directory = root.path().to_string_lossy().into_owned();

    let before = call(
        &daemon,
        "list-before",
        CoreRequest::SessionList {
            project_id: directory.clone(),
            show_archived: false,
            limit: 10,
        },
    )
    .await;
    assert_eq!(
        error_of(&before).0,
        "project_context_required",
        "listing an unregistered directory must fail closed"
    );

    launch_bootstrap(&daemon, root.path()).await;

    let after = call(
        &daemon,
        "list-after",
        CoreRequest::SessionList {
            project_id: directory.clone(),
            show_archived: false,
            limit: 10,
        },
    )
    .await;
    assert!(
        matches!(after, CoreResponse::SessionList { .. }),
        "session list must resolve after bootstrap: {after:?}"
    );
}

/// A path in `project_id` with no `workspace_id` is rejected by construction.
/// The new-session command used to send exactly that shape, which is why
/// ctrl+N could only ever fail with "must be provided together".
#[tokio::test(flavor = "current_thread")]
async fn path_shaped_project_id_without_workspace_is_rejected() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool).await;
    let root = tempfile::tempdir().expect("tempdir");
    let directory = root.path().to_string_lossy().into_owned();
    launch_bootstrap(&daemon, root.path()).await;

    let response = call(
        &daemon,
        "create-path-as-identity",
        session_create(&directory, Some(directory.clone()), None),
    )
    .await;
    let (code, message) = error_of(&response);
    assert_eq!(code, "project_context_required");
    assert!(
        message.contains("project_id and workspace_id must be provided together"),
        "unexpected message: {message}"
    );
}

/// The bootstrap is authorized for a local TUI client — the only mode a plain
/// `codegg` launch runs in. A remote principal without project configuration
/// rights must still be denied, so this does not widen the trust boundary.
#[tokio::test(flavor = "current_thread")]
async fn launch_bootstrap_is_authorized_for_a_local_tui_client() {
    let pool = test_pool().await;
    let daemon = local_tui_daemon(pool.clone()).await;
    let root = tempfile::tempdir().expect("tempdir");

    let (project_id, workspace_id) = launch_bootstrap(&daemon, root.path()).await;
    assert!(!project_id.is_empty());
    assert!(!workspace_id.is_empty());

    // Registration is still `LocalOwner`-gated: an unauthorized team
    // principal must not be able to name an arbitrary root.
    let team = codegg_core::team::TeamStore::new(pool.clone());
    let tokens =
        codegg_core::transport_auth::PersonalTokenStore::with_team(pool.clone(), team.clone());
    let record = team
        .create_principal(codegg_core::team::PrincipalKind::Human, "Launch-outsider")
        .await
        .expect("create principal");
    let (plaintext, _) = tokens
        .create_personal_token(&record.id, "device", None)
        .await
        .expect("create token");
    let outsider = tokens
        .verify_for_client(&plaintext, "client-outsider")
        .await
        .expect("verify token");
    daemon.clients.register_with_principal(
        "client-outsider".to_owned(),
        "client-outsider".to_owned(),
        None,
        outsider,
    );

    let denied = call_as(
        &daemon,
        "client-outsider",
        "outsider-workspace-register",
        CoreRequest::WorkspaceRegister {
            root: root.path().to_string_lossy().into_owned(),
        },
    )
    .await;
    let (code, message) = error_of(&denied);
    // `workspace_register` is `ScopeKind::Opaque` with `project.configure`.
    // The outsider is an active principal, so it clears the status check and
    // fails on the absent project scope — raw root registration stays
    // LocalOwner-gated. This proves the launch bootstrap widens nothing: it
    // only makes an already-authorized local client exercise that authority.
    assert_eq!(
        code, "authorization_scope_required",
        "an unauthorized principal must not be able to register a raw root: {denied:?}"
    );
    assert!(
        !message.contains(&root.path().to_string_lossy().into_owned()),
        "the denial must not echo the rejected path: {message}"
    );
}
