//! C008 qualification: the HTTP checkout writers are never ungated.
//!
//! C004 mediated the HTTP file and workspace routes only `if let Some(daemon)`.
//! A server with no daemon — which `codegg server` really can be, when the
//! project-local store or the agent resolution fails at startup — therefore
//! wrote into a reserved checkout with **no gate at all**.
//!
//! What it claims: with no daemon present, `POST /file`, `DELETE /file` and
//! `POST /workspace` refuse with the same typed **409 Conflict** the daemon
//! side uses, and nothing lands on disk. The refusal is a conflict, not an
//! authorization failure: the caller's own authority is untouched.

#![cfg(feature = "server")]

use std::sync::Arc;

use super::common::checkout_reservation::ReservedCheckout;
use axum::extract::{Extension, State};
use axum::Json;
use codegg::server::routes::file::{DeleteFileRequest, WriteFileRequest};
use codegg::server::routes::workspace::CreateWorkspaceRequest;
use codegg::server::{ServerState, WsRateLimiter};
use codegg_core::transport_auth::AuthenticatedPrincipal;

/// A server state with **no daemon** — the configuration C004 left ungated.
fn daemonless_server_state(fixture: &ReservedCheckout) -> ServerState {
    ServerState {
        pool: fixture.pool.clone(),
        mcp_service: Arc::new(tokio::sync::RwLock::new(codegg::mcp::McpService::new())),
        config: codegg::config::schema::Config::default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(100, 60)),
        daemon: None,
        projection_lifecycle_seam: Default::default(),
        connection_task_probe: None,
        probe_factory: None,
        transport_test_config: None,
    }
}

fn principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::local_owner("checkout-mediation-c008-http")
}

/// The HTTP write is refused when the server has no daemon, and the file
/// never appears.
#[tokio::test]
async fn the_http_file_write_is_refused_without_a_daemon() {
    let fixture = ReservedCheckout::build().await;
    let state = daemonless_server_state(&fixture);
    let target = fixture.checkout.path().join("ungated-http-written.txt");

    let outcome = codegg::server::routes::file::write_file(
        Extension(principal()),
        State(state),
        Json(WriteFileRequest {
            path: "ungated-http-written.txt".to_owned(),
            content: "payload".to_owned(),
            project_id: Some(fixture.project_id.clone()),
            workspace_id: Some(fixture.workspace_id.clone()),
            directory: None,
        }),
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("a write without the mediation gate must be refused"),
        Err(error) => error,
    };
    assert!(
        matches!(
            &error.0,
            codegg::error::AppError::Storage(codegg::error::StorageError::Conflict(_))
        ),
        "expected a typed conflict, got {:?}",
        error.0
    );
    let response = axum::response::IntoResponse::into_response(error);
    assert_eq!(
        response.status(),
        axum::http::StatusCode::CONFLICT,
        "the refusal must be a conflict, not an authorization failure or a server fault"
    );
    assert!(
        !target.exists(),
        "a refused HTTP write must not leave the file behind"
    );
}

/// The HTTP delete is a checkout write too, and it is refused the same way.
#[tokio::test]
async fn the_http_file_delete_is_refused_without_a_daemon() {
    let fixture = ReservedCheckout::build().await;
    let state = daemonless_server_state(&fixture);
    let target = fixture.checkout.path().join("keep-me-c008.txt");
    std::fs::write(&target, "content").expect("seed the file");

    let outcome = codegg::server::routes::file::delete_file(
        Extension(principal()),
        State(state),
        Json(DeleteFileRequest {
            path: "keep-me-c008.txt".to_owned(),
            project_id: Some(fixture.project_id.clone()),
            workspace_id: Some(fixture.workspace_id.clone()),
            directory: None,
        }),
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("a delete without the mediation gate must be refused"),
        Err(error) => error,
    };
    assert!(
        matches!(
            &error.0,
            codegg::error::AppError::Storage(codegg::error::StorageError::Conflict(_))
        ),
        "expected a typed conflict, got {:?}",
        error.0
    );
    assert!(
        target.exists(),
        "a refused HTTP delete must not remove the file"
    );
}

/// Creating a directory inside a checkout is a checkout write too.
#[tokio::test]
async fn the_http_workspace_create_is_refused_without_a_daemon() {
    let fixture = ReservedCheckout::build().await;
    let state = daemonless_server_state(&fixture);
    let target = fixture.checkout.path().join("ungated-new-dir");

    let outcome = codegg::server::routes::workspace::create_workspace(
        Extension(principal()),
        State(state),
        Json(CreateWorkspaceRequest {
            name: "ungated-new-dir".to_owned(),
            project_id: Some(fixture.project_id.clone()),
            workspace_id: Some(fixture.workspace_id.clone()),
            path: "ungated-new-dir".to_owned(),
        }),
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("a directory creation without the mediation gate must be refused"),
        Err(error) => error,
    };
    assert!(
        matches!(
            &error.0,
            codegg::error::AppError::Storage(codegg::error::StorageError::Conflict(_))
        ),
        "expected a typed conflict, got {:?}",
        error.0
    );
    assert!(
        !target.exists(),
        "a refused directory creation must not leave the directory behind"
    );
}
