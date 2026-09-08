//! Auth and credential types for provider authentication.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use thiserror::Error;

// --- AuthError ---

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("credential not found for provider '{0}'")]
    NotFound(String),

    #[error("credential expired for provider '{0}'")]
    Expired(String),

    #[error("no master key configured; set CODEGG_MASTER_KEY to store new credentials")]
    MasterKeyMissing,

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("auth mode '{0}' is recognized but not yet implemented in this build")]
    Unsupported(String),

    #[error("invalid auth configuration: {0}")]
    Invalid(String),

    #[error("external command '{command}' failed: {message}")]
    ExternalCommand { command: String, message: String },
}

impl From<crate::crypto::CryptoError> for AuthError {
    fn from(value: crate::crypto::CryptoError) -> Self {
        AuthError::Crypto(value.to_string())
    }
}

// --- CredentialKind ---

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CredentialKind {
    ApiKey,
    BearerToken,
}

// --- CredentialCapability ---
//
// Provider-registration capability describing which stored credential kinds a
// provider path can consume. Defined once in the auth owner so factories do
// not duplicate the classification per call site. See
// `crate::provider_core::credential_capability_for` for the built-in matrix.

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum CredentialCapability {
    /// Static API-key string only (e.g. `x-api-key` transports and all
    /// `String`-contract factories). Stored bearer records are rejected
    /// explicitly and never reinterpreted as API keys.
    #[default]
    ApiKeyOnly,
    /// Full `Credential` envelope; either an API key or a bearer token is
    /// accepted and the kind is preserved end-to-end.
    ApiKeyOrBearer,
}

impl CredentialCapability {
    pub fn accepts(&self, kind: CredentialKind) -> bool {
        match self {
            CredentialCapability::ApiKeyOnly => kind == CredentialKind::ApiKey,
            CredentialCapability::ApiKeyOrBearer => true,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            CredentialCapability::ApiKeyOnly => "api_key_only",
            CredentialCapability::ApiKeyOrBearer => "api_key_or_bearer",
        }
    }
}

/// Actionable diagnostic for an explicitly bound stored credential whose kind
/// the target provider path cannot consume. Contains no secret material.
pub fn incompatible_credential_message(
    provider_id: &str,
    kind: CredentialKind,
    capability: CredentialCapability,
) -> String {
    let kind_label = match kind {
        CredentialKind::ApiKey => "api-key",
        CredentialKind::BearerToken => "bearer",
    };
    format!(
        "stored {kind_label} credential for provider '{provider_id}' is not supported by this provider path (capability: {}); store an API-key credential for this provider/account",
        capability.as_str()
    )
}

// --- Credential ---

#[derive(Clone)]
pub struct Credential {
    pub kind: CredentialKind,
    pub secret: String,
    pub expires_at: Option<DateTime<Utc>>,
}

// Repo policy: never log secrets. The derived Debug would render the
// plaintext secret, so mask it explicitly.
impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Credential")
            .field("kind", &self.kind)
            .field("secret", &mask_secret(&self.secret))
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl Credential {
    pub fn api_key(secret: impl Into<String>) -> Self {
        Self {
            kind: CredentialKind::ApiKey,
            secret: secret.into(),
            expires_at: None,
        }
    }

    pub fn bearer(secret: impl Into<String>, expires_at: Option<DateTime<Utc>>) -> Self {
        Self {
            kind: CredentialKind::BearerToken,
            secret: secret.into(),
            expires_at,
        }
    }

    pub fn authorization_header_value(&self) -> String {
        match self.kind {
            CredentialKind::ApiKey | CredentialKind::BearerToken => {
                format!("Bearer {}", self.secret)
            }
        }
    }
}

pub fn mask_secret(secret: &str) -> String {
    let mask_char = '\u{2022}';
    let max_len = 16;
    let rendered = mask_char.to_string().repeat(max_len);
    if secret.is_empty() {
        String::new()
    } else {
        rendered
    }
}

