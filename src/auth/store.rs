//! User-level encrypted credential store.
//!
//! Secrets stored here live in `~/.config/codegg/credentials.json` (or the
//! platform equivalent). The file is a JSON object whose value is a list
//! of `StoredCredentialRecord`s. Each `encrypted_secret` is encrypted with
//! the existing `CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` master key
//! using `crypto::encrypt_to_string`.
//!
//! Reading plain API keys from env/config still works without a master key.
//! Storing new credentials requires a master key and returns
//! `AuthError::MasterKeyMissing` if none is configured.

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::credential::CredentialKind;
use crate::auth::error::AuthError;
use crate::config::encryption::get_master_key;
use crate::config::paths::global_config_path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredCredentialRecord {
    pub provider_id: String,
    pub account_id: Option<String>,
    pub kind: CredentialKind,
    pub encrypted_secret: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    records: Vec<StoredCredentialRecord>,
}

/// On-disk, encrypted, thread-safe credential store. Backed by a JSON file
/// at `<config_dir>/codegg/credentials.json` (or a custom path used in
/// tests). Reads and writes are serialized through a `Mutex`.
#[derive(Debug)]
pub struct CredentialStore {
    path: PathBuf,
    records: Mutex<Vec<StoredCredentialRecord>>,
}

impl CredentialStore {
    /// Create a store at the default user-config location.
    pub fn at_default_location() -> Result<Self, AuthError> {
        let base = global_config_path()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .or_else(|| dirs::config_dir().map(|d| d.join("codegg")))
            .ok_or_else(|| {
                AuthError::Invalid("could not determine user config directory".to_string())
            })?;
        let path = base.join("credentials.json");
        Self::at_path(path)
    }

    /// Create a store at an explicit path. The path's parent directory is
    /// created if missing.
    pub fn at_path(path: PathBuf) -> Result<Self, AuthError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let records = load_from_disk(&path).unwrap_or_default();
        Ok(Self {
            path,
            records: Mutex::new(records),
        })
    }

    /// Path to the on-disk file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Store or replace a credential. Requires a master key. When replacing,
    /// the previous `created_at` is preserved and `updated_at` is set to
    /// `now`.
    pub fn put(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        kind: CredentialKind,
        secret: &str,
        expires_at: Option<DateTime<Utc>>,
        scopes: Vec<String>,
    ) -> Result<(), AuthError> {
        let master = get_master_key().ok_or(AuthError::MasterKeyMissing)?;
        let encrypted = crate::crypto::encrypt_to_string(secret, &master)?;
        let now = Utc::now();
        let mut records = self.records.lock().expect("poisoned");
        let existing = records
            .iter_mut()
            .find(|r| r.provider_id == provider_id && r.account_id.as_deref() == account_id);
        if let Some(rec) = existing {
            rec.kind = kind;
            rec.encrypted_secret = encrypted;
            rec.expires_at = expires_at;
            rec.scopes = scopes;
            rec.updated_at = now;
        } else {
            records.push(StoredCredentialRecord {
                provider_id: provider_id.to_string(),
                account_id: account_id.map(|s| s.to_string()),
                kind,
                encrypted_secret: encrypted,
                expires_at,
                scopes,
                created_at: now,
                updated_at: now,
            });
        }
        write_to_disk(&self.path, &records)
    }

    /// Remove a credential by provider + account. Returns true if a record
    /// was removed. If `account_id` is `None`, removes the record with
    /// `account_id == None` for that provider. Pass `Some("*")` to remove
    /// all records for the provider.
    pub fn remove(&self, provider_id: &str, account_id: Option<&str>) -> Result<bool, AuthError> {
        let mut records = self.records.lock().expect("poisoned");
        let original_len = records.len();
        if account_id == Some("*") {
            records.retain(|r| r.provider_id != provider_id);
        } else {
            records.retain(|r| {
                !(r.provider_id == provider_id && r.account_id.as_deref() == account_id)
            });
        }
        let removed = records.len() != original_len;
        if removed {
            write_to_disk(&self.path, &records)?;
        }
        Ok(removed)
    }

    /// List records (metadata only — no plaintext).
    pub fn list(&self) -> Vec<StoredCredentialRecord> {
        self.records.lock().expect("poisoned").clone()
    }

    /// Read a plaintext secret for the given provider/account, filtered by
    /// a predicate over the public record (e.g. by `kind`). Returns
    /// `Ok(None)` when no matching record exists.
    pub fn get_plaintext(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        mut predicate: impl FnMut(&StoredCredentialRecord) -> bool,
    ) -> Result<Option<String>, AuthError> {
        let master = match get_master_key() {
            Some(m) => m,
            None => {
                // Reading the store without a master key is allowed for
                // operations that don't actually decrypt, but the
                // `get_plaintext` API needs the master. Callers that
                // don't need plaintext (i.e. just metadata) should use
                // `list()`.
                return Ok(None);
            }
        };
        let records = self.records.lock().expect("poisoned");
        let rec = records
            .iter()
            .find(|r| {
                r.provider_id == provider_id
                    && r.account_id.as_deref() == account_id
                    && predicate(r)
            })
            .cloned();
        drop(records);
        let Some(rec) = rec else {
            return Ok(None);
        };
        let plain = crate::crypto::decrypt_from_string(&rec.encrypted_secret, &master)?;
        Ok(Some(plain))
    }
}

fn load_from_disk(path: &Path) -> Option<Vec<StoredCredentialRecord>> {
    let text = fs::read_to_string(path).ok()?;
    let parsed: StoreFile = serde_json::from_str(&text).ok()?;
    Some(parsed.records)
}

