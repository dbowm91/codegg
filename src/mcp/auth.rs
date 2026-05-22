use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::Engine;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::error::McpError;

const ENCRYPTION_KEY_ENV: &str = "CODEGG_TOKEN_KEY";
const MAGIC_BYTES: &[u8] = b"CODEGG_ENC_v1";

fn get_encryption_key() -> Option<[u8; 32]> {
    std::env::var(ENCRYPTION_KEY_ENV).ok().map(|k| {
        let key = k.as_bytes();
        if key.len() >= 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&key[..32]);
            arr
        } else {
            let mut hasher = Sha256::new();
            hasher.update(key);
            hasher.finalize().into()
        }
    })
}

fn encrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, McpError> {
    let cipher = Aes256Gcm::new(key.into());
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| McpError::Encryption(e.to_string()))?;
    let mut result = Vec::with_capacity(12 + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

fn decrypt_data(data: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, McpError> {
    let cipher = Aes256Gcm::new(key.into());
    let nonce = Nonce::from_slice(&data[..12]);
    let ciphertext = &data[12..];
    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| McpError::Encryption(e.to_string()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub token_type: String,
    pub expires_at: Option<u64>,
    pub scope: Option<String>,
}

impl TokenSet {
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            now >= expires_at
        } else {
            false
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerTokens {
    pub server_url: String,
    pub tokens: TokenSet,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UsedCode {
    expires_at: u64,
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

        let _ = manager.load_used_codes_sync();
        if manager.token_store.exists() {
            let _ = manager.load_tokens_sync();
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

    fn is_code_used(&self, code: &str) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if let Some(used_code) = self.used_codes.get(code) {
            if now < used_code.expires_at {
                return true;
            }
        }
        false
    }

    async fn mark_code_used(&mut self, code: String, expires_at: u64) -> Result<(), McpError> {
        self.used_codes.insert(code, UsedCode { expires_at });
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

        if self.is_code_used(code) {
            return Err(McpError::OAuth(
                "authorization code has already been used".into(),
            ));
        }

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let code_expires_at = now + 600;

        let tokens = self.exchange_code_for_tokens(
            token_url,
            client_id,
            client_secret,
            code,
            code_verifier,
            redirect_uri,
        )
        .await?;

        self.mark_code_used(code.to_string(), code_expires_at)
            .await?;

        Ok(tokens)
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
            let _ = handle_callback(listener, &state, tx).await;
        });

        Ok((port, rx))
    }

    fn load_tokens_sync(&mut self) -> Result<(), McpError> {
        if !self.token_store.exists() {
            return Ok(());
        }

        let key = get_encryption_key();
        let content = std::fs::read_to_string(&self.token_store)
            .map_err(|e| McpError::OAuth(format!("failed to read token store: {e}")))?;

        let key = match key {
            Some(k) => k,
            None => {
                return Err(McpError::OAuth(
                    "cannot load tokens: CODEGG_TOKEN_KEY environment variable not set"
                        .to_string(),
                ));
            }
        };

        if content.as_bytes().starts_with(MAGIC_BYTES) {
            let encrypted = base64::engine::general_purpose::STANDARD
                .decode(&content[MAGIC_BYTES.len()..])
                .map_err(|e| McpError::OAuth(format!("failed to decode token store: {e}")))?;
            let decrypted = decrypt_data(&encrypted, &key)
                .map_err(|e| McpError::OAuth(format!("failed to decrypt token store: {e}")))?;
            let tokens: Vec<ServerTokens> = serde_json::from_slice(&decrypted)
                .map_err(|e| McpError::OAuth(format!("failed to parse token store: {e}")))?;
            for entry in tokens {
                self.servers.insert(entry.server_url.clone(), entry);
            }
        } else {
            let tokens: Vec<ServerTokens> = serde_json::from_str(&content)
                .map_err(|e| McpError::OAuth(format!("failed to parse token store: {e}")))?;
            for entry in tokens {
                self.servers.insert(entry.server_url.clone(), entry);
            }
        }

        Ok(())
    }

    async fn load_tokens_async(&mut self) -> Result<(), McpError> {
        if !self.token_store.exists() {
            return Ok(());
        }

        let key = get_encryption_key();
        let content = tokio::fs::read_to_string(&self.token_store)
            .await
            .map_err(|e| McpError::OAuth(format!("failed to read token store: {e}")))?;

        let key = match key {
            Some(k) => k,
            None => {
                return Err(McpError::OAuth(
                    "cannot load tokens: CODEGG_TOKEN_KEY environment variable not set"
                        .to_string(),
                ));
            }
        };

        if content.as_bytes().starts_with(MAGIC_BYTES) {
            let encrypted = base64::engine::general_purpose::STANDARD
                .decode(&content[MAGIC_BYTES.len()..])
                .map_err(|e| McpError::OAuth(format!("failed to decode token store: {e}")))?;
            let decrypted = decrypt_data(&encrypted, &key)
                .map_err(|e| McpError::OAuth(format!("failed to decrypt token store: {e}")))?;
            let tokens: Vec<ServerTokens> = serde_json::from_slice(&decrypted)
                .map_err(|e| McpError::OAuth(format!("failed to parse token store: {e}")))?;
            for entry in tokens {
                self.servers.insert(entry.server_url.clone(), entry);
            }
        } else {
            let tokens: Vec<ServerTokens> = serde_json::from_str(&content)
                .map_err(|e| McpError::OAuth(format!("failed to parse token store: {e}")))?;
            for entry in tokens {
                self.servers.insert(entry.server_url.clone(), entry);
            }
        }

        Ok(())
    }

    fn load_used_codes_sync(&mut self) -> Result<(), McpError> {
        if !self.used_codes_store.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&self.used_codes_store)
            .map_err(|e| McpError::OAuth(format!("failed to read used codes store: {e}")))?;

        let codes: std::collections::HashMap<String, UsedCode> = serde_json::from_str(&content)
            .map_err(|e| McpError::OAuth(format!("failed to parse used codes store: {e}")))?;

        self.used_codes = codes;
        self.cleanup_expired_codes();

        Ok(())
    }

    async fn load_used_codes_async(&mut self) -> Result<(), McpError> {
        if !self.used_codes_store.exists() {
            return Ok(());
        }

        let content = tokio::fs::read_to_string(&self.used_codes_store)
            .await
            .map_err(|e| McpError::OAuth(format!("failed to read used codes store: {e}")))?;

        let codes: std::collections::HashMap<String, UsedCode> = serde_json::from_str(&content)
            .map_err(|e| McpError::OAuth(format!("failed to parse used codes store: {e}")))?;

        self.used_codes = codes;
        self.cleanup_expired_codes();

        Ok(())
    }

    #[allow(dead_code)]
    fn save_used_codes_sync(&self) -> Result<(), McpError> {
        let parent = self
            .used_codes_store
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let _ = std::fs::create_dir_all(parent);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let codes_to_keep: std::collections::HashMap<String, UsedCode> = self
            .used_codes
            .iter()
            .filter(|(_, v)| now < v.expires_at)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let content = serde_json::to_string_pretty(&codes_to_keep)
            .map_err(|e| McpError::OAuth(format!("failed to serialize used codes: {e}")))?;

        std::fs::write(&self.used_codes_store, content)
            .map_err(|e| McpError::OAuth(format!("failed to write used codes store: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(
                &self.used_codes_store,
                std::fs::Permissions::from_mode(0o600),
            );
        }

        Ok(())
    }

    async fn save_used_codes_async(&self) -> Result<(), McpError> {
        let parent = self
            .used_codes_store
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let _ = tokio::fs::create_dir_all(parent).await;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let codes_to_keep: std::collections::HashMap<String, UsedCode> = self
            .used_codes
            .iter()
            .filter(|(_, v)| now < v.expires_at)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let content = serde_json::to_string_pretty(&codes_to_keep)
            .map_err(|e| McpError::OAuth(format!("failed to serialize used codes: {e}")))?;

        tokio::fs::write(&self.used_codes_store, content)
            .await
            .map_err(|e| McpError::OAuth(format!("failed to write used codes store: {e}")))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let used_codes_store = self.used_codes_store.clone();
            let _ = tokio::task::spawn_blocking(move || {
                let _ = std::fs::set_permissions(
                    &used_codes_store,
                    std::fs::Permissions::from_mode(0o600),
                );
            })
            .await;
        }

        Ok(())
    }

    async fn save_tokens_async(&self) -> Result<(), McpError> {
        let parent = self
            .token_store
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let _ = tokio::fs::create_dir_all(parent).await;

        let tokens: Vec<&ServerTokens> = self.servers.values().collect();
        let content = serde_json::to_string_pretty(&tokens)
            .map_err(|e| McpError::OAuth(format!("failed to serialize tokens: {e}")))?;

        if let Some(key) = get_encryption_key() {
            let encrypted = encrypt_data(content.as_bytes(), &key)?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&encrypted);
            let final_content = format!("{}{}", "CODEGG_ENC_v1", encoded);
            tokio::fs::write(&self.token_store, final_content)
                .await
                .map_err(|e| McpError::OAuth(format!("failed to write token store: {e}")))?;
        } else {
            return Err(McpError::OAuth(
                "cannot save tokens: CODEGG_TOKEN_KEY environment variable not set".to_string(),
            ));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&self.token_store, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    #[allow(dead_code)]
    fn save_tokens(&self) -> Result<(), McpError> {
        let parent = self
            .token_store
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        let _ = std::fs::create_dir_all(parent);

        let tokens: Vec<&ServerTokens> = self.servers.values().collect();
        let content = serde_json::to_string_pretty(&tokens)
            .map_err(|e| McpError::OAuth(format!("failed to serialize tokens: {e}")))?;

        if let Some(key) = get_encryption_key() {
            let encrypted = encrypt_data(content.as_bytes(), &key)?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(&encrypted);
            let final_content = format!("{}{}", "CODEGG_ENC_v1", encoded);
            std::fs::write(&self.token_store, final_content)
                .map_err(|e| McpError::OAuth(format!("failed to write token store: {e}")))?;
        } else {
            return Err(McpError::OAuth(
                "cannot save tokens: CODEGG_TOKEN_KEY environment variable not set".to_string(),
            ));
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ =
                std::fs::set_permissions(&self.token_store, std::fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }
}

impl Default for OAuthManager {
    fn default() -> Self {
        Self::new()
    }
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

    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.flush().await;

    let _ = tx.send(code);
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
