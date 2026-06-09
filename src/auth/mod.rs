//! Auth and credential resolution.
//!
//! Core auth types (AuthConfig, Credential, CredentialKind, CredentialStore,
//! AuthResolver, etc.) are now defined in the `codegg-providers` crate and
//! re-exported here for backward compatibility.
//!
//! This module retains CLI-specific, external-command, and OAuth scaffolding
//! that is not part of the provider crate.

pub mod cli;
pub mod external;
pub mod oauth;

pub use cli::AuthCli;
pub use codegg_providers::auth_types::{
    mask_secret, AuthConfig, AuthError, AuthResolver, Credential, CredentialKind, CredentialStore,
    ExternalCommandProvider, ExternalCredential, ResolvedAuth, ResolvedAuthSource, ResolverContext,
    StoredCredentialRecord,
};

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