fn write_to_disk(path: &Path, records: &[StoredCredentialRecord]) -> Result<(), AuthError> {
    let file = StoreFile {
        version: 1,
        records: records.to_vec(),
    };
    let mut body = serde_json::to_string_pretty(&file)?;
    body.push('\n');
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Atomic write: write to a sibling temp file, then rename.
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path)?;
    // Restrict permissions on Unix so only the owner can read the file.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
    }
    Ok(())
}

/// Diagnostics: returns counts grouped by provider.
pub fn summarize(records: &[StoredCredentialRecord]) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for r in records {
        *out.entry(r.provider_id.clone()).or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> (tempfile::TempDir, CredentialStore) {
        let tmp = tempfile::tempdir().expect("tmpdir");
        let path = tmp.path().join("credentials.json");
        let store = CredentialStore::at_path(path).expect("store");
        (tmp, store)
    }

    #[test]
    fn put_with_master_key_succeeds_and_without_returns_error() {
        let _guard = crate::auth::test_support::lock_env();

        let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
        let prev_enc = std::env::var("CODEGG_ENCRYPTION_KEY").ok();
        let prev_opencode = std::env::var("OPENCODE_ENCRYPTION_KEY").ok();

        // Without master key: should error.
        std::env::remove_var("CODEGG_MASTER_KEY");
        std::env::remove_var("CODEGG_ENCRYPTION_KEY");
        std::env::remove_var("OPENCODE_ENCRYPTION_KEY");
        {
            let (_tmp, store) = make_store();
            let err = store
                .put(
                    "openai",
                    None,
                    CredentialKind::ApiKey,
                    "sk-test",
                    None,
                    vec![],
                )
                .unwrap_err();
            assert!(matches!(err, AuthError::MasterKeyMissing));
        }

        // With master key: should succeed.
        std::env::set_var("CODEGG_MASTER_KEY", "put-test-master");
        {
            let (_tmp, store) = make_store();
            store
                .put(
                    "openai",
                    None,
                    CredentialKind::ApiKey,
                    "sk-test",
                    None,
                    vec![],
                )
                .expect("put with master key should succeed");
        }

        // Restore prior state.
        if let Some(v) = prev_master {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        } else {
            std::env::remove_var("CODEGG_MASTER_KEY");
        }
        if let Some(v) = prev_enc {
            std::env::set_var("CODEGG_ENCRYPTION_KEY", v);
        }
        if let Some(v) = prev_opencode {
            std::env::set_var("OPENCODE_ENCRYPTION_KEY", v);
        }
    }

    #[test]
    fn round_trip_with_master_key() {
        let _guard = crate::auth::test_support::lock_env();
        let prev = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "unit-test-master");
        let (_tmp, store) = make_store();
        store
            .put(
                "openai",
                Some("acct-1"),
                CredentialKind::ApiKey,
                "sk-secret",
                None,
                vec!["read".to_string()],
            )
            .expect("put");
        let plain = store
            .get_plaintext("openai", Some("acct-1"), |_| true)
            .expect("get")
            .expect("some");
        assert_eq!(plain, "sk-secret");
        if let Some(v) = prev {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
    }

    #[test]
    fn reloading_from_disk_preserves_records() {
        let _guard = crate::auth::test_support::lock_env();
        let prev = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "unit-test-master-2");
        let tmp = tempfile::tempdir().expect("tmpdir");
        let path = tmp.path().join("credentials.json");
        {
            let store = CredentialStore::at_path(path.clone()).expect("store");
            store
                .put(
                    "anthropic",
                    None,
                    CredentialKind::ApiKey,
                    "sk-ant",
                    None,
                    vec![],
                )
                .expect("put");
        }
        let store2 = CredentialStore::at_path(path).expect("reload");
        let plain = store2
            .get_plaintext("anthropic", None, |_| true)
            .expect("get")
            .expect("some");
        assert_eq!(plain, "sk-ant");
        if let Some(v) = prev {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
    }

    #[test]
    fn remove_clears_record() {
        let _guard = crate::auth::test_support::lock_env();
        let prev = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "unit-test-master-3");
        let (_tmp, store) = make_store();
        store
            .put("xai", None, CredentialKind::ApiKey, "k1", None, vec![])
            .expect("put");
        let removed = store.remove("xai", None).expect("remove");
        assert!(removed);
        let got = store
            .get_plaintext("xai", None, |_| true)
            .expect("get")
            .is_none();
        assert!(got);
        if let Some(v) = prev {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
    }

    #[test]
    fn on_disk_file_is_not_plaintext() {
        let _guard = crate::auth::test_support::lock_env();
        let prev = std::env::var("CODEGG_MASTER_KEY").ok();
        std::env::set_var("CODEGG_MASTER_KEY", "unit-test-master-4");
        let tmp = tempfile::tempdir().expect("tmpdir");
        let path = tmp.path().join("credentials.json");
        let store = CredentialStore::at_path(path.clone()).expect("store");
        let secret = "sk-should-never-appear-on-disk";
        store
            .put("openai", None, CredentialKind::ApiKey, secret, None, vec![])
            .expect("put");
        let raw = std::fs::read_to_string(&path).expect("read");
        assert!(
            !raw.contains(secret),
            "raw file must not contain plaintext secret"
        );
        if let Some(v) = prev {
            std::env::set_var("CODEGG_MASTER_KEY", v);
        }
    }
}