// --- AuthConfig ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthConfig {
    ApiKey {
        env: Option<String>,
        value: Option<String>,
        encrypted_value: Option<String>,
    },
    Stored {
        account_id: Option<String>,
    },
    ExternalCommand {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        timeout_ms: Option<u64>,
    },
    OAuthDevice {
        client_id: String,
        #[serde(default)]
        scopes: Vec<String>,
        auth_url: String,
        token_url: String,
    },
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
    pub fn is_api_key(&self) -> bool {
        matches!(self, AuthConfig::ApiKey { .. })
    }

    pub fn is_supported(&self) -> bool {
        matches!(self, AuthConfig::ApiKey { .. } | AuthConfig::Stored { .. })
    }
}

// --- ExternalCommandProvider ---

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

    pub fn fetch(&self, cred: &ExternalCredential) -> Result<Credential, AuthError> {
        if cred.command.trim().is_empty() {
            return Err(AuthError::Invalid("external command is empty".to_string()));
        }
        Err(AuthError::Unsupported(
            "ExternalCommand requires async timeout plumbing".to_string(),
        ))
    }
}

// --- Resolver types ---

#[derive(Debug, Clone, Default)]
pub struct ResolverContext {
    pub provider_id: String,
    pub account_id: Option<String>,
    pub legacy_api_key: Option<String>,
    pub legacy_decrypted: Option<String>,
    pub store: Option<std::sync::Arc<CredentialStore>>,
    pub env_override: Option<String>,
    /// Accepted stored credential kinds for the target provider path.
    /// Defaults to `ApiKeyOnly` so direct resolver callers preserve the
    /// historical API-key-only store filtering unless they opt in.
    /// `resolve_provider_credential` sets this from the executable
    /// provider capability matrix.
    pub capability: CredentialCapability,
}

