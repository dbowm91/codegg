//! M007-H: the daemon's new read surfaces. A minted LSP preview, a
//! daemon-owned security-review run, and the human-shell run listing are
//! daemon state: the client reads the daemon's own answer.

use super::common;

use std::sync::Arc;

use codegg::core::daemon::CoreDaemon;
use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};
use codegg_protocol::human_shell::{
    HumanShellOriginDto, HumanShellRunCreateRequestDto, HumanShellRunListDto,
};
use codegg_protocol::lsp::{LspPreviewCandidateDto, LspPreviewReadDto};
use codegg_protocol::security_review::{
    SecurityReviewReadDto, SecurityReviewRunStatusDto, SecurityReviewStartDto,
    SECURITY_REVIEW_MAX_REPORT_BYTES, SECURITY_REVIEW_PROTOCOL_VERSION,
};

const LOCAL_CLIENT: &str = "m007h-local-client";

fn register_local_client(daemon: &CoreDaemon) {
    daemon.clients.register_with_principal(
        LOCAL_CLIENT.to_owned(),
        "m007h-local".to_owned(),
        None,
        codegg_core::transport_auth::AuthenticatedPrincipal::local_owner(LOCAL_CLIENT),
    );
}

async fn request(daemon: &CoreDaemon, payload: CoreRequest) -> CoreResponse {
    daemon
        .handle_request_for_client(
            RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload,
            },
            LOCAL_CLIENT,
        )
        .await
        .expect("the daemon must answer")
}

async fn seed_session(
    pool: &sqlx::SqlitePool,
    project_id: &str,
    workspace_id: &str,
    session_id: &str,
) {
    sqlx::query(
        "INSERT OR IGNORE INTO project (id, name, worktree, time_created, time_updated, sandboxes) \
         VALUES (?, ?, '/tmp', 1, 1, '[]')",
    )
    .bind(project_id)
    .bind(project_id)
    .execute(pool)
    .await
    .expect("seed the project");
    sqlx::query(
        "INSERT INTO session (id, project_id, workspace_id, slug, directory, title, version, \
         time_created, time_updated) VALUES (?, ?, ?, 's', '/tmp', 't', 'v', 1, 1)",
    )
    .bind(session_id)
    .bind(project_id)
    .bind(workspace_id)
    .execute(pool)
    .await
    .expect("seed the session");
}

fn candidate(preview_id: &str) -> LspPreviewCandidateDto {
    LspPreviewCandidateDto {
        preview_id: preview_id.to_owned(),
        preview_revision: 1,
        preview_digest: "0".repeat(64),
        kind: "format".to_owned(),
        title: "Format".to_owned(),
        provenance: "editor:format".to_owned(),
        patches: Vec::new(),
    }
}

/// M007-H: a minted preview is readable by the session that minted it and by
/// no other session, even when both sessions share the workspace.
#[tokio::test(flavor = "current_thread")]
async fn the_lsp_preview_read_answers_only_for_the_session_that_minted_it() {
    let pool = common::pool::isolated_pool().await;
    let daemon = CoreDaemon::new(Some(pool.clone()), None, None);
    register_local_client(&daemon);
    let root = tempfile::tempdir().expect("a workspace root");
    let workspace = daemon
        .workspaces
        .get_or_register(root.path())
        .await
        .expect("register the workspace");
    let workspace_id = workspace.id.as_str().to_owned();
    seed_session(&pool, "m007h-project", &workspace_id, "m007h-session-a").await;
    seed_session(&pool, "m007h-project", &workspace_id, "m007h-session-b").await;
    daemon
        .lsp_previews
        .register("m007h-session-a", candidate("preview-a"));

    let read = |session_id: &str| CoreRequest::LspPreviewRead {
        request: LspPreviewReadDto {
            preview_id: "preview-a".to_owned(),
            workspace_id: workspace_id.clone(),
            session_id: session_id.to_owned(),
        },
    };
    match request(&daemon, read("m007h-session-a")).await {
        CoreResponse::LspPreviewRead { candidate } => {
            let candidate = candidate.expect("the minting session reads its own candidate");
            assert_eq!(candidate.preview_id, "preview-a");
        }
        other => panic!("expected a preview read, got {other:?}"),
    }
    match request(&daemon, read("m007h-session-b")).await {
        CoreResponse::LspPreviewRead { candidate } => {
            assert!(
                candidate.is_none(),
                "another session must not read a preview it did not mint"
            );
        }
        other => panic!("expected a preview read, got {other:?}"),
    }
}

