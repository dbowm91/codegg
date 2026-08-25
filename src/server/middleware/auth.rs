use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use http::StatusCode;
use subtle::ConstantTimeEq;

use crate::server::state::ServerState;

/// Auth is disabled only via explicit opt-in (`1` or `true`,
/// case-insensitive). Any other value — including `0` or `false` —
/// keeps auth enabled so a co-located subprocess cannot silently turn
/// endpoint auth off with an innocuous value.
pub fn auth_disabled_by_env() -> bool {
    match std::env::var("CODEGG_SERVER_AUTH_DISABLED") {
        Ok(v) => v == "1" || v.eq_ignore_ascii_case("true"),
        Err(_) => false,
    }
}

/// Resolve the expected bearer token: env var first, then config-file
/// token.
pub fn resolve_expected_token(config: &crate::config::schema::Config) -> Option<String> {
    std::env::var("CODEGG_SERVER_TOKEN")
        .ok()
        .or_else(|| config.server.as_ref().and_then(|s| s.token.clone()))
}

pub async fn auth_middleware(
    State(state): State<ServerState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if auth_disabled_by_env() {
        return Ok(next.run(request).await);
    }

    let expected_token = resolve_expected_token(&state.config);

    match expected_token {
        Some(expected) => {
            let auth_header = request
                .headers()
                .get(http::header::AUTHORIZATION)
                .and_then(|h| h.to_str().ok());

            let token = auth_header.and_then(|h| h.strip_prefix("Bearer "));

            match token {
                Some(provided) if validate_token(provided, &expected) => {
                    Ok(next.run(request).await)
                }
                _ => Err(StatusCode::UNAUTHORIZED),
            }
        }
        // Fail closed: with auth enabled and no token configured there
        // is no way to distinguish callers, so refuse to serve instead
        // of accepting unauthenticated traffic.
        None => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}

pub fn validate_token(provided: &str, expected: &str) -> bool {
    provided.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1
}