#[derive(Debug, Clone)]
pub struct ResolvedAuth {
    pub credential: Credential,
    pub source: ResolvedAuthSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedAuthSource {
    EnvExplicit,
    EnvConventional,
    InlineValue,
    EncryptedConfig,
    UserStore,
    LegacyApiKey,
    LegacyDecrypted,
    ExternalCommand,
}

impl ResolvedAuthSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolvedAuthSource::EnvExplicit => "env(explicit)",
            ResolvedAuthSource::EnvConventional => "env(conventional)",
            ResolvedAuthSource::InlineValue => "config(inline)",
            ResolvedAuthSource::EncryptedConfig => "config(encrypted)",
            ResolvedAuthSource::UserStore => "user_store",
            ResolvedAuthSource::LegacyApiKey => "legacy(api_key)",
            ResolvedAuthSource::LegacyDecrypted => "legacy(decrypted)",
            ResolvedAuthSource::ExternalCommand => "external_command",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AuthResolver {
    #[allow(dead_code)]
    external: ExternalCommandProvider,
}

impl AuthResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn resolve(
        &self,
        auth: Option<&AuthConfig>,
        ctx: &ResolverContext,
    ) -> Result<Option<ResolvedAuth>, AuthError> {
        if let Some(cfg) = auth {
            match cfg {
                AuthConfig::ApiKey {
                    env,
                    value,
                    encrypted_value,
                } => {
                    if let Some(env_name) = ctx.env_override.as_deref().or(env.as_deref()) {
                        if let Some(v) = read_env(env_name) {
                            return Ok(Some(resolved(
                                Credential::api_key(v),
                                ResolvedAuthSource::EnvExplicit,
                            )));
                        }
                    }
                    let conventional = conventional_env_for(&ctx.provider_id);
                    if let Some(v) = read_env(&conventional) {
                        return Ok(Some(resolved(
                            Credential::api_key(v),
                            ResolvedAuthSource::EnvConventional,
                        )));
                    }
                    if let Some(v) = value {
                        if !v.is_empty() {
                            return Ok(Some(resolved(
                                Credential::api_key(v.clone()),
                                ResolvedAuthSource::InlineValue,
                            )));
                        }
                    }
                    if let Some(enc) = encrypted_value {
                        if let Some(master) = codegg_config::encryption::get_master_key() {
                            match crate::crypto::decrypt_from_string(enc, &master) {
                                Ok(plain) => {
                                    return Ok(Some(resolved(
                                        Credential::api_key(plain),
                                        ResolvedAuthSource::EncryptedConfig,
                                    )));
                                }
                                Err(e) => {
                                    return Err(AuthError::Crypto(format!(
                                        "decrypt encrypted_value: {e}"
                                    )));
                                }
                            }
                        } else {
                            return Err(AuthError::MasterKeyMissing);
                        }
                    }
                }
                AuthConfig::Stored { account_id } => {
                    let store = ctx
                        .store
                        .as_ref()
                        .ok_or_else(|| AuthError::NotFound(ctx.provider_id.clone()))?;
                    let account = account_id.clone().or_else(|| ctx.account_id.clone());
                    let record = match store.find_record(&ctx.provider_id, account.as_deref()) {
                        Some(record) => record,
                        None => return Err(AuthError::NotFound(ctx.provider_id.clone())),
                    };
                    if is_expired(record.expires_at) {
                        return Err(AuthError::Expired(ctx.provider_id.clone()));
                    }
                    if !ctx.capability.accepts(record.kind) {
                        return Err(AuthError::Unsupported(incompatible_credential_message(
                            &ctx.provider_id,
                            record.kind,
                            ctx.capability,
                        )));
                    }
                    match store.get_credential(&ctx.provider_id, account.as_deref())? {
                        Some(credential) => {
                            if is_expired(credential.expires_at) {
                                return Err(AuthError::Expired(ctx.provider_id.clone()));
                            }
                            if credential.secret.is_empty() {
                                return Err(AuthError::NotFound(ctx.provider_id.clone()));
                            }
                            return Ok(Some(resolved(credential, ResolvedAuthSource::UserStore)));
                        }
                        None => {
                            // No master key to decrypt: preserve the historical
                            // Stored-miss contract rather than inventing a new
                            // fallback.
                            return Err(AuthError::NotFound(ctx.provider_id.clone()));
                        }
                    }
                }
                AuthConfig::ExternalCommand { .. } => {
                    return Err(AuthError::Unsupported("ExternalCommand".to_string()));
                }
                AuthConfig::OAuthDevice { .. } => {
                    return Err(AuthError::Unsupported("OAuthDevice".to_string()));
                }
                AuthConfig::None => return Ok(None),
            }
        }

        // No auth: try conventional env var, then legacy fields.
        if let Some(env_name) = ctx.env_override.as_deref() {
            if let Some(v) = read_env(env_name) {
                return Ok(Some(resolved(
                    Credential::api_key(v),
                    ResolvedAuthSource::EnvExplicit,
                )));
            }
        }
        let conventional = conventional_env_for(&ctx.provider_id);
        if let Some(v) = read_env(&conventional) {
            return Ok(Some(resolved(
                Credential::api_key(v),
                ResolvedAuthSource::EnvConventional,
            )));
        }
        if let Some(ref k) = ctx.legacy_api_key {
            if !k.is_empty() {
                return Ok(Some(resolved(
                    Credential::api_key(k.clone()),
                    ResolvedAuthSource::LegacyApiKey,
                )));
            }
        }
        if let Some(ref k) = ctx.legacy_decrypted {
            if !k.is_empty() {
                return Ok(Some(resolved(
                    Credential::api_key(k.clone()),
                    ResolvedAuthSource::LegacyDecrypted,
                )));
            }
        }
        if let Some(store) = ctx.store.as_ref() {
            if let Some(record) = store.find_record(&ctx.provider_id, ctx.account_id.as_deref()) {
                if is_expired(record.expires_at) {
                    return Err(AuthError::Expired(ctx.provider_id.clone()));
                }
                if !ctx.capability.accepts(record.kind) {
                    return Err(AuthError::Unsupported(incompatible_credential_message(
                        &ctx.provider_id,
                        record.kind,
                        ctx.capability,
                    )));
                }
                match store.get_credential(&ctx.provider_id, ctx.account_id.as_deref())? {
                    Some(credential) => {
                        if is_expired(credential.expires_at) {
                            return Err(AuthError::Expired(ctx.provider_id.clone()));
                        }
                        if !credential.secret.is_empty() {
                            return Ok(Some(resolved(credential, ResolvedAuthSource::UserStore)));
                        }
                    }
                    None => {
                        // No master key: fall through to Ok(None) so
                        // env/config paths still work without a key.
                    }
                }
            }
        }
        Ok(None)
    }
}

