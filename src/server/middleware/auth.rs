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

/// M002 transport authentication and principal binding.
///
/// Personal-local startup remains login-free: trusted local transports bind
/// `LocalOwner` without a credential. Network transports fail closed and
/// resolve every accepted connection to a canonical principal from trusted
/// evidence only.
///
/// Resolution order for network callers:
/// 1. Personal token (`cggt_...`) verified against the digest store. On
///    success the caller is bound to the token's canonical principal as
///    `AuthenticatedRemote`. Distinct tokens bind distinct principals.
/// 2. Legacy global bearer (`CODEGG_SERVER_TOKEN` / `server.token`) as a
///    bootstrap/compatibility credential. On success the caller is bound to
///    `LocalOwner` via [`AuthenticatedPrincipal::bootstrap_global_bearer`].
///    This MUST NOT masquerade as a distinct team identity. Removal
///    condition: once every operator has a personal token and no deployment
///    relies on the shared secret, delete `server.token` /
///    `CODEGG_SERVER_TOKEN` and require personal tokens; the bootstrap path
///    is then dead code to be removed.
/// 3. Anything else (missing, wrong, revoked, expired, disabled principal,
///    or no credential configured) is rejected fail-closed.
///
/// Secrets never enter logs/events. Only token kind is logged via
/// [`codegg_core::transport_auth::redact_presented_token`].
pub async fn auth_middleware(
    State(state): State<ServerState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Explicit opt-in disable remains for local diagnostics. Even then the
    // request carries an explicit LocalOwner principal so downstream code
    // never observes an anonymous request.
    if auth_disabled_by_env() {
        request.extensions_mut().insert(
            codegg_core::transport_auth::AuthenticatedPrincipal::local_owner("http-auth-disabled"),
        );
        return Ok(next.run(request).await);
    }

    let auth_header = request
        .headers()
        .get(http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok());
    let presented = auth_header.and_then(|h| h.strip_prefix("Bearer "));

    let Some(presented) = presented else {
        // No credential: fail closed. When no credential is even configured
        // the listener cannot distinguish callers, so report 503 to match
        // the historical fail-closed contract; otherwise 401.
        if resolve_expected_token(&state.config).is_none() {
            // Also fail when no personal-token backend can distinguish
            // callers: without a pool there is no token store.
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        return Err(StatusCode::UNAUTHORIZED);
    };

    let client_id = format!("http-{}", uuid::Uuid::new_v4());
    let principal =
        resolve_bearer_principal(presented, &state.config, &state.pool, &client_id).await?;
    request.extensions_mut().insert(principal);
    Ok(next.run(request).await)
}

/// Resolve one presented bearer value to a canonical principal.
///
/// Personal-token presentations (`cggt_...`) are always verified against
/// the digest store, even when a global bearer is also configured. Legacy
/// presentations are compared constant-time against the expected global
/// bearer and, on success, bind `LocalOwner` as bootstrap compatibility.
pub async fn resolve_bearer_principal(
    presented: &str,
    config: &crate::config::schema::Config,
    pool: &sqlx::SqlitePool,
    client_id: &str,
) -> Result<codegg_core::transport_auth::AuthenticatedPrincipal, StatusCode> {
    use codegg_core::transport_auth::{is_personal_token_presentation, PersonalTokenStore};

    if is_personal_token_presentation(presented) {
        let store = PersonalTokenStore::new(pool.clone());
        return store
            .verify_for_client(presented, client_id)
            .await
            .map_err(|_| StatusCode::UNAUTHORIZED);
    }

    let expected_token = resolve_expected_token(config);
    match expected_token {
        Some(expected) => {
            if validate_token(presented, &expected) {
                Ok(
                    codegg_core::transport_auth::AuthenticatedPrincipal::bootstrap_global_bearer(
                        client_id,
                    ),
                )
            } else {
                Err(StatusCode::UNAUTHORIZED)
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
