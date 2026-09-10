use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use codegg_providers::crypto::{decrypt_from_string, encrypt_to_string};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{hash_map::Entry, HashMap};
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::McpError;

const LEGACY_KEY_ENV: &str = "CODEGG_TOKEN_KEY";
const LEGACY_MAGIC_BYTES: &[u8] = b"CODEGG_ENC_v1";
const V2_MAGIC: &str = "CODEGG_MCP_ENC_v2:";
const USED_CODES_FORMAT_VERSION: u8 = 1;

fn legacy_key_from_value(value: &str) -> [u8; 32] {
    let key = value.as_bytes();
    if key.len() >= 32 {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&key[..32]);
        arr
    } else {
        Sha256::digest(key).into()
    }
}

fn get_legacy_key() -> Option<[u8; 32]> {
    std::env::var(LEGACY_KEY_ENV)
        .ok()
        .map(|value| legacy_key_from_value(&value))
}

/// Decrypts the historical MCP token format. This is deliberately read-only:
/// all new persistence goes through `codegg_providers::crypto`.
fn decrypt_legacy_v1(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, McpError> {
    if data.len() < 12 {
        return Err(McpError::Encryption(
            "legacy token payload is shorter than its nonce".to_string(),
        ));
    }
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| McpError::Encryption(e.to_string()))
}

fn digest_used_code(code: &str) -> String {
    hex::encode(Sha256::digest(code.as_bytes()))
}

fn token_store_error(message: &str) -> McpError {
    McpError::OAuth(message.to_string())
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TokenSet")
            .field("access_token", &"[redacted]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[redacted]"),
            )
            .field("token_type", &self.token_type)
            .field("expires_at", &self.expires_at)
            .field("scope", &self.scope)
            .finish()
    }
}

