//! External-command credential provider.
//!
//! Some providers document an officially-supported CLI for issuing
//! short-lived credentials. The `ExternalCommandProvider` is the typed
//! home for that future path. The synchronous resolution path is
//! intentionally disabled: the original `std::process::Command`-based
//! implementation did not enforce its timeout, which would let a
//! hanging command stall provider registration indefinitely.
//!
//! [`AuthResolver::resolve`](crate::auth::resolver::AuthResolver::resolve)
//! returns
//! [`AuthError::Unsupported("ExternalCommand")`](crate::auth::error::AuthError::Unsupported)
//! for `AuthConfig::ExternalCommand`. This module's
//! [`ExternalCommandProvider::fetch`] entry point mirrors that policy:
//! it validates the command is non-empty and otherwise returns the
//! same `Unsupported` error so no safe path can accidentally shell
//! out. When async timeout plumbing is in place (using
//! `tokio::process::Command` with `tokio::time::timeout`), this arm can
//! be re-enabled.

use crate::auth::AuthError;
use crate::auth::Credential;

#[derive(Debug, Clone)]
pub struct ExternalCredential {
    pub command: String,
    pub args: Vec<String>,
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ExternalCommandProvider;

impl ExternalCommandProvider {
    pub fn new() -> Self {
        Self
    }

    /// Resolve a credential from an external command.
    ///
    /// The current implementation always returns
    /// [`AuthError::Unsupported`] for a non-empty command. An empty
    /// command is rejected up-front with
    /// [`AuthError::Invalid`]. The synchronous shell-out path is
    /// disabled because it did not enforce its timeout and could
    /// otherwise hang provider registration indefinitely. The async
    /// re-implementation is tracked as a follow-up.
    pub fn fetch(&self, cred: &ExternalCredential) -> Result<Credential, AuthError> {
        if cred.command.trim().is_empty() {
            return Err(AuthError::Invalid("external command is empty".to_string()));
        }
        Err(AuthError::Unsupported(
            "ExternalCommand requires async timeout plumbing".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_rejects_empty_command() {
        let provider = ExternalCommandProvider::new();
        let cred = ExternalCredential {
            command: "   ".to_string(),
            args: vec![],
            timeout_ms: None,
        };
        let err = provider.fetch(&cred).unwrap_err();
        match err {
            AuthError::Invalid(_) => {}
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn fetch_returns_unsupported_for_non_empty_command() {
        // A non-empty command must not execute. The fetch path is
        // intentionally disabled until async timeout plumbing lands.
        let provider = ExternalCommandProvider::new();
        let cred = ExternalCredential {
            command: "true".to_string(),
            args: vec![],
            timeout_ms: Some(1_000),
        };
        let err = provider.fetch(&cred).unwrap_err();
        match err {
            AuthError::Unsupported(reason) => {
                assert!(
                    reason.contains("ExternalCommand") || reason.contains("async timeout plumbing"),
                    "expected unsupported reason mentioning ExternalCommand/async, got: {reason}"
                );
            }
            other => panic!("expected Unsupported, got {other:?}"),
        }
    }
}
