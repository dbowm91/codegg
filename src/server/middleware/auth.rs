use axum::{extract::{Request, State}, middleware::Next, response::Response};
use http::StatusCode;
use subtle::ConstantTimeEq;

use crate::server::state::ServerState;

pub async fn auth_middleware(
    State(state): State<ServerState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_disabled = std::env::var("CODEGG_SERVER_AUTH_DISABLED").is_ok();
    if auth_disabled {
        return Ok(next.run(request).await);
    }

    let expected_token = std::env::var("CODEGG_SERVER_TOKEN").ok().or_else(|| {
        state.config.server.as_ref().and_then(|s| s.token.clone())
    });

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
        None => {
            // If no token is configured and auth is not explicitly disabled,
            // we default to requiring it but it's impossible to provide.
            // This is a safety measure.
            Err(StatusCode::UNAUTHORIZED)
        }
    }
}

pub fn validate_token(provided: &str, expected: &str) -> bool {
    provided.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1
}