/// M007-H: the security-review run belongs to the daemon and to the session
/// that started it. Another session of the same workspace does not see it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_security_review_run_is_owned_by_the_daemon_and_its_session() {
    let pool = common::pool::isolated_pool().await;
    let daemon = Arc::new(CoreDaemon::new(Some(pool.clone()), None, None));
    register_local_client(&daemon);
    let root = tempfile::tempdir().expect("a workspace root");
    let workspace = daemon
        .workspaces
        .get_or_register(root.path())
        .await
        .expect("register the workspace");
    let workspace_id = workspace.id.as_str().to_owned();
    seed_session(&pool, "m007h-project", &workspace_id, "m007h-session-a").await;
    seed_session(&pool, "m007h-project", &workspace_id, "m007h-session-b").await;

    let started = request(
        &daemon,
        CoreRequest::SecurityReviewStart {
            request: SecurityReviewStartDto {
                protocol_version: SECURITY_REVIEW_PROTOCOL_VERSION,
                project_id: "m007h-project".to_owned(),
                workspace_id: workspace_id.clone(),
                session_id: "m007h-session-a".to_owned(),
                argument: String::new(),
            },
        },
    )
    .await;
    let CoreResponse::SecurityReviewRun { run } = started else {
        panic!("expected a security review run, got {started:?}");
    };
    assert_eq!(run.session_id, "m007h-session-a");
    assert!(!run.run_id.is_empty());

    let read = |session_id: &str, run_id: Option<String>| CoreRequest::SecurityReviewRead {
        request: SecurityReviewReadDto {
            protocol_version: SECURITY_REVIEW_PROTOCOL_VERSION,
            project_id: "m007h-project".to_owned(),
            workspace_id: workspace_id.clone(),
            session_id: session_id.to_owned(),
            run_id,
        },
    };
    let mut settled = None;
    for _ in 0..200 {
        let answer = request(&daemon, read("m007h-session-a", Some(run.run_id.clone()))).await;
        let CoreResponse::SecurityReviewRun { run } = answer else {
            panic!("expected a security review run, got {answer:?}");
        };
        if run.status != SecurityReviewRunStatusDto::Running {
            settled = Some(run);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let settled = settled.expect("the run must reach a terminal state");
    assert_eq!(settled.run_id, run.run_id);
    if let Some(report) = settled.report.as_deref() {
        assert!(
            report.len() <= SECURITY_REVIEW_MAX_REPORT_BYTES,
            "the report must be bounded by the protocol's own bound"
        );
    }

    let other = request(&daemon, read("m007h-session-b", Some(run.run_id.clone()))).await;
    assert!(
        matches!(other, CoreResponse::Error { .. }),
        "another session must not read this run, got {other:?}"
    );
    let other_newest = request(&daemon, read("m007h-session-b", None)).await;
    assert!(
        matches!(other_newest, CoreResponse::Error { .. }),
        "another session must not see this run as its newest, got {other_newest:?}"
    );
}

/// M007-H: the run listing carries the command and the working directory
/// each run used, and it is bounded by the request's own limit.
#[tokio::test(flavor = "current_thread")]
async fn the_human_shell_run_list_carries_the_command_and_is_bounded() {
    let daemon = CoreDaemon::new(None, None, None);
    let root = tempfile::tempdir().expect("a workspace root");
    let workspace = daemon
        .workspaces
        .get_or_register(root.path())
        .await
        .expect("register the workspace");
    let workspace_id = workspace.id.as_str().to_owned();
    let canonical_root = root.path().canonicalize().expect("canonical root");

    let created = daemon
        .handle_request(codegg::core::new_request(
            "m007h-shell-create".into(),
            CoreRequest::HumanShellRunCreate {
                request: HumanShellRunCreateRequestDto {
                    protocol_version: codegg_protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION,
                    submission_id: "m007h-submission".into(),
                    project_id: "m007h-project".into(),
                    workspace_id: workspace_id.clone(),
                    session_id: None,
                    origin: HumanShellOriginDto::Bang,
                    confirmed_dangerous: false,
                    command: "printf listed".into(),
                    timeout_secs: 300,
                },
            },
        ))
        .await
        .expect("the daemon must answer");
    let CoreResponse::HumanShellRunCreated { run, .. } = created else {
        panic!("expected a human shell run, got {created:?}");
    };

    let list = |limit: Option<u32>| CoreRequest::HumanShellRunList {
        request: HumanShellRunListDto {
            protocol_version: codegg_protocol::human_shell::HUMAN_SHELL_PROTOCOL_VERSION,
            project_id: "m007h-project".into(),
            workspace_id: workspace_id.clone(),
            session_id: None,
            limit,
        },
    };
    let answer = daemon
        .handle_request(codegg::core::new_request(
            "m007h-shell-list".into(),
            list(Some(4)),
        ))
        .await
        .expect("the daemon must answer");
    let CoreResponse::HumanShellRunList { runs } = answer else {
        panic!("expected a human shell run list, got {answer:?}");
    };
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].run_id, run.run_id);
    assert_eq!(runs[0].command, "printf listed");
    assert_eq!(runs[0].cwd, canonical_root.to_string_lossy());

    let bounded = daemon
        .handle_request(codegg::core::new_request(
            "m007h-shell-list-zero".into(),
            list(Some(0)),
        ))
        .await
        .expect("the daemon must answer");
    let CoreResponse::HumanShellRunList { runs } = bounded else {
        panic!("expected a human shell run list, got {bounded:?}");
    };
    assert!(runs.is_empty(), "the listing must honour its own limit");
}
