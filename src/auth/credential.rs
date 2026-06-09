//! Resolved credential type used by providers at request time.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Discriminator for the kind of credential held by a [`Credential`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    /// Static API key. Sent as `Bearer {secret}` for OpenAI-compatible
    /// providers and as `x-api-key: {secret}` for Anthropic-style providers.
    ApiKey,
    /// Short-lived bearer / OAuth-style access token.
    BearerToken,
}

/// A resolved credential ready to be turned into an HTTP header.
///
/// Construct via [`crate::auth::resolver::AuthResolver`] or the
/// [`Credential::api_key`] / [`Credential::bearer`] helpers.
#[derive(Debug, Clone)]
pub struct Credential {
    pub kind: CredentialKind,
    pub secret: String,
    pub expires_at: Option<DateTime<Utc>>,
}

impl Credential {
    /// Construct an API-key credential. `expires_at` is `None` for static
    /// API keys.
    pub fn api_key(secret: impl Into<String>) -> Self {
        Self {
            kind: CredentialKind::ApiKey,
            secret: secret.into(),
            expires_at: None,
        }
    }

    /// Construct a bearer-token credential. `expires_at` is meaningful for
    /// short-lived tokens and may be used by future refresh logic.
    pub fn bearer(secret: impl Into<String>, expires_at: Option<DateTime<Utc>>) -> Self {
        Self {
            kind: CredentialKind::BearerToken,
            secret: secret.into(),
            expires_at,
        }
    }

    /// Build the value for an `Authorization: ...` header.
    pub fn authorization_header_value(&self) -> String {
        match self.kind {
            CredentialKind::ApiKey | CredentialKind::BearerToken => {
                format!("Bearer {}", self.secret)
            }
        }
    }
}

/// Render `secret` as a fixed mask. Never returns prefix or suffix of the
/// input. The rendered length is bounded to keep the UI predictable.
pub fn mask_secret(secret: &str) -> String {
    let mask_char = '\u{2022}'; // bullet
    let max_len = 16;
    let rendered = mask_char.to_string().repeat(max_len);
    if secret.is_empty() {
        String::new()
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_credential_builds_bearer_header() {
        let c = Credential::api_key("sk-test-1234");
        assert_eq!(c.authorization_header_value(), "Bearer sk-test-1234");
        assert_eq!(c.kind, CredentialKind::ApiKey);
        assert!(c.expires_at.is_none());
    }

    #[test]
    fn bearer_credential_builds_bearer_header() {
        let c = Credential::bearer("tok-abc", None);
        assert_eq!(c.authorization_header_value(), "Bearer tok-abc");
    }

    #[test]
    fn mask_secret_never_returns_raw_or_substring() {
        let secret = "sk-verylongsecretvalue-1234567890";
        let masked = mask_secret(secret);
        assert!(!masked.contains(secret));
        assert!(!masked.contains("sk-"));
        assert!(!masked.contains("7890"));
        assert!(!masked.is_empty());
    }

    #[test]
    fn mask_secret_for_empty_returns_empty() {
        assert_eq!(mask_secret(""), "");
    }

    #[test]
    fn mask_secret_for_short_input_is_still_masked() {
        let masked = mask_secret("abc");
        assert!(!masked.contains('a'));
        assert!(!masked.contains('b'));
        assert!(!masked.contains('c'));
    }
}