fn resolved(credential: Credential, source: ResolvedAuthSource) -> ResolvedAuth {
    ResolvedAuth { credential, source }
}

fn is_expired(expires_at: Option<DateTime<Utc>>) -> bool {
    expires_at.is_some_and(|expires_at| expires_at <= Utc::now())
}

fn read_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

fn conventional_env_for(provider_id: &str) -> String {
    let upper = provider_id.to_uppercase().replace('-', "_");
    format!("{upper}_API_KEY")
}

pub fn conventional_env_map() -> std::collections::HashMap<&'static str, &'static str> {
    let mut m = std::collections::HashMap::new();
    m.insert("anthropic", "ANTHROPIC_API_KEY");
    m.insert("openai", "OPENAI_API_KEY");
    m.insert("google", "GOOGLE_API_KEY");
    m.insert("openrouter", "OPENROUTER_API_KEY");
    m.insert("opencode_zen", "OPENCODE_ZEN_API_KEY");
    m.insert("mistral", "MISTRAL_API_KEY");
    m.insert("groq", "GROQ_API_KEY");
    m.insert("deepinfra", "DEEPINFRA_API_KEY");
    m.insert("cerebras", "CEREBRAS_API_KEY");
    m.insert("cohere", "COHERE_API_KEY");
    m.insert("together", "TOGETHERAI_API_KEY");
    m.insert("perplexity", "PERPLEXITY_API_KEY");
    m.insert("xai", "XAI_API_KEY");
    m.insert("venice", "VENICE_API_KEY");
    m.insert("minimax", "MINIMAX_API_KEY");
    m.insert("opencode_go", "OPENCODE_GO_API_KEY");
    m.insert("generalcompute", "GENERALCOMPUTE_API_KEY");
    m
}

// --- CredentialStore ---

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

#[derive(Debug)]
pub struct CredentialStore {
    path: PathBuf,
    records: Mutex<Vec<StoredCredentialRecord>>,
}

