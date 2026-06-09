//! CLI commands for managing the user-level credential store.
//!
//! Provides:
//! - `auth status` — list stored credentials (no plaintext, no ciphertext, no fingerprint).
//! - `auth set-key <provider>` — store an API key for a provider.
//! - `auth logout <provider>` — remove stored credentials for a provider.
//!
//! The CLI is intentionally minimal. Interactive `/connect`-style flows in
//! the TUI can build on the same `CredentialStore` API for richer
//! behavior.
//!
//! ## Identifier validation
//!
//! Provider and account ids must be non-empty and contain only
//! conservative characters: `[A-Za-z0-9_-]`. Account ids additionally
//! allow `*` for `logout` so callers can wipe every account for a
//! provider in one call. Validation is enforced up-front to keep log
//! and error messages secret-free and to prevent store-file corruption
//! from arbitrary user input.

use std::io::{self, Read};

use crate::auth::credential::CredentialKind;
use crate::auth::store::CredentialStore;
use crate::auth::AuthError;
use crate::error::AppError;

const VALID_ID_CHARS: &str = "provider/account id must contain only [A-Za-z0-9_-] characters";

fn validate_id(id: &str, label: &str) -> Result<(), AppError> {
    if id.is_empty() {
        return Err(AppError::Config(crate::error::ConfigError::Invalid(
            format!("{label} id must not be empty"),
        )));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(AppError::Config(crate::error::ConfigError::Invalid(
            format!("{label} {VALID_ID_CHARS}"),
        )));
    }
    Ok(())
}

fn validate_provider_id(provider_id: &str) -> Result<(), AppError> {
    validate_id(provider_id, "provider")
}

fn validate_account_id(account_id: &str) -> Result<(), AppError> {
    validate_id(account_id, "account")
}

#[derive(Debug, Clone)]
pub struct AuthCli {
    /// Override the credential-store path. When `None`, the
    /// [`CredentialStore::at_default_location`] path is used.
    pub store_path: Option<std::path::PathBuf>,
}

impl Default for AuthCli {
    fn default() -> Self {
        Self::new()
    }
}

impl AuthCli {
    pub fn new() -> Self {
        Self { store_path: None }
    }

    pub fn with_store_path(mut self, path: std::path::PathBuf) -> Self {
        self.store_path = Some(path);
        self
    }

    fn open_store(&self) -> Result<CredentialStore, AppError> {
        let store = match &self.store_path {
            Some(p) => CredentialStore::at_path(p.clone()),
            None => CredentialStore::at_default_location(),
        }
        .map_err(|e| {
            AppError::Config(crate::error::ConfigError::Invalid(format!(
                "could not open credential store: {e}"
            )))
        })?;
        Ok(store)
    }

    /// List stored credentials (metadata only — no plaintext, no
    /// ciphertext, no secret-derived fingerprint).
    pub fn status(&self) -> Result<(), AppError> {
        let store = self.open_store()?;
        let records = store.list();
        if records.is_empty() {
            println!("No credentials stored.");
            return Ok(());
        }
        println!("Stored credentials ({}):", records.len());
        for rec in &records {
            let account = rec.account_id.as_deref().unwrap_or("(default)");
            let kind = match rec.kind {
                CredentialKind::ApiKey => "api_key",
                CredentialKind::BearerToken => "bearer",
            };
            let expires = rec
                .expires_at
                .map(|t| t.to_rfc3339())
                .unwrap_or_else(|| "never".to_string());
            println!(
                "  - {} [{}] account={} expires={} scopes={:?}",
                rec.provider_id, kind, account, expires, rec.scopes
            );
        }
        Ok(())
    }

    /// Store a plaintext API key for the given provider.
    ///
    /// `key` is passed in directly so callers can decide whether to read
    /// from stdin, a hidden input prompt, or an environment variable.
    /// The key is never echoed to stdout/stderr.
    pub fn set_key(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        key: &str,
    ) -> Result<(), AppError> {
        validate_provider_id(provider_id)?;
        if let Some(acct) = account_id {
            validate_account_id(acct)?;
        }
        if key.is_empty() {
            return Err(AppError::Config(crate::error::ConfigError::Invalid(
                "key must not be empty".to_string(),
            )));
        }
        let store = self.open_store()?;
        store
            .put(
                provider_id,
                account_id,
                CredentialKind::ApiKey,
                key,
                None,
                Vec::new(),
            )
            .map_err(auth_error_to_app_error)?;
        // Avoid echoing any key material; use a generic confirmation.
        let account_note = account_id
            .map(|a| format!(" (account={a})"))
            .unwrap_or_default();
        println!("Stored API key for provider '{provider_id}'{account_note}.");
        Ok(())
    }

