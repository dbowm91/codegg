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

// --- Credential ---

#[derive(Debug, Clone)]
pub struct Credential {
    pub kind: CredentialKind,
    pub secret: String,
    pub expires_at: Option<DateTime<Utc>>,
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
                    if let Some(plain) =
                        store.get_plaintext(&ctx.provider_id, account.as_deref(), |s| {
                            s.kind == CredentialKind::ApiKey
                        })?
                    {
                        return Ok(Some(resolved(
                            Credential::api_key(plain),
                            ResolvedAuthSource::UserStore,
                        )));
                    }
                    return Err(AuthError::NotFound(ctx.provider_id.clone()));
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
            if let Some(plain) =
                store.get_plaintext(&ctx.provider_id, ctx.account_id.as_deref(), |s| {
                    s.kind == CredentialKind::ApiKey
                })?
            {
                return Ok(Some(resolved(
                    Credential::api_key(plain),
                    ResolvedAuthSource::UserStore,
                )));
            }
        }
        Ok(None)
    }
}

fn resolved(credential: Credential, source: ResolvedAuthSource) -> ResolvedAuth {
    ResolvedAuth { credential, source }
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

    pub fn list(&self) -> Vec<StoredCredentialRecord> {
        self.records.lock().expect("poisoned").clone()
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
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(body.as_bytes())?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)?;
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