impl CredentialStore {
    pub fn at_default_location() -> Result<Self, AuthError> {
        let base = codegg_config::paths::global_config_path()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .or_else(|| dirs::config_dir().map(|d| d.join("codegg")))
            .ok_or_else(|| {
                AuthError::Invalid("could not determine user config directory".to_string())
            })?;
        let path = base.join("credentials.json");
        Self::at_path(path)
    }

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

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn put(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        kind: CredentialKind,
        secret: &str,
        expires_at: Option<DateTime<Utc>>,
        scopes: Vec<String>,
    ) -> Result<(), AuthError> {
        let master =
            codegg_config::encryption::get_master_key().ok_or(AuthError::MasterKeyMissing)?;
        let encrypted = crate::crypto::encrypt_to_string(secret, &master)?;
        let now = Utc::now();
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

    pub fn remove(&self, provider_id: &str, account_id: Option<&str>) -> Result<bool, AuthError> {
        let mut records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

    pub fn list(&self) -> Vec<StoredCredentialRecord> {
        self.records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub fn get_plaintext(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
        mut predicate: impl FnMut(&StoredCredentialRecord) -> bool,
    ) -> Result<Option<String>, AuthError> {
        let master = match codegg_config::encryption::get_master_key() {
            Some(m) => m,
            None => return Ok(None),
        };
        let records = self
            .records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
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

    /// Exact-match metadata lookup for one provider/account binding.
    ///
    /// The store enforces one record per `(provider_id, account_id)` binding
    /// (`put` replaces in place), so this lookup is deterministic and never
    /// guesses across accounts or kinds.
    pub fn find_record(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
    ) -> Option<StoredCredentialRecord> {
        self.records
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .find(|r| r.provider_id == provider_id && r.account_id.as_deref() == account_id)
            .cloned()
    }

    /// Decrypt one exact binding into a full [`Credential`], preserving
    /// [`CredentialKind`] and `expires_at`.
    ///
    /// Returns `Ok(None)` when no record exists or when no master key is
    /// configured (matching `get_plaintext` semantics so env/config paths
    /// still work without a key). Expiry and capability checks are the
    /// resolver's responsibility so it can return distinct typed errors.
    pub fn get_credential(
        &self,
        provider_id: &str,
        account_id: Option<&str>,
    ) -> Result<Option<Credential>, AuthError> {
        let rec = match self.find_record(provider_id, account_id) {
            Some(rec) => rec,
            None => return Ok(None),
        };
        let master = match codegg_config::encryption::get_master_key() {
            Some(m) => m,
            None => return Ok(None),
        };
        let plain = crate::crypto::decrypt_from_string(&rec.encrypted_secret, &master)?;
        Ok(Some(Credential {
            kind: rec.kind,
            secret: plain,
            expires_at: rec.expires_at,
        }))
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
    let tmp = path.with_file_name(format!(
        ".{}.tmp-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("credentials.json"),
        uuid::Uuid::new_v4()
    ));
    let write_result = (|| -> Result<(), AuthError> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut f = options.open(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all()?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = fs::remove_file(&tmp);
        return Err(error);
    }

    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error.into());
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

pub fn summarize(records: &[StoredCredentialRecord]) -> BTreeMap<String, usize> {
    let mut out: BTreeMap<String, usize> = BTreeMap::new();
    for r in records {
        *out.entry(r.provider_id.clone()).or_insert(0) += 1;
    }
    out
}

// --- Test support ---

#[doc(hidden)]
pub mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    pub fn env_lock() -> &'static Mutex<()> {
        &ENV_LOCK
    }

    pub fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn credential_debug_masks_secret() {
        let credential = Credential::api_key("sk-super-secret-value");
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("sk-super-secret-value"));
        assert!(rendered.contains("Credential"));
    }

    #[test]
    fn credential_store_recovers_from_poisoned_lock() {
        let path =
            std::env::temp_dir().join(format!("codegg-credentials-{}", uuid::Uuid::new_v4()));
        let store = CredentialStore::at_path(path.clone()).unwrap();
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = store.records.lock().unwrap();
            panic!("poison credential store lock");
        }));

        assert!(store.list().is_empty());
        let _ = std::fs::remove_file(path);
    }

    struct MasterGuard {
        prev_master: Option<String>,
        prev_enc: Option<String>,
        prev_opencode: Option<String>,
        _env: std::sync::MutexGuard<'static, ()>,
    }

    impl MasterGuard {
        fn with_master(master: &str) -> Self {
            let env = test_support::lock_env();
            let prev_master = std::env::var("CODEGG_MASTER_KEY").ok();
            let prev_enc = std::env::var("CODEGG_ENCRYPTION_KEY").ok();
            let prev_opencode = std::env::var("OPENCODE_ENCRYPTION_KEY").ok();
            std::env::set_var("CODEGG_MASTER_KEY", master);
            std::env::remove_var("CODEGG_ENCRYPTION_KEY");
            std::env::remove_var("OPENCODE_ENCRYPTION_KEY");
            Self {
                prev_master,
                prev_enc,
                prev_opencode,
                _env: env,
            }
        }
    }

