//! Runtime inference feedback for provider connection qualification (M010).
//!
//! Provisioning can only ever establish *catalog reachability* for most
//! providers: `Provider::models()` may return a local/static array, and
//! best-effort compatible discovery falls back rather than proving auth. The
//! first genuinely authenticated operation on a connection is a real inference
//! request, so this is where credential verification actually happens.
//!
//! Design constraints:
//!
//! - **Typed classification only.** Outcomes come from
//!   [`ProviderError::error_class`], never from provider-specific string
//!   matching or response-body sniffing.
//! - **Transitions are monotonic in safety.** An inconclusive or transient
//!   outcome writes nothing, so a network blip can never erase a credential
//!   verdict.
//! - **No credentials.** Nothing here touches secret material; only the
//!   connection id and its revision identify the target row.

use std::sync::Arc;

use crate::error::AppError;
use codegg_providers::ProviderError;

/// What a completed inference attempt established about the credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceCredentialOutcome {
    /// The provider accepted the credential: an authenticated request
    /// completed. Establishes `verified`.
    Authenticated,
    /// The provider rejected the credential (typed 401/403). Establishes
    /// `authentication_failed`.
    AuthenticationFailed,
    /// The attempt proves nothing about the credential: transport failure,
    /// timeout, cancellation, rate limit, context overflow, or any other
    /// non-auth error. Writes nothing.
    Inconclusive,
}

impl InferenceCredentialOutcome {
    /// Classify a terminal turn error. Only the typed authentication class
    /// produces a credential verdict.
    pub fn from_error(error: &AppError) -> Self {
        match error {
            AppError::Provider(provider_error) => Self::from_provider_error(provider_error),
            // Non-provider failures (cancellation, tool errors, local IO) say
            // nothing about a remote credential.
            _ => Self::Inconclusive,
        }
    }

    /// Same rule for a bare provider error.
    pub fn from_provider_error(error: &ProviderError) -> Self {
        if error.error_class() == "auth" {
            Self::AuthenticationFailed
        } else {
            Self::Inconclusive
        }
    }
}

/// Receives the terminal credential implication of one inference turn.
///
/// Implementations must be cheap and non-blocking: this is called on the turn
/// hot path.
pub trait ProviderCredentialObserver: Send + Sync {
    fn observe(&self, outcome: InferenceCredentialOutcome);
}

/// Convenience alias for the optional slot carried by the agent loop.
pub type SharedProviderCredentialObserver = Arc<dyn ProviderCredentialObserver>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_typed_auth_errors_produce_a_credential_verdict() {
        assert_eq!(
            InferenceCredentialOutcome::from_provider_error(&ProviderError::Auth(
                "invalid api key".to_string()
            )),
            InferenceCredentialOutcome::AuthenticationFailed
        );
        // A 401 surfaced as a generic API error is still an auth rejection.
        assert_eq!(
            InferenceCredentialOutcome::from_provider_error(&ProviderError::from_http_status(
                401,
                "unauthorized"
            )),
            InferenceCredentialOutcome::AuthenticationFailed
        );
    }

    #[test]
    fn transient_failures_are_inconclusive() {
        for error in [
            ProviderError::Timeout("stalled".to_string()),
            ProviderError::RateLimit,
            ProviderError::from_http_status(500, "server error"),
            ProviderError::from_http_status(429, "rate limited"),
            ProviderError::Transport {
                kind: "dns".to_string(),
            },
            ProviderError::CircuitOpen("open".to_string()),
        ] {
            assert_eq!(
                InferenceCredentialOutcome::from_provider_error(&error),
                InferenceCredentialOutcome::Inconclusive,
                "{error} must not be read as an invalid credential"
            );
        }
    }

    #[test]
    fn non_provider_errors_are_inconclusive() {
        assert_eq!(
            InferenceCredentialOutcome::from_error(&AppError::Other(anyhow::anyhow!("cancelled"))),
            InferenceCredentialOutcome::Inconclusive
        );
    }
}
