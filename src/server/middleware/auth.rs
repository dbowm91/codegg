use axum::{extract::Request, middleware::Next, response::Response};
use subtle::ConstantTimeEq;

use crate::error::{AppError, ProviderError, ServerRuntimeError};

#[allow(dead_code)]
pub struct AuthMiddleware {
    expected_token: Option<String>,
    auth_required: bool,
}

impl AuthMiddleware {
    #[allow(dead_code)]
    pub fn new() -> Self {
        let expected_token = std::env::var("CODEGG_SERVER_TOKEN").ok();
        let auth_required = std::env::var("CODEGG_SERVER_AUTH_DISABLED").is_err();
    let expected = std::env::var("CODEGG_SERVER_TOKEN").ok();
    let auth_required = std::env::var("CODEGG_SERVER_AUTH_DISABLED").is_err();

    if auth_required && expected.is_none() {
        return Err(AppError::Provider(ProviderError::Auth(
            "server not configured with auth token".to_string(),
        )));
    }

    if let Some(expected_token) = expected {
        match token {
            Some(t) if t.as_bytes().ct_eq(expected_token.as_bytes()).unwrap_u8() == 1 => {
                Ok(next.run(request).await)
            }
            _ => Err(AppError::Provider(ProviderError::Auth(
                "invalid or missing token".to_string(),
            ))),
        }
    } else {
        Ok(next.run(request).await)
    }
}

#[allow(dead_code)]
pub fn validate_token(provided: &str, expected: &str) -> Result<(), ServerRuntimeError> {
    if provided.as_bytes().ct_eq(expected.as_bytes()).unwrap_u8() == 1 {
        Ok(())
    } else {
        Err(ServerRuntimeError::Auth("invalid token".to_string()))
    }
}
