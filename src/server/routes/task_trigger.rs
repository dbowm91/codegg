//! Narrow external task-trigger fire endpoint (Project Work Orders M005).
//!
//! `POST /api/v1/task-triggers/:trigger_id/fire` with
//! `Authorization: Bearer cggtr_<trigger-id>.<secret>` latches the bound
//! `ExternalTrigger` gate and wakes the M002 coordinator. It performs
//! exactly one semantic action and grants no other access.
//!
//! ## Boundary rules (enforced here, pinned by tests + static guard)
//!
//! - POST-only: axum answers any other method with `405` and no side
//!   effect. `GET` can never fire.
//! - The bearer is a narrow capability, never a principal credential:
//!   this router sits outside the principal `auth_middleware`, and
//!   principal-shaped bearers (`cggt_...`, global tokens) presented here
//!   are rejected without principal verification.
//! - Query strings are never read: a secret in the URL is ignored, never
//!   accepted, and never echoed.
//! - The request body must be empty (the initial endpoint needs none);
//!   anything larger than the tiny bound is rejected with `413`.
//! - Failures are privacy-safe: unknown locators, wrong secrets, and
//!   revoked/expired/exhausted triggers share one generic `401` shape
//!   that carries no locator, project, secret, or verifier content.
//! - The `Authorization` header value never enters logs, events, audit
//!   metadata, or error bodies. Only the public locator and the narrow
//!   outcome word are logged.
//! - Responses carry a narrow status word (`accepted` / `already_fired`)
//!   plus a stable opaque receipt. No project, work-order, occurrence,
//!   session, model, or gate detail leaves this endpoint.

use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json},
    routing::post,
    Router,
};
use http::header::AUTHORIZATION;
use serde::Serialize;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::set_header::SetResponseHeaderLayer;

use super::super::state::ServerState;
use codegg_core::identity::TaskTriggerId;
use codegg_core::work_order::{
    is_task_trigger_presentation, split_presented_trigger, FireOutcome, MAX_PRESENTED_TRIGGER_LEN,
    MAX_TRIGGER_FIRE_BODY_BYTES, MAX_TRIGGER_IDEMPOTENCY_KEY_LEN,
};