impl TokenSet {
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            match SystemTime::now().duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_secs() >= expires_at,
                // Fail closed: a clock earlier than UNIX_EPOCH (or any
                // other duration_since error) must not be treated as a
                // 0-second "now", which would otherwise leave every
                // non-zero `expires_at` looking valid.
                Err(error) => {
                    tracing::warn!(
                        ?error,
                        "system clock before UNIX_EPOCH; treating token as expired"
                    );
                    true
                }
            }
        } else {
            false
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerTokens {
    pub server_url: String,
    pub tokens: TokenSet,
}

impl fmt::Debug for ServerTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ServerTokens")
            .field("server_url", &self.server_url)
            .field("tokens", &self.tokens)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsedCode {
    expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsedCodesFile {
    version: u8,
    codes: HashMap<String, UsedCode>,
}

pub struct OAuthManager {
    token_store: PathBuf,
    used_codes_store: PathBuf,
    servers: std::collections::HashMap<String, ServerTokens>,
    used_codes: std::collections::HashMap<String, UsedCode>,
}

impl OAuthManager {
    pub fn new() -> Self {
        let token_store = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codegg")
            .join("mcp_tokens.json");

        let used_codes_store = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codegg")
            .join("mcp_used_codes.json");

        let mut manager = Self {
            token_store,
            used_codes_store,
            servers: std::collections::HashMap::new(),
            used_codes: std::collections::HashMap::new(),
        };

        if let Err(e) = manager.load_used_codes_sync() {
            tracing::warn!("failed to load used codes sync: {}", e);
        }
        if manager.token_store.exists() {
            if let Err(e) = manager.load_tokens_sync() {
                tracing::warn!("failed to load tokens sync: {}", e);
            }
        }
        manager
    }

    pub fn generate_pkce_pair() -> (String, String) {
        let mut verifier = [0u8; 32];
        rand::rng().fill_bytes(&mut verifier);
        let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier);

        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let hash = hasher.finalize();
        let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash);

        (code_verifier, code_challenge)
    }

    pub fn build_authorization_url(
        &self,
        auth_url: &str,
        client_id: &str,
        code_challenge: &str,
        redirect_uri: &str,
        scope: &str,
    ) -> Result<String, McpError> {
        let mut url = url::Url::parse(auth_url)
            .map_err(|e| McpError::OAuth(format!("invalid authorization URL: {e}")))?;

        let redirect = url::Url::parse(redirect_uri)
            .map_err(|e| McpError::OAuth(format!("invalid redirect_uri: {e}")))?;

        if redirect.scheme() != "https"
            && redirect.host_str() != Some("localhost")
            && redirect.host_str() != Some("127.0.0.1")
        {
            return Err(McpError::OAuth(
                "redirect_uri must use HTTPS or be localhost".into(),
            ));
        }

        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", scope)
            .append_pair("state", &OAuthManager::generate_state());

        Ok(url.to_string())
    }

    pub async fn exchange_code_for_tokens(
        &self,
        token_url: &str,
        client_id: &str,
        client_secret: Option<&str>,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, McpError> {
        let params = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("code_verifier", code_verifier),
        ];

        let client = reqwest::Client::new();
        let mut request = client.post(token_url).form(&params);

        if let Some(secret) = client_secret {
            request = request.basic_auth(client_id, Some(secret));
        }

        let resp = request
            .send()
            .await
            .map_err(|e| McpError::OAuth(e.to_string()))?;

        if !resp.status().is_success() {
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(McpError::OAuth(format!("token exchange failed: {text}")));
        }

        let token_response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| McpError::OAuth(e.to_string()))?;

        let access_token = token_response
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::OAuth("missing access_token".into()))?
            .to_string();

        let refresh_token = token_response
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(String::from);

        let token_type = token_response
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer")
            .to_string();

        let expires_in = token_response
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + expires_in;

        let scope = token_response
            .get("scope")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(TokenSet {
            access_token,
            refresh_token,
            token_type,
            expires_at: Some(expires_at),
            scope,
        })
    }

    #[allow(dead_code)]
    fn is_code_used(&self, code: &str) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(used_code) = self.used_codes.get(&digest_used_code(code)) {
            if now < used_code.expires_at {
                return true;
            }
        }
        false
    }

    #[allow(dead_code)]
    async fn mark_code_used(&mut self, code: String, expires_at: u64) -> Result<(), McpError> {
        self.used_codes
            .insert(digest_used_code(&code), UsedCode { expires_at });
        self.save_used_codes_async().await
    }

    fn cleanup_expired_codes(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.used_codes.retain(|_, v| now < v.expires_at);
    }

    pub async fn exchange_code_for_tokens_with_replay_protection(
        &mut self,
        token_url: &str,
        client_id: &str,
        client_secret: Option<&str>,
        code: &str,
        code_verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, McpError> {
        self.cleanup_expired_codes();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let code_expires_at = now + 600;

        {
            let code_digest = digest_used_code(code);
            let entry = self.used_codes.entry(code_digest.clone());
            if matches!(entry, Entry::Occupied(_)) {
                return Err(McpError::OAuth(
                    "authorization code has already been used".into(),
                ));
            }
            entry.or_insert(UsedCode {
                expires_at: code_expires_at,
            });
        }

        if let Err(e) = self.save_used_codes_async().await {
            self.used_codes.remove(code);
            return Err(e);
        }

        let tokens = self
            .exchange_code_for_tokens(
                token_url,
                client_id,
                client_secret,
                code,
                code_verifier,
                redirect_uri,
            )
            .await;

        if tokens.is_err() {
            self.used_codes.remove(&digest_used_code(code));
        }

        tokens
    }

    pub async fn refresh_tokens(
        &self,
        token_url: &str,
        client_id: &str,
        client_secret: Option<&str>,
        refresh_token: &str,
    ) -> Result<TokenSet, McpError> {
        let params = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
        ];

        let client = reqwest::Client::new();
        let mut request = client.post(token_url).form(&params);

        if let Some(secret) = client_secret {
            request = request.basic_auth(client_id, Some(secret));
        }

        let resp = request
            .send()
            .await
            .map_err(|e| McpError::OAuth(e.to_string()))?;

        if !resp.status().is_success() {
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(McpError::OAuth(format!("token refresh failed: {text}")));
        }

        let token_response: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| McpError::OAuth(e.to_string()))?;

        let access_token = token_response
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| McpError::OAuth("missing access_token".into()))?
            .to_string();

        let new_refresh_token = token_response
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(String::from);

        let token_type = token_response
            .get("token_type")
            .and_then(|v| v.as_str())
            .unwrap_or("Bearer")
            .to_string();

        let expires_in = token_response
            .get("expires_in")
            .and_then(|v| v.as_u64())
            .unwrap_or(3600);

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + expires_in;

        let scope = token_response
            .get("scope")
            .and_then(|v| v.as_str())
            .map(String::from);

        Ok(TokenSet {
            access_token,
            refresh_token: new_refresh_token,
            token_type,
            expires_at: Some(expires_at),
            scope,
        })
    }

    pub async fn revoke_token(
        &self,
        revocation_url: &str,
        client_id: &str,
        client_secret: Option<&str>,
        token: &str,
    ) -> Result<(), McpError> {
        let params = vec![("token", token), ("client_id", client_id)];

        let client = reqwest::Client::new();
        let mut request = client.post(revocation_url).form(&params);

        if let Some(secret) = client_secret {
            request = request.basic_auth(client_id, Some(secret));
        }

        let resp = request
            .send()
            .await
            .map_err(|e| McpError::OAuth(e.to_string()))?;

        if !resp.status().is_success() {
            let text = resp
                .text()
                .await
                .unwrap_or_else(|_| "unknown error".to_string());
            return Err(McpError::OAuth(format!("token revocation failed: {text}")));
        }

        Ok(())
    }

    pub async fn store_tokens_async(
        &mut self,
        server_url: &str,
        tokens: TokenSet,
    ) -> Result<(), McpError> {
        let entry = ServerTokens {
            server_url: server_url.to_string(),
            tokens,
        };
        self.servers.insert(server_url.to_string(), entry);
        self.save_tokens_async().await
    }

    pub fn get_tokens(&self, server_url: &str) -> Option<&TokenSet> {
        self.servers.get(server_url).map(|entry| &entry.tokens)
    }

    pub fn get_valid_token(&self, server_url: &str) -> Option<&TokenSet> {
        let tokens = self.get_tokens(server_url)?;
        if tokens.is_expired() {
            return None;
        }
        Some(tokens)
    }

    pub fn get_token_for_server(&self, server_url: &str) -> Option<String> {
        self.get_valid_token(server_url)
            .map(|t| t.access_token.clone())
    }

    pub async fn remove_tokens_async(&mut self, server_url: &str) -> Result<(), McpError> {
        self.servers.remove(server_url);
        self.save_tokens_async().await
    }

    pub fn generate_state() -> String {
        let mut state = [0u8; 16];
        rand::rng().fill_bytes(&mut state);
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(state)
    }

    pub async fn start_callback_server(
        expected_state: &str,
    ) -> Result<
        (
            u16,
            tokio::sync::oneshot::Receiver<Result<String, McpError>>,
        ),
        McpError,
    > {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|e| McpError::OAuth(format!("failed to bind callback server: {e}")))?;
        let local_addr = listener
            .local_addr()
            .map_err(|e| McpError::OAuth(format!("failed to get local addr: {e}")))?;
        let port = local_addr.port();

        let (tx, rx) = tokio::sync::oneshot::channel();
        let state = expected_state.to_string();

        tokio::spawn(async move {
            if let Err(e) = handle_callback(listener, &state, tx).await {
                tracing::error!(error = %e, "OAuth callback handler failed; oneshot dropped");
            }
        });

        Ok((port, rx))
    }

    fn load_tokens_sync(&mut self) -> Result<(), McpError> {
        if !self.token_store.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.token_store)
            .map_err(|e| McpError::OAuth(format!("failed to read token store: {e}")))?;

        match decode_token_store(&content)? {
            DecodedTokenStore::V2(tokens) => {
                self.install_tokens(tokens);
            }
            DecodedTokenStore::Legacy(tokens) => {
                self.install_tokens(tokens.clone());
                if let Some(master_key) = codegg_config::encryption::get_master_key() {
                    if let Err(error) =
                        migrate_legacy_token_store(&self.token_store, &tokens, &master_key)
                    {
                        tracing::warn!(
                            error = %error,
                            "MCP OAuth token-store migration deferred; legacy store remains available"
                        );
                    } else {
                        tracing::info!("MCP OAuth token store migrated to canonical encryption");
                    }
                } else {
                    tracing::warn!(
                        "MCP OAuth token store uses deprecated CODEGG_TOKEN_KEY; configure a canonical master key to migrate"
                    );
                }
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    async fn load_tokens_async(&mut self) -> Result<(), McpError> {
        self.load_tokens_sync()
    }

    fn load_used_codes_sync(&mut self) -> Result<(), McpError> {
        if !self.used_codes_store.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.used_codes_store)
            .map_err(|e| McpError::OAuth(format!("failed to read used codes store: {e}")))?;

        let value: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| McpError::OAuth(format!("failed to parse used codes store: {e}")))?;

        let legacy = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .is_none();
        if legacy {
            let codes: HashMap<String, UsedCode> = serde_json::from_value(value)
                .map_err(|e| McpError::OAuth(format!("failed to parse used codes store: {e}")))?;
            self.used_codes = codes
                .into_iter()
                .map(|(code, used_code)| (digest_used_code(&code), used_code))
                .collect();
        } else {
            let store: UsedCodesFile = serde_json::from_str(&content)
                .map_err(|e| McpError::OAuth(format!("failed to parse used codes store: {e}")))?;
            if store.version != USED_CODES_FORMAT_VERSION {
                return Err(token_store_error("unsupported used codes store version"));
            }
            self.used_codes = store.codes;
        }
        self.cleanup_expired_codes();

        if legacy {
            if let Err(error) = self.save_used_codes_sync() {
                tracing::warn!(
                    error = %error,
                    "MCP OAuth used-code store migration deferred; replay digests remain in memory"
                );
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    async fn load_used_codes_async(&mut self) -> Result<(), McpError> {
        self.load_used_codes_sync()
    }

    #[allow(dead_code)]
    fn save_used_codes_sync(&self) -> Result<(), McpError> {
        let parent = self
            .used_codes_store
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(parent)
            .map_err(|e| McpError::OAuth(format!("failed to create token directory: {e}")))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let codes_to_keep: HashMap<String, UsedCode> = self
            .used_codes
            .iter()
            .filter(|(_, v)| now < v.expires_at)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let content = serde_json::to_string_pretty(&UsedCodesFile {
            version: USED_CODES_FORMAT_VERSION,
            codes: codes_to_keep,
        })
        .map_err(|e| McpError::OAuth(format!("failed to serialize used codes: {e}")))?;

        write_secure_atomic_sync(&self.used_codes_store, &content, "used codes")
    }

    async fn save_used_codes_async(&self) -> Result<(), McpError> {
        let parent = self
            .used_codes_store
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(parent)
            .map_err(|e| McpError::OAuth(format!("failed to create token directory: {e}")))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let codes_to_keep: HashMap<String, UsedCode> = self
            .used_codes
            .iter()
            .filter(|(_, v)| now < v.expires_at)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let content = serde_json::to_string_pretty(&UsedCodesFile {
            version: USED_CODES_FORMAT_VERSION,
            codes: codes_to_keep,
        })
        .map_err(|e| McpError::OAuth(format!("failed to serialize used codes: {e}")))?;

        let path = self.used_codes_store.clone();
        tokio::task::spawn_blocking(move || write_secure_atomic_sync(&path, &content, "used codes"))
            .await
            .map_err(|_| token_store_error("used codes store write task failed"))?
    }

    async fn save_tokens_async(&self) -> Result<(), McpError> {
        let content = self.serialize_v2_tokens()?;
        let path = self.token_store.clone();
        tokio::task::spawn_blocking(move || write_secure_atomic_sync(&path, &content, "token"))
            .await
            .map_err(|_| token_store_error("token store write task failed"))?
    }

    #[allow(dead_code)]
    fn save_tokens(&self) -> Result<(), McpError> {
        let content = self.serialize_v2_tokens()?;
        write_secure_atomic_sync(&self.token_store, &content, "token")
    }

    fn install_tokens(&mut self, tokens: Vec<ServerTokens>) {
        for entry in tokens {
            self.servers.insert(entry.server_url.clone(), entry);
        }
    }

    fn serialize_v2_tokens(&self) -> Result<String, McpError> {
        let master_key = codegg_config::encryption::get_master_key().ok_or_else(|| {
            token_store_error(
                "cannot save MCP OAuth tokens: canonical master key is not configured; set CODEGG_MASTER_KEY",
            )
        })?;
        let tokens: Vec<ServerTokens> = self.servers.values().cloned().collect();
        encode_v2_token_store(&tokens, &master_key)
    }
}

impl Default for OAuthManager {
    fn default() -> Self {
        Self::new()
    }
}

enum DecodedTokenStore {
    V2(Vec<ServerTokens>),
    Legacy(Vec<ServerTokens>),
}

fn decode_token_store(content: &str) -> Result<DecodedTokenStore, McpError> {
    if let Some(ciphertext) = content.strip_prefix(V2_MAGIC) {
        let master_key = codegg_config::encryption::get_master_key().ok_or_else(|| {
            token_store_error(
                "cannot load MCP OAuth tokens: canonical master key is not configured; set CODEGG_MASTER_KEY",
            )
        })?;
        let plaintext = decrypt_from_string(ciphertext, &master_key)
            .map_err(|_| token_store_error("failed to decrypt MCP OAuth token store"))?;
        let tokens = serde_json::from_str(&plaintext)
            .map_err(|_| token_store_error("failed to parse MCP OAuth token store"))?;
        return Ok(DecodedTokenStore::V2(tokens));
    }

    if let Some(encoded) = content.strip_prefix(std::str::from_utf8(LEGACY_MAGIC_BYTES).unwrap()) {
        let legacy_key = get_legacy_key().ok_or_else(|| {
            token_store_error(
                "cannot load legacy MCP OAuth tokens: CODEGG_TOKEN_KEY is not configured",
            )
        })?;
        let encrypted = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| token_store_error("failed to decode legacy MCP OAuth token store"))?;
        let plaintext = decrypt_legacy_v1(&encrypted, &legacy_key)
            .map_err(|_| token_store_error("failed to decrypt legacy MCP OAuth token store"))?;
        let tokens = serde_json::from_slice(&plaintext)
            .map_err(|_| token_store_error("failed to parse legacy MCP OAuth token store"))?;
        return Ok(DecodedTokenStore::Legacy(tokens));
    }

    Err(token_store_error(
        "unsupported MCP OAuth token-store format; refusing plaintext or unknown data",
    ))
}

fn encode_v2_token_store(tokens: &[ServerTokens], master_key: &str) -> Result<String, McpError> {
    let plaintext = serde_json::to_string(tokens)
        .map_err(|_| token_store_error("failed to serialize MCP OAuth token store"))?;
    let ciphertext = encrypt_to_string(&plaintext, master_key)
        .map_err(|_| token_store_error("failed to encrypt MCP OAuth token store"))?;
    Ok(format!("{V2_MAGIC}{ciphertext}"))
}

fn token_sets_equal(left: &[ServerTokens], right: &[ServerTokens]) -> bool {
    let as_map = |tokens: &[ServerTokens]| {
        tokens
            .iter()
            .map(|entry| (entry.server_url.clone(), entry.clone()))
            .collect::<HashMap<_, _>>()
    };
    as_map(left) == as_map(right)
}

fn migrate_legacy_token_store(
    path: &Path,
    tokens: &[ServerTokens],
    master_key: &str,
) -> Result<(), McpError> {
    let content = encode_v2_token_store(tokens, master_key)?;
    let temporary = write_secure_temp_sync(path, &content, "token migration")?;
    let result = (|| {
        let readback = fs::read_to_string(&temporary)
            .map_err(|_| token_store_error("failed to read back migrated MCP OAuth token store"))?;
        let DecodedTokenStore::V2(readback_tokens) = decode_token_store(&readback)? else {
            return Err(token_store_error(
                "migrated MCP OAuth token store did not retain its v2 format",
            ));
        };
        if !token_sets_equal(tokens, &readback_tokens) {
            return Err(token_store_error(
                "migrated MCP OAuth token store failed semantic read-back verification",
            ));
        }
        replace_secure_temp_sync(&temporary, path, "token migration")
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_secure_temp_sync(path: &Path, content: &str, purpose: &str) -> Result<PathBuf, McpError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|_| {
        token_store_error(&format!(
            "failed to create directory for {purpose} persistence"
        ))
    })?;

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("store");
    let temporary = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|_| {
            token_store_error(&format!("failed to create temporary {purpose} store"))
        })?;
        file.write_all(content.as_bytes()).map_err(|_| {
            token_store_error(&format!("failed to write temporary {purpose} store"))
        })?;
        file.sync_all()
            .map_err(|_| token_store_error(&format!("failed to sync temporary {purpose} store")))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temporary, fs::Permissions::from_mode(0o600)).map_err(|_| {
                token_store_error(&format!("failed to secure temporary {purpose} store"))
            })?;
        }
        Ok(temporary.clone())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn replace_secure_temp_sync(
    temporary: &Path,
    destination: &Path,
    purpose: &str,
) -> Result<(), McpError> {
    fs::rename(temporary, destination)
        .map_err(|_| token_store_error(&format!("failed to atomically replace {purpose} store")))?;

    #[cfg(unix)]
    if let Some(parent) = destination.parent() {
        if let Ok(directory) = fs::File::open(parent) {
            let _ = directory.sync_all();
        }
    }
    Ok(())
}