    /// Remove stored credentials for the given provider. When
    /// `account_id` is `None`, the default-account record is removed.
    /// Pass `Some("*")` to remove all records for the provider.
    pub fn logout(&self, provider_id: &str, account_id: Option<&str>) -> Result<(), AppError> {
        validate_provider_id(provider_id)?;
        if let Some(acct) = account_id {
            // The wildcard `*` is a documented logout-only escape hatch.
            if acct != "*" {
                validate_account_id(acct)?;
            }
        }
        let store = self.open_store()?;
        let removed = store
            .remove(provider_id, account_id)
            .map_err(auth_error_to_app_error)?;
        if removed {
            let account_note = account_id
                .map(|a| format!(" (account={a})"))
                .unwrap_or_default();
            println!("Removed credentials for provider '{provider_id}'{account_note}.");
        } else {
            println!(
                "No stored credentials for provider '{}' (account={}).",
                provider_id,
                account_id.unwrap_or("(default)")
            );
        }
        Ok(())
    }
}

fn auth_error_to_app_error(e: AuthError) -> AppError {
    match e {
        AuthError::MasterKeyMissing => AppError::Config(crate::error::ConfigError::Invalid(
            "no master key configured; set CODEGG_MASTER_KEY to store new credentials".to_string(),
        )),
        other => AppError::Config(crate::error::ConfigError::Invalid(format!(
            "credential store error: {other}"
        ))),
    }
}

/// Read a key from stdin (or an env-supplied override).
///
/// Reads the entire stdin payload, trims trailing newlines, and returns
/// the result. This is the default path when no hidden-input crate is
/// wired in. Callers that want a `readline`-style UX can replace this
/// without changing the public API of [`AuthCli::set_key`].
pub fn read_key_from_stdin() -> Result<String, AppError> {
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| AppError::Config(crate::error::ConfigError::Invalid(e.to_string())))?;
    Ok(buf.trim_end_matches(['\n', '\r']).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_cli() -> (tempfile::TempDir, AuthCli) {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let cli = AuthCli::new().with_store_path(tmp.path().join("credentials.json"));
        (tmp, cli)
    }

    #[test]
    fn status_reports_no_records_on_empty_store() {
        let _guard = crate::auth::test_support::lock_env();
        let (_tmp, cli) = make_cli();
        cli.status().expect("status should succeed on empty store");
    }

    #[test]
    fn set_key_then_status_reports_record_without_plaintext() {
        let _guard = crate::auth::test_support::lock_env();
        let prev = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "cli-test-master-key");
        let (_tmp, cli) = make_cli();
        cli.set_key("openai", None, "sk-secret").expect("set_key");
        let store = cli.open_store().expect("open");
        let records = store.list();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].provider_id, "openai");
        assert_eq!(records[0].kind, CredentialKind::ApiKey);
        if let Some(v) = prev {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
    }

    #[test]
    fn set_key_without_master_key_returns_error() {
        let _guard = crate::auth::test_support::lock_env();
        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        let prev_enc = std::env::var("CODEGG_ENCRYPTION_KEY").ok();
        let prev_opencode = std::env::var("OPENCODE_ENCRYPTION_KEY").ok();
        std::env::remove_var("CODEGG_MASTER_KEY");
        std::env::remove_var("CODEGG_ENCRYPTION_KEY");
        std::env::remove_var("OPENCODE_ENCRYPTION_KEY");
        let (_tmp, cli) = make_cli();
        let err = cli.set_key("openai", None, "sk-secret").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("master key") || msg.contains("MasterKey"),
            "expected master-key error, got: {msg}"
        );
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
        if let Some(v) = prev_enc {
            std::env::set_var("CODEGG_ENCRYPTION_KEY", v);
        }
        if let Some(v) = prev_opencode {
            std::env::set_var("OPENCODE_ENCRYPTION_KEY", v);
        }
    }

    #[test]
    fn logout_removes_record() {
        let _guard = crate::auth::test_support::lock_env();
        let prev = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "cli-logout-test-master-key");
        let (_tmp, cli) = make_cli();
        cli.set_key("xai", None, "sk-test").expect("set_key");
        cli.logout("xai", None).expect("logout");
        let store = cli.open_store().expect("open");
        assert!(store.list().is_empty());
        if let Some(v) = prev {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
    }

    #[test]
    fn set_key_rejects_invalid_provider_id() {
        let (_tmp, cli) = make_cli();
        let err = cli.set_key("open ai", None, "sk-test").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("provider"),
            "expected provider error, got: {msg}"
        );
    }

    #[test]
    fn set_key_rejects_empty_key() {
        let (_tmp, cli) = make_cli();
        let err = cli.set_key("openai", None, "").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("key"), "expected key error, got: {msg}");
    }

    #[test]
    fn set_key_rejects_invalid_account_id() {
        let (_tmp, cli) = make_cli();
        let err = cli
            .set_key("openai", Some("work account"), "sk-test")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("account"),
            "expected account error, got: {msg}"
        );
    }

    #[test]
    fn logout_wildcard_is_accepted() {
        let _guard = crate::auth::test_support::lock_env();
        let prev = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "cli-wildcard-master-key");
        let (_tmp, cli) = make_cli();
        cli.set_key("openai", Some("work"), "sk-test")
            .expect("set_key work");
        cli.set_key("openai", Some("home"), "sk-test")
            .expect("set_key home");
        cli.logout("openai", Some("*")).expect("wildcard logout");
        let store = cli.open_store().expect("open");
        assert!(store.list().is_empty());
        if let Some(v) = prev {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
    }
}
