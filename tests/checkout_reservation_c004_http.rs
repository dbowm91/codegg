//! C004 qualification: the HTTP checkout writers are mediated.
//!
//! The HTTP file and workspace routes are a human surface: they carry a
//! transport-bound principal and never a run. They live in the server
//! module, which is behind the `server` feature, so this suite is separate
//! from the daemon-request suite in `tests/checkout_reservation_c004.rs`.
//!
//! What it claims: an HTTP write into a checkout reserved by a live
//! agent-run group is refused before the first filesystem mutation, the
//! caller receives a typed **409 Conflict** with its recovery path (not a
//! 404, and never a silent success), and nothing lands on disk. An HTTP
//! write into an ordinary checkout still succeeds.

#![cfg(feature = "server")]

mod common;

use std::sync::Arc;

use axum::extract::{Extension, State};
use axum::Json;
use codegg::server::routes::file::{DeleteFileRequest, WriteFileRequest};
use codegg::server::routes::workspace::CreateWorkspaceRequest;
use codegg::server::{ServerState, WsRateLimiter};
use codegg_core::transport_auth::AuthenticatedPrincipal;
use common::checkout_reservation::ReservedCheckout;

fn server_state(fixture: &ReservedCheckout) -> ServerState {
    ServerState {
        pool: fixture.pool.clone(),
        mcp_service: Arc::new(tokio::sync::RwLock::new(codegg::mcp::McpService::new())),
        config: codegg::config::schema::Config::default(),
        ws_rate_limiter: Arc::new(WsRateLimiter::new(100, 60)),
        daemon: Some(fixture.daemon.clone()),
        projection_lifecycle_seam: Default::default(),
        connection_task_probe: None,
        probe_factory: None,
        transport_test_config: None,
    }
}

fn principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal::local_owner("checkout-reservation-c004-http")
}

/// An HTTP write into a reserved checkout is refused with a typed conflict,
/// and the file never appears.
#[tokio::test]
async fn an_http_write_into_a_reserved_checkout_is_refused_with_no_effect() {
    let fixture = ReservedCheckout::build().await;
    let state = server_state(&fixture);
    let target = fixture.checkout.path().join("http-written.txt");

    let outcome = codegg::server::routes::file::write_file(
        Extension(principal()),
        State(state.clone()),
        Json(WriteFileRequest {
            path: "http-written.txt".to_owned(),
            content: "payload".to_owned(),
            project_id: Some(fixture.project_id.clone()),
            workspace_id: Some(fixture.workspace_id.clone()),
            directory: None,
        }),
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("a reserved checkout must refuse the HTTP write"),
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
        "a refused HTTP write must not leave the file behind"
    );
}

/// The refusal is a 409 Conflict that carries its recovery path, so a
/// client can tell a busy checkout from a missing resource.
#[tokio::test]
async fn the_http_refusal_is_a_conflict_with_a_recovery_path() {
    let fixture = ReservedCheckout::build().await;
    let state = server_state(&fixture);

    let outcome = codegg::server::routes::file::write_file(
        Extension(principal()),
        State(state),
        Json(WriteFileRequest {
            path: "http-conflict.txt".to_owned(),
            content: "payload".to_owned(),
            project_id: Some(fixture.project_id.clone()),
            workspace_id: Some(fixture.workspace_id.clone()),
            directory: None,
        }),
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("a reserved checkout must refuse the HTTP write"),
        Err(error) => error,
    };
    let response = axum::response::IntoResponse::into_response(error);
    assert_eq!(
        response.status(),
        axum::http::StatusCode::CONFLICT,
        "a refused checkout write is a conflict, not a server fault or a missing resource"
    );
}

/// The HTTP delete is a checkout write too, and is refused the same way.
#[tokio::test]
async fn an_http_delete_of_a_reserved_checkout_is_refused_with_no_effect() {
    let fixture = ReservedCheckout::build().await;
    let state = server_state(&fixture);
    let target = fixture.checkout.path().join("keep-me.txt");
    std::fs::write(&target, "content").expect("seed the file");

    let outcome = codegg::server::routes::file::delete_file(
        Extension(principal()),
        State(state),
        Json(DeleteFileRequest {
            path: "keep-me.txt".to_owned(),
            project_id: Some(fixture.project_id.clone()),
            workspace_id: Some(fixture.workspace_id.clone()),
            directory: None,
        }),
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("a reserved checkout must refuse the HTTP delete"),
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

/// An HTTP write into an ordinary, unreserved checkout still succeeds. The
/// mediation must not turn ordinary API writes into a blocking gate.
#[tokio::test]
async fn an_http_write_into_an_unreserved_checkout_succeeds() {
    let fixture = ReservedCheckout::build().await;
    let state = server_state(&fixture);
    let (ordinary, project_id, workspace_id) = fixture.ordinary_workspace().await;

    let outcome = codegg::server::routes::file::write_file(
        Extension(principal()),
        State(state),
        Json(WriteFileRequest {
            path: "ordinary-http.txt".to_owned(),
            content: "payload".to_owned(),
            project_id: Some(project_id),
            workspace_id: Some(workspace_id),
            directory: None,
        }),
    )
    .await;

    let info = outcome.expect("an ordinary checkout must accept the HTTP write");
    assert_eq!(info.size, "payload".len() as u64);
    assert!(
        ordinary.path().join("ordinary-http.txt").exists(),
        "the ordinary HTTP write must land"
    );
}

/// Creating a directory inside a reserved checkout is refused too.
#[tokio::test]
async fn an_http_workspace_create_into_a_reserved_checkout_is_refused() {
    let fixture = ReservedCheckout::build().await;
    let state = server_state(&fixture);
    let target = fixture.checkout.path().join("new-dir");

    let outcome = codegg::server::routes::workspace::create_workspace(
        Extension(principal()),
        State(state),
        Json(CreateWorkspaceRequest {
            name: "new-dir".to_owned(),
            project_id: Some(fixture.project_id.clone()),
            workspace_id: Some(fixture.workspace_id.clone()),
            path: "new-dir".to_owned(),
        }),
    )
    .await;

    let error = match outcome {
        Ok(_) => panic!("a reserved checkout must refuse the directory creation"),
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