fn write_secure_atomic_sync(path: &Path, content: &str, purpose: &str) -> Result<(), McpError> {
    let temporary = write_secure_temp_sync(path, content, purpose)?;
    let result = replace_secure_temp_sync(&temporary, path, purpose);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

async fn handle_callback(
    listener: TcpListener,
    expected_state: &str,
    tx: tokio::sync::oneshot::Sender<Result<String, McpError>>,
) -> Result<(), McpError> {
    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|e| McpError::OAuth(format!("failed to accept connection: {e}")))?;

    let mut buf = [0u8; 4096];
    let n = stream
        .read(&mut buf)
        .await
        .map_err(|e| McpError::OAuth(format!("failed to read request: {e}")))?;

    let request = String::from_utf8_lossy(&buf[..n]);
    let code = parse_callback_params(&request, expected_state);

    let body = match &code {
        Ok(_) => {
            "<html><head><title>OAuth Callback</title></head><body>\
             <h1>Authentication Successful</h1>\
             <p>You can close this window and return to codegg.</p>\
             </html>"
        }
        Err(e) => &format!(
            "<html><head><title>OAuth Callback</title></head><body>\
                 <h1>Authentication Failed</h1>\
                 <p>{e}</p>\
                 </html>"
        ),
    };

    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        if code.is_ok() { "200 OK" } else { "400 Bad Request" },
        body.len(),
        body
    );

    if let Err(error) = stream.write_all(resp.as_bytes()).await {
        tracing::warn!(error = %error, "failed to write OAuth callback response");
    }
    if let Err(error) = stream.flush().await {
        tracing::warn!(error = %error, "failed to flush OAuth callback response");
    }

    if tx.send(code).is_err() {
        tracing::debug!("OAuth callback receiver closed before code delivery");
    }
    Ok(())
}

