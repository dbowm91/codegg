//! Auth and credential resolution.
//!
//! This module is the central seam where providers obtain credentials. It owns
//! typed [`AuthConfig`] (the configuration shape), [`Credential`] (the resolved
//! secret + metadata used to build an `Authorization` header), the
//! [`AuthResolver`] (which performs env → config → store priority), a
//! user-level encrypted [`credential_store::CredentialStore`], an
//! `ExternalCommandProvider` that shells out to an official CLI, and OAuth
//! scaffolding (typed but unimplemented in this pass).
//!
//! Providers should not log secret material. The [`mask_secret`] helper
//! returns a fixed-length mask that never exposes prefix or suffix of a key.

pub mod credential;
pub mod error;
pub mod external;
pub mod oauth;
pub mod resolver;
pub mod store;

pub use credential::{mask_secret, Credential, CredentialKind};
pub use error::AuthError;
pub use external::{ExternalCommandProvider, ExternalCredential};
pub use resolver::{AuthResolver, ResolvedAuth, ResolverContext};
pub use store::{CredentialStore, StoredCredentialRecord};

/// Test-only utilities shared across modules. The exported
/// [`test_support::env_lock`] is a single, cross-module mutex that
/// serializes tests mutating `CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY`
/// / `OPENCODE_ENCRYPTION_KEY` / `OPENAI_API_KEY`. Production code should
/// never observe this module.
#[doc(hidden)]
pub mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub fn env_lock() -> &'static Mutex<()> {
        &ENV_LOCK
    }

    /// Acquire the test env lock. The guard should be held for the entire
    /// span where the test flips master-key related env vars.
    pub fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(|e| e.into_inner())
    }
}

use serde::{Deserialize, Serialize};

/// Configuration-side auth descriptor. This is the shape that lives in
/// `ProviderConfig::auth` and lets providers express richer auth modes than a
/// single static API-key string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    /// API key from environment, inline value, or encrypted config value.
    ApiKey {
        /// Optional override for the env var name (defaults to
        /// `{PROVIDER}_API_KEY` for backward compatibility).
        env: Option<String>,
        /// Optional explicit API key value. Prefer env vars or the credential
        /// store over this field.
        value: Option<String>,
        /// Optional pre-encrypted value (see `crate::config::encryption`).
        encrypted_value: Option<String>,
    },
    /// Reference to a credential stored in the user-level credential store.
    Stored {
        /// Optional account id, used when multiple accounts exist for one
        /// provider.
        account_id: Option<String>,
    },
    /// External command that returns a credential on stdout (e.g. an
    /// officially-supported CLI that brokers access to a provider).
    ExternalCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        timeout_ms: Option<u64>,
    },
    /// OAuth device-code / PKCE flow. Reserved for providers that publish a
    /// stable, public contract. The first pass parses this variant but
    /// resolution returns [`AuthError::Unsupported`].
    OAuthDevice {
        client_id: String,
        #[serde(default)]
        scopes: Vec<String>,
        auth_url: String,
        token_url: String,
    },
    /// Explicitly no auth configured. Useful as a marker in defaults.
    None,
}

impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig::ApiKey {
            env: None,
            value: None,
            encrypted_value: None,
        }
    }
}

impl AuthConfig {
    /// Returns true if this variant represents an API-key-shaped credential
    /// (the path that the current providers all use).
    pub fn is_api_key(&self) -> bool {
        matches!(self, AuthConfig::ApiKey { .. })
    }

    /// Returns true if this variant is currently resolvable by the
    /// first-pass [`AuthResolver`].
    pub fn is_supported(&self) -> bool {
        matches!(
            self,
            AuthConfig::ApiKey { .. }
                | AuthConfig::Stored { .. }
                | AuthConfig::ExternalCommand { .. }
        )
    }
}