/// Build the trigger fire router: one POST route with its own
/// IP-keyed rate limiter (same budget posture as the global limiter),
/// a tiny body cap, and hardening response headers. No principal auth
/// layer: the trigger bearer is verified inside the handler against
/// the stored verifier only.
pub fn task_trigger_router(state: ServerState, max_requests: usize, window_secs: u64) -> Router {
    let limiter = crate::server::http::RateLimiter::new(max_requests, window_secs);
    Router::new()
        .route(
            "/api/v1/task-triggers/{trigger_id}/fire",
            post(fire_task_trigger),
        )
        .layer(axum::middleware::from_fn_with_state(
            limiter,
            crate::server::http::rate_limit_middleware,
        ))
        .layer(RequestBodyLimitLayer::new(MAX_TRIGGER_FIRE_BODY_BYTES))
        .layer(SetResponseHeaderLayer::overriding(
            http::header::X_CONTENT_TYPE_OPTIONS,
            http::HeaderValue::from_static("nosniff"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            http::header::X_FRAME_OPTIONS,
            http::HeaderValue::from_static("DENY"),
        ))
        .with_state(state)
}

#[derive(Serialize)]
struct FireSuccessBody {
    status: String,
    receipt_id: String,
}

#[derive(Serialize)]
struct FireErrorBody {
    code: &'static str,
    message: &'static str,
}

fn invalid_trigger() -> (StatusCode, Json<FireErrorBody>) {
    (
        StatusCode::UNAUTHORIZED,
        Json(FireErrorBody {
            code: "trigger_invalid",
            message: "invalid or inactive trigger",
        }),
    )
}

/// Narrow fire handler. Every early return precedes any store lookup
/// (framing) or shares the generic inactive shape (credential/state),
/// so responses never oracle trigger existence or lifecycle.
pub async fn fire_task_trigger(
    State(state): State<ServerState>,
    Path(trigger_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    // Bounded path locator; malformed locators are framing errors that
    // perform no lookup.
    if trigger_id.is_empty()
        || trigger_id.len() > MAX_PRESENTED_TRIGGER_LEN
        || TaskTriggerId::parse(&trigger_id).is_err()
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(FireErrorBody {
                code: "trigger_invalid_request",
                message: "malformed trigger locator",
            }),
        )
            .into_response();
    }
    // The initial endpoint needs no body; reject anything non-empty
    // rather than parsing it. (The router-level 413 cap bounds memory;
    // this rejects even tiny payloads so no prompt/task mutation can
    // ever ride a fire request.)
    if !body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(FireErrorBody {
                code: "trigger_invalid_request",
                message: "fire endpoint accepts no body",
            }),
        )
            .into_response();
    }
    // Bearer extraction only: no query, cookie, or alternate header is
    // ever consulted, so URL smuggling cannot authenticate.
    let presented = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(presented) = presented else {
        return invalid_trigger().into_response();
    };
    if presented.len() > MAX_PRESENTED_TRIGGER_LEN {
        return invalid_trigger().into_response();
    }
    // Principal-shaped bearers are rejected here without principal
    // verification: this endpoint must never mint a principal session
    // from a trigger-shaped or principal-shaped value alike — only the
    // trigger verifier path below authenticates.
    if !is_task_trigger_presentation(presented) {
        return invalid_trigger().into_response();
    }
    let Some((bearer_locator, _)) = split_presented_trigger(presented) else {
        return invalid_trigger().into_response();
    };
    if bearer_locator != trigger_id {
        return invalid_trigger().into_response();
    }
    let idempotency_key: Option<String> = match headers.get("idempotency-key") {
        None => None,
        Some(raw) => match raw.to_str() {
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(FireErrorBody {
                        code: "trigger_invalid_request",
                        message: "malformed idempotency key",
                    }),
                )
                    .into_response();
            }
            Ok(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() || trimmed.len() > MAX_TRIGGER_IDEMPOTENCY_KEY_LEN {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(FireErrorBody {
                            code: "trigger_invalid_request",
                            message: "malformed idempotency key",
                        }),
                    )
                        .into_response();
                }
                Some(trimmed.to_owned())
            }
        },
    };
    let Some(daemon) = state.daemon.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(FireErrorBody {
                code: "trigger_unavailable",
                message: "task triggers are unavailable on this server",
            }),
        )
            .into_response();
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    match daemon
        .fire_work_order_trigger(presented, idempotency_key.as_deref(), now_ms)
        .await
    {
        Ok(outcome) => {
            tracing::info!(
                trigger_id = %trigger_id,
                status = outcome.status(),
                duplicate = outcome.duplicate,
                "task trigger fire resolved"
            );
            fire_success(outcome).into_response()
        }
        Err(codegg_core::work_order::WorkOrderError::NotFound(_)) => {
            tracing::info!(trigger_id = %trigger_id, "task trigger fire rejected");
            invalid_trigger().into_response()
        }
        Err(codegg_core::work_order::WorkOrderError::Invalid { .. }) => (
            StatusCode::BAD_REQUEST,
            Json(FireErrorBody {
                code: "trigger_invalid_request",
                message: "malformed fire request",
            }),
        )
            .into_response(),
        Err(codegg_core::work_order::WorkOrderError::Unavailable(_)) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(FireErrorBody {
                code: "trigger_unavailable",
                message: "task triggers are unavailable on this server",
            }),
        )
            .into_response(),
        Err(_) => {
            tracing::warn!(trigger_id = %trigger_id, "task trigger fire failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(FireErrorBody {
                    code: "trigger_unavailable",
                    message: "task trigger fire failed",
                }),
            )
                .into_response()
        }
    }
}

fn fire_success(outcome: FireOutcome) -> (StatusCode, Json<FireSuccessBody>) {
    (
        StatusCode::OK,
        Json(FireSuccessBody {
            status: outcome.status().to_owned(),
            receipt_id: outcome.receipt_id,
        }),
    )
}