fn parse_callback_params(request: &str, expected_state: &str) -> Result<String, McpError> {
    let first_line = request.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("");
    let query = path.split('?').nth(1).unwrap_or("");

    let mut code = None;
    let mut state = None;

    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("");
        match key {
            "code" => code = Some(val.to_string()),
            "state" => state = Some(val.to_string()),
            _ => {}
        }
    }

    let code = code.ok_or_else(|| McpError::OAuth("missing code parameter".into()))?;
    let state = state.ok_or_else(|| McpError::OAuth("missing state parameter".into()))?;

    if state != expected_state {
        return Err(McpError::OAuth("state mismatch".into()));
    }

    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    struct EnvironmentGuard {
        previous: Vec<(&'static str, Option<String>)>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvironmentGuard {
        fn new() -> Self {
            let lock = crate::auth::test_support::lock_env();
            let names = [
                LEGACY_KEY_ENV,
                "CODEGG_MASTER_KEY",
                "CODEGG_ENCRYPTION_KEY",
                "OPENCODE_ENCRYPTION_KEY",
            ];
            let previous = names
                .into_iter()
                .map(|name| {
                    let value = std::env::var(name).ok();
                    std::env::remove_var(name);
                    (name, value)
                })
                .collect();
            Self {
                previous,
                _lock: lock,
            }
        }

        fn set(&self, name: &str, value: &str) {
            std::env::set_var(name, value);
        }
    }

    impl Drop for EnvironmentGuard {
        fn drop(&mut self) {
            for (name, value) in self.previous.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(name, value);
                } else {
                    std::env::remove_var(name);
                }
            }
        }
    }

    fn sample_tokens() -> Vec<ServerTokens> {
        vec![ServerTokens {
            server_url: "https://mcp.example.test".to_string(),
            tokens: TokenSet {
                access_token: "synthetic-access-token".to_string(),
                refresh_token: Some("synthetic-refresh-token".to_string()),
                token_type: "Bearer".to_string(),
                expires_at: Some(4_000_000_000),
                scope: Some("tools.read".to_string()),
            },
        }]
    }

    fn manager_at(token_store: PathBuf, used_codes_store: PathBuf) -> OAuthManager {
        OAuthManager {
            token_store,
            used_codes_store,
            servers: HashMap::new(),
            used_codes: HashMap::new(),
        }
    }

    fn encode_legacy_v1_for_test(tokens: &[ServerTokens], key_value: &str) -> String {
        let plaintext = serde_json::to_vec(tokens).expect("serialize legacy fixture");
        let key = legacy_key_from_value(key_value);
        let cipher = Aes256Gcm::new((&key).into());
        let mut nonce_bytes = [0u8; 12];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let ciphertext = cipher
            .encrypt(Nonce::from_slice(&nonce_bytes), plaintext.as_ref())
            .expect("encrypt legacy fixture");
        let mut encrypted = nonce_bytes.to_vec();
        encrypted.extend_from_slice(&ciphertext);
        format!(
            "{}{}",
            std::str::from_utf8(LEGACY_MAGIC_BYTES).expect("legacy prefix"),
            base64::engine::general_purpose::STANDARD.encode(encrypted)
        )
    }

    #[test]
    fn legacy_v1_reader_loads_without_canonical_key_but_never_rewrites_legacy() {
        let environment = EnvironmentGuard::new();
        environment.set(LEGACY_KEY_ENV, "synthetic-legacy-key");
        let directory = tempfile::tempdir().expect("temporary directory");
        let token_store = directory.path().join("mcp_tokens.json");
        let used_codes_store = directory.path().join("mcp_used_codes.json");
        let original = encode_legacy_v1_for_test(&sample_tokens(), "synthetic-legacy-key");
        fs::write(&token_store, &original).expect("write legacy fixture");

        let mut manager = manager_at(token_store.clone(), used_codes_store);
        manager.load_tokens_sync().expect("load legacy fixture");

        assert_eq!(
            manager.get_token_for_server("https://mcp.example.test"),
            Some("synthetic-access-token".to_string())
        );
        assert_eq!(fs::read_to_string(&token_store).unwrap(), original);
        let error = manager.save_tokens().unwrap_err();
        assert!(format!("{error}").contains("master key"));
        assert!(!format!("{error}").contains("synthetic-"));
    }

    #[test]
    fn legacy_v1_migrates_transactionally_and_restarts_from_v2() {
        let environment = EnvironmentGuard::new();
        environment.set(LEGACY_KEY_ENV, "synthetic-legacy-key");
        environment.set("CODEGG_MASTER_KEY", "synthetic-master-key");
        let directory = tempfile::tempdir().expect("temporary directory");
        let token_store = directory.path().join("mcp_tokens.json");
        let used_codes_store = directory.path().join("mcp_used_codes.json");
        fs::write(
            &token_store,
            encode_legacy_v1_for_test(&sample_tokens(), "synthetic-legacy-key"),
        )
        .expect("write legacy fixture");

        let mut manager = manager_at(token_store.clone(), used_codes_store.clone());
        manager.load_tokens_sync().expect("migrate legacy fixture");
        let migrated = fs::read_to_string(&token_store).expect("read migrated fixture");
        assert!(migrated.starts_with(V2_MAGIC));
        assert!(!migrated.contains("synthetic-"));

        std::env::remove_var(LEGACY_KEY_ENV);
        let mut restarted = manager_at(token_store, used_codes_store);
        restarted
            .load_tokens_sync()
            .expect("load migrated fixture without legacy key");
        assert_eq!(
            restarted.get_tokens("https://mcp.example.test"),
            manager.get_tokens("https://mcp.example.test")
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn new_token_writes_use_v2_and_survive_restart() {
        let environment = EnvironmentGuard::new();
        environment.set("CODEGG_MASTER_KEY", "synthetic-master-key");
        let directory = tempfile::tempdir().expect("temporary directory");
        let token_store = directory.path().join("mcp_tokens.json");
        let used_codes_store = directory.path().join("mcp_used_codes.json");
        let mut manager = manager_at(token_store.clone(), used_codes_store.clone());
        let entry = sample_tokens().remove(0);

        manager
            .store_tokens_async(&entry.server_url, entry.tokens.clone())
            .await
            .expect("write v2 token store");
        let content = fs::read_to_string(&token_store).expect("read v2 token store");
        assert!(content.starts_with(V2_MAGIC));
        assert!(!content.contains("synthetic-"));

        let mut restarted = manager_at(token_store.clone(), used_codes_store);
        restarted
            .load_tokens_sync()
            .expect("restart from v2 token store");
        assert_eq!(restarted.get_tokens(&entry.server_url), Some(&entry.tokens));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(token_store)
                    .expect("v2 token-store metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn wrong_missing_corrupt_and_plaintext_stores_fail_closed_without_overwrite() {
        let environment = EnvironmentGuard::new();
        environment.set("CODEGG_MASTER_KEY", "synthetic-master-key");
        let directory = tempfile::tempdir().expect("temporary directory");
        let token_store = directory.path().join("mcp_tokens.json");
        let used_codes_store = directory.path().join("mcp_used_codes.json");
        let v2 = encode_v2_token_store(&sample_tokens(), "synthetic-master-key")
            .expect("encode v2 fixture");
        fs::write(&token_store, &v2).expect("write v2 fixture");

        std::env::set_var("CODEGG_MASTER_KEY", "wrong-master-key");
        let mut manager = manager_at(token_store.clone(), used_codes_store.clone());
        let error = manager.load_tokens_sync().unwrap_err();
        assert!(format!("{error}").contains("decrypt"));
        assert!(!format!("{error}").contains("synthetic-"));
        assert_eq!(fs::read_to_string(&token_store).unwrap(), v2);

        std::env::remove_var("CODEGG_MASTER_KEY");
        let error = manager.load_tokens_sync().unwrap_err();
        assert!(format!("{error}").contains("master key"));
        assert_eq!(fs::read_to_string(&token_store).unwrap(), v2);

        std::env::set_var("CODEGG_MASTER_KEY", "synthetic-master-key");
        fs::write(&token_store, "CODEGG_MCP_ENC_v2:truncated").expect("write corrupt fixture");
        let error = manager.load_tokens_sync().unwrap_err();
        assert!(format!("{error}").contains("decrypt"));
        assert_eq!(
            fs::read_to_string(&token_store).unwrap(),
            "CODEGG_MCP_ENC_v2:truncated"
        );

        fs::write(&token_store, "[]").expect("write plaintext fixture");
        let error = manager.load_tokens_sync().unwrap_err();
        assert!(format!("{error}").contains("plaintext"));
        assert_eq!(fs::read_to_string(&token_store).unwrap(), "[]");
    }

    #[test]
    fn used_code_legacy_entries_migrate_to_digests_and_keep_replay_semantics() {
        let _environment = EnvironmentGuard::new();
        let directory = tempfile::tempdir().expect("temporary directory");
        let token_store = directory.path().join("mcp_tokens.json");
        let used_codes_store = directory.path().join("mcp_used_codes.json");
        let code = "synthetic-authorization-code";
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_secs()
            + 600;
        fs::write(
            &used_codes_store,
            serde_json::to_string(&HashMap::from([(
                code.to_string(),
                UsedCode { expires_at },
            )]))
            .expect("serialize legacy used-code fixture"),
        )
        .expect("write legacy used-code fixture");

        let mut manager = manager_at(token_store, used_codes_store.clone());
        manager.load_used_codes_sync().expect("load used codes");

        assert!(manager.is_code_used(code));
        let migrated = fs::read_to_string(&used_codes_store).expect("read migrated used codes");
        assert!(migrated.contains("\"version\": 1"));
        assert!(migrated.contains(&digest_used_code(code)));
        assert!(!migrated.contains(code));
    }

    #[test]
    fn token_debug_output_redacts_access_and_refresh_tokens() {
        let entries = sample_tokens();
        let tokens = entries[0].tokens.clone();
        let rendered = format!("{tokens:?}");
        assert!(rendered.contains("[redacted]"));
        assert!(!rendered.contains("synthetic-access-token"));
        assert!(!rendered.contains("synthetic-refresh-token"));
    }

    #[cfg(unix)]
    #[test]
    fn failed_migration_write_preserves_legacy_source() {
        let environment = EnvironmentGuard::new();
        environment.set(LEGACY_KEY_ENV, "synthetic-legacy-key");
        environment.set("CODEGG_MASTER_KEY", "synthetic-master-key");
        let directory = tempfile::tempdir().expect("temporary directory");
        let token_store = directory.path().join("mcp_tokens.json");
        let original = encode_legacy_v1_for_test(&sample_tokens(), "synthetic-legacy-key");
        fs::write(&token_store, &original).expect("write legacy fixture");

        let original_permissions = fs::metadata(directory.path())
            .expect("directory metadata")
            .permissions();
        let mut read_only = original_permissions.clone();
        use std::os::unix::fs::PermissionsExt;
        read_only.set_mode(0o500);
        fs::set_permissions(directory.path(), read_only).expect("make directory read-only");
        let result =
            migrate_legacy_token_store(&token_store, &sample_tokens(), "synthetic-master-key");
        fs::set_permissions(directory.path(), original_permissions).expect("restore permissions");

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(token_store).unwrap(), original);
    }
}