    impl Drop for MasterGuard {
        fn drop(&mut self) {
            if let Some(v) = self.prev_master.take() {
                std::env::set_var("CODEGG_MASTER_KEY", v);
            } else {
                std::env::remove_var("CODEGG_MASTER_KEY");
            }
            if let Some(v) = self.prev_enc.take() {
                std::env::set_var("CODEGG_ENCRYPTION_KEY", v);
            } else {
                std::env::remove_var("CODEGG_ENCRYPTION_KEY");
            }
            if let Some(v) = self.prev_opencode.take() {
                std::env::set_var("OPENCODE_ENCRYPTION_KEY", v);
            } else {
                std::env::remove_var("OPENCODE_ENCRYPTION_KEY");
            }
        }
    }

    fn temp_store() -> (tempfile::TempDir, Arc<CredentialStore>) {
        let dir = tempfile::tempdir().expect("tmpdir");
        let store =
            Arc::new(CredentialStore::at_path(dir.path().join("credentials.json")).expect("store"));
        (dir, store)
    }

    fn stored_ctx(
        provider: &str,
        account: Option<&str>,
        store: &Arc<CredentialStore>,
        capability: CredentialCapability,
    ) -> ResolverContext {
        ResolverContext {
            provider_id: provider.to_string(),
            account_id: account.map(|s| s.to_string()),
            store: Some(store.clone()),
            capability,
            ..Default::default()
        }
    }

    #[test]
    fn bearer_debug_masks_secret() {
        let credential = Credential::bearer("bearer-sentinel-123", None);
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("bearer-sentinel-123"));
        assert!(rendered.contains("BearerToken"));
    }

    #[test]
    fn stored_api_key_resolves_under_both_capabilities() {
        let _master = MasterGuard::with_master("m010-api-key-both-caps");
        let (_dir, store) = temp_store();
        store
            .put(
                "cap_test",
                Some("acct"),
                CredentialKind::ApiKey,
                "stored-api-sentinel",
                None,
                vec![],
            )
            .expect("put");
        let resolver = AuthResolver::new();
        let auth = AuthConfig::Stored {
            account_id: Some("acct".to_string()),
        };
        for capability in [
            CredentialCapability::ApiKeyOnly,
            CredentialCapability::ApiKeyOrBearer,
        ] {
            let ctx = stored_ctx("cap_test", None, &store, capability);
            let resolved = resolver
                .resolve(Some(&auth), &ctx)
                .expect("ok")
                .expect("some");
            assert_eq!(resolved.credential.secret, "stored-api-sentinel");
            assert_eq!(resolved.credential.kind, CredentialKind::ApiKey);
            assert_eq!(resolved.source, ResolvedAuthSource::UserStore);
        }
    }

    #[test]
    fn stored_bearer_resolves_under_compatible_capability() {
        let _master = MasterGuard::with_master("m010-bearer-compatible");
        let (_dir, store) = temp_store();
        store
            .put(
                "cap_bearer",
                Some("acct"),
                CredentialKind::BearerToken,
                "bearer-sentinel-compat",
                None,
                vec![],
            )
            .expect("put");
        let resolver = AuthResolver::new();
        let auth = AuthConfig::Stored {
            account_id: Some("acct".to_string()),
        };
        let ctx = stored_ctx(
            "cap_bearer",
            None,
            &store,
            CredentialCapability::ApiKeyOrBearer,
        );
        let resolved = resolver
            .resolve(Some(&auth), &ctx)
            .expect("ok")
            .expect("some");
        assert_eq!(resolved.credential.secret, "bearer-sentinel-compat");
        assert_eq!(resolved.credential.kind, CredentialKind::BearerToken);
        assert_eq!(resolved.source, ResolvedAuthSource::UserStore);
        assert_eq!(
            resolved.credential.authorization_header_value(),
            "Bearer bearer-sentinel-compat"
        );
    }

    #[test]
    fn stored_bearer_rejected_under_api_key_only_with_typed_error() {
        let _master = MasterGuard::with_master("m010-bearer-incompat");
        let (_dir, store) = temp_store();
        store
            .put(
                "cap_incompat",
                Some("acct"),
                CredentialKind::BearerToken,
                "bearer-sentinel-incompat",
                None,
                vec![],
            )
            .expect("put");
        let resolver = AuthResolver::new();
        let auth = AuthConfig::Stored {
            account_id: Some("acct".to_string()),
        };
        let ctx = stored_ctx(
            "cap_incompat",
            None,
            &store,
            CredentialCapability::ApiKeyOnly,
        );
        let err = match resolver.resolve(Some(&auth), &ctx) {
            Ok(_) => panic!("bearer under ApiKeyOnly must fail"),
            Err(err) => err,
        };
        assert!(
            matches!(err, AuthError::Unsupported(_)),
            "expected Unsupported, got {err:?}"
        );
        let rendered = format!("{err}");
        assert!(
            !rendered.contains("bearer-sentinel-incompat"),
            "error must not leak secret"
        );
        assert!(rendered.contains("cap_incompat"));
    }

    #[test]
    fn fallback_bearer_rejected_under_api_key_only() {
        let _master = MasterGuard::with_master("m010-fallback-incompat");
        let (_dir, store) = temp_store();
        store
            .put(
                "cap_fallback",
                None,
                CredentialKind::BearerToken,
                "bearer-sentinel-fallback",
                None,
                vec![],
            )
            .expect("put");
        let resolver = AuthResolver::new();
        // No AuthConfig: fallback store path with ApiKeyOnly must be explicit.
        let ctx = stored_ctx(
            "cap_fallback",
            None,
            &store,
            CredentialCapability::ApiKeyOnly,
        );
        let err = match resolver.resolve(None, &ctx) {
            Ok(_) => panic!("fallback bearer under ApiKeyOnly must fail"),
            Err(err) => err,
        };
        assert!(matches!(err, AuthError::Unsupported(_)));
        assert!(!format!("{err}").contains("bearer-sentinel-fallback"));
    }

    #[test]
    fn fallback_bearer_resolves_under_compatible() {
        let _master = MasterGuard::with_master("m010-fallback-compat");
        let (_dir, store) = temp_store();
        store
            .put(
                "cap_fallback_ok",
                None,
                CredentialKind::BearerToken,
                "bearer-sentinel-fallback-ok",
                None,
                vec![],
            )
            .expect("put");
        let resolver = AuthResolver::new();
        let ctx = stored_ctx(
            "cap_fallback_ok",
            None,
            &store,
            CredentialCapability::ApiKeyOrBearer,
        );
        let resolved = resolver.resolve(None, &ctx).expect("ok").expect("some");
        assert_eq!(resolved.credential.kind, CredentialKind::BearerToken);
        assert_eq!(resolved.source, ResolvedAuthSource::UserStore);
    }

    #[test]
    fn expired_bearer_fails_before_transport() {
        let _master = MasterGuard::with_master("m010-expired-bearer");
        let (_dir, store) = temp_store();
        let expired = Utc::now() - chrono::Duration::minutes(5);
        store
            .put(
                "cap_expired",
                Some("acct"),
                CredentialKind::BearerToken,
                "bearer-sentinel-expired",
                Some(expired),
                vec![],
            )
            .expect("put");
        let resolver = AuthResolver::new();
        let auth = AuthConfig::Stored {
            account_id: Some("acct".to_string()),
        };
        let ctx = stored_ctx(
            "cap_expired",
            None,
            &store,
            CredentialCapability::ApiKeyOrBearer,
        );
        let err = match resolver.resolve(Some(&auth), &ctx) {
            Ok(_) => panic!("expired bearer must fail"),
            Err(err) => err,
        };
        assert!(matches!(err, AuthError::Expired(_)));
        assert!(!format!("{err}").contains("bearer-sentinel-expired"));
    }

    #[test]
    fn expired_api_key_fails_before_transport() {
        let _master = MasterGuard::with_master("m010-expired-apikey");
        let (_dir, store) = temp_store();
        let expired = Utc::now() - chrono::Duration::minutes(5);
        store
            .put(
                "cap_expired_key",
                None,
                CredentialKind::ApiKey,
                "api-sentinel-expired",
                Some(expired),
                vec![],
            )
            .expect("put");
        let resolver = AuthResolver::new();
        let ctx = stored_ctx(
            "cap_expired_key",
            None,
            &store,
            CredentialCapability::ApiKeyOnly,
        );
        let err = match resolver.resolve(None, &ctx) {
            Ok(_) => panic!("expired api key must fail"),
            Err(err) => err,
        };
        assert!(matches!(err, AuthError::Expired(_)));
    }

    #[test]
    fn external_command_and_oauth_remain_unsupported() {
        let resolver = AuthResolver::new();
        let ctx = ResolverContext {
            provider_id: "cap_unsupported".to_string(),
            ..Default::default()
        };
        let external = AuthConfig::ExternalCommand {
            command: "some-cli".to_string(),
            args: vec![],
            timeout_ms: None,
        };
        assert!(matches!(
            resolver.resolve(Some(&external), &ctx),
            Err(AuthError::Unsupported(_))
        ));
        let oauth = AuthConfig::OAuthDevice {
            client_id: "id".to_string(),
            scopes: vec![],
            auth_url: "https://example.invalid/auth".to_string(),
            token_url: "https://example.invalid/token".to_string(),
        };
        assert!(matches!(
            resolver.resolve(Some(&oauth), &ctx),
            Err(AuthError::Unsupported(_))
        ));
        assert!(matches!(
            ExternalCommandProvider::new().fetch(&ExternalCredential {
                command: "some-cli".to_string(),
                args: vec![],
                timeout_ms: None,
            }),
            Err(AuthError::Unsupported(_))
        ));
    }

    #[test]
    fn store_reopen_retains_kind_and_expiry() {
        let _master = MasterGuard::with_master("m010-reopen-kind");
        let dir = tempfile::tempdir().expect("tmpdir");
        let path = dir.path().join("credentials.json");
        let expires_at = Utc::now() + chrono::Duration::hours(1);
        {
            let store = CredentialStore::at_path(path.clone()).expect("store");
            store
                .put(
                    "cap_reopen",
                    Some("acct"),
                    CredentialKind::BearerToken,
                    "bearer-sentinel-reopen",
                    Some(expires_at),
                    vec![],
                )
                .expect("put");
        }
        let reopened = CredentialStore::at_path(path).expect("reopen");
        let record = reopened
            .find_record("cap_reopen", Some("acct"))
            .expect("record");
        assert_eq!(record.kind, CredentialKind::BearerToken);
        assert_eq!(record.expires_at, Some(expires_at));
        let credential = reopened
            .get_credential("cap_reopen", Some("acct"))
            .expect("ok")
            .expect("some");
        assert_eq!(credential.kind, CredentialKind::BearerToken);
        assert_eq!(credential.secret, "bearer-sentinel-reopen");
    }

    #[test]
    fn store_put_replaces_kind_for_same_binding() {
        let _master = MasterGuard::with_master("m010-replace-kind");
        let (_dir, store) = temp_store();
        store
            .put(
                "cap_replace",
                Some("acct"),
                CredentialKind::ApiKey,
                "api-first",
                None,
                vec![],
            )
            .expect("put api");
        store
            .put(
                "cap_replace",
                Some("acct"),
                CredentialKind::BearerToken,
                "bearer-second",
                None,
                vec![],
            )
            .expect("put bearer");
        let records = store.list();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].kind, CredentialKind::BearerToken);
    }

    #[test]
    fn capability_accepts_only_declared_kinds() {
        assert!(CredentialCapability::ApiKeyOnly.accepts(CredentialKind::ApiKey));
        assert!(!CredentialCapability::ApiKeyOnly.accepts(CredentialKind::BearerToken));
        assert!(CredentialCapability::ApiKeyOrBearer.accepts(CredentialKind::ApiKey));
        assert!(CredentialCapability::ApiKeyOrBearer.accepts(CredentialKind::BearerToken));
        assert_eq!(
            CredentialCapability::default(),
            CredentialCapability::ApiKeyOnly
        );
    }
}
