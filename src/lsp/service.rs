use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{info, warn};
use url::Url;

use super::client::LspClient;
use super::download;
use super::language::{detect_language, language_id_to_server_id};
use super::root;
use super::server::{self, LspServerDef};
use crate::config::schema::{LspConfig, LspRule};
use crate::error::LspError;

struct ClientEntry {
    client: LspClient,
}

pub struct LspService {
    clients: Arc<RwLock<HashMap<String, ClientEntry>>>,
    config: LspConfig,
}

impl LspService {
    pub fn new(config: LspConfig) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    pub async fn get_or_create_client(
        &self,
        file_path: &Path,
    ) -> Result<(String, PathBuf), LspError> {
        let lang = detect_language(file_path.to_str().unwrap_or("")).ok_or_else(|| {
            LspError::UnsupportedLanguage(format!("unknown language for {}", file_path.display()))
        })?;

        let server_id = language_id_to_server_id(lang).ok_or_else(|| {
            LspError::UnsupportedLanguage(format!("no LSP server for language '{}'", lang))
        })?;

        if self.is_disabled(server_id) {
            return Err(LspError::ServerNotFound(format!(
                "server '{}' disabled by config",
                server_id
            )));
        }

        let server = server::find_server(server_id).ok_or_else(|| {
            LspError::ServerNotFound(format!("server definition not found for '{}'", server_id))
        })?;

        let project_root = root::find_project_root(file_path).ok_or_else(|| {
            LspError::LaunchFailed("could not determine project root".to_string())
        })?;

        let key = format!("{}:{}", project_root.display(), server_id);

        {
            let clients = self.clients.read().await;
            if clients.contains_key(&key) {
                return Ok((key, project_root));
            }
        }

        self.init_client(server, &project_root).await?;

        Ok((key, project_root))
    }

    async fn init_client(
        &self,
        server: &'static LspServerDef,
        root: &Path,
    ) -> Result<(), LspError> {
        let binary = download::ensure_server_binary(server).await?;

        let env = self.get_env_overrides(server.id);
        let init_opts = self.get_init_opts(server.id);

        #[allow(unused_mut)]
        let mut client = LspClient::new(server, &binary, root, &env).await?;
        client.initialize(init_opts).await?;
        client.send_initialized().await?;

        let key = format!("{}:{}", root.display(), server.id);
        self.clients
            .write()
            .await
            .insert(key, ClientEntry { client });

        info!(server = server.id, root = ?root, "LSP client initialized");
        Ok(())
    }

    pub async fn open_file(&self, file_path: &Path, text: &str) -> Result<(), LspError> {
        let (key, _root) = self.get_or_create_client(file_path).await?;

        let mut clients = self.clients.write().await;
        let entry = clients
            .get_mut(&key)
            .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?;

        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let version = entry
            .client
            .opened_files
            .lock()
            .await
            .get(&uri.to_string())
            .cloned()
            .unwrap_or(0)
            + 1;
        entry.client.open_file(&uri, text, version).await
    }

    pub async fn update_file(&self, file_path: &Path, text: &str) -> Result<(), LspError> {
        let (key, _root) = self.get_or_create_client(file_path).await?;

        let mut clients = self.clients.write().await;
        let entry = clients
            .get_mut(&key)
            .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?;

        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let version = entry
            .client
            .opened_files
            .lock()
            .await
            .get(&uri.to_string())
            .cloned()
            .unwrap_or(0)
            + 1;
        entry.client.update_file(&uri, text, version).await
    }

    pub async fn close_file(&self, file_path: &Path) -> Result<(), LspError> {
        let uri_str = Url::from_file_path(file_path)
            .map(|u| u.to_string())
            .unwrap_or_default();

        let clients = self.clients.read().await;
        let key = {
            let mut found = None;
            for (k, e) in clients.iter() {
                if e.client.opened_files.lock().await.contains_key(&uri_str) {
                    found = Some(k.clone());
                    break;
                }
            }
            found
        };
        drop(clients);

        if let Some(key) = key {
            let mut clients = self.clients.write().await;
            if let Some(entry) = clients.get_mut(&key) {
                let uri = Url::from_file_path(file_path).map_err(|_| {
                    LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
                })?;
                return entry.client.close_file(&uri).await;
            }
        }
        Ok(())
    }

    pub async fn save_file(&self, file_path: &Path, text: Option<&str>) -> Result<(), LspError> {
        let uri_str = Url::from_file_path(file_path)
            .map(|u| u.to_string())
            .unwrap_or_default();

        let clients = self.clients.read().await;
        let key = {
            let mut found = None;
            for (k, e) in clients.iter() {
                if e.client.opened_files.lock().await.contains_key(&uri_str) {
                    found = Some(k.clone());
                    break;
                }
            }
            found
        };
        drop(clients);

        if let Some(key) = key {
            let mut clients = self.clients.write().await;
            if let Some(entry) = clients.get_mut(&key) {
                let uri = Url::from_file_path(file_path).map_err(|_| {
                    LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
                })?;
                return entry.client.save_file(&uri, text).await;
            }
        }
        Ok(())
    }

    pub fn is_disabled(&self, server_id: &str) -> bool {
        match &self.config {
            LspConfig::Disabled(false) => false,
            LspConfig::Disabled(true) => true,
            LspConfig::Rules(rules) => {
                if let Some(rule) = rules.get(server_id) {
                    match rule {
                        LspRule::Disabled { disabled } => *disabled,
                        LspRule::Active { disabled, .. } => disabled.unwrap_or(false),
                    }
                } else {
                    false
                }
            }
        }
    }

    fn get_env_overrides(&self, server_id: &str) -> Vec<(String, String)> {
        match &self.config {
            LspConfig::Rules(rules) => {
                if let Some(LspRule::Active { env, .. }) = rules.get(server_id) {
                    env.as_ref()
                        .map(|e| e.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                }
            }
            _ => Vec::new(),
        }
    }

    fn get_init_opts(&self, server_id: &str) -> Option<serde_json::Value> {
        match &self.config {
            LspConfig::Rules(rules) => {
                if let Some(LspRule::Active { initialization, .. }) = rules.get(server_id) {
                    initialization
                        .clone()
                        .map(serde_json::to_value)
                        .transpose()
                        .ok()
                        .flatten()
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    pub async fn shutdown_all(&self) {
        let mut clients = self.clients.write().await;
        for (key, entry) in clients.iter_mut() {
            info!(server = %key, "shutting down LSP client");
            if let Err(e) = entry.client.shutdown().await {
                warn!(server = %key, error = %e, "error shutting down LSP client");
            }
        }
        clients.clear();
    }

    pub async fn get_diagnostics_for_key(
        &self,
        key: &str,
        uri_str: &str,
    ) -> Result<Vec<lsp_types::Diagnostic>, LspError> {
        let clients = self.clients.read().await;
        let entry = clients
            .get(key)
            .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?;
        Ok(entry.client.get_diagnostics(uri_str).await)
    }

    pub async fn get_all_diagnostics_for_key(
        &self,
        key: &str,
    ) -> Result<HashMap<String, Vec<lsp_types::Diagnostic>>, LspError> {
        let clients = self.clients.read().await;
        let entry = clients
            .get(key)
            .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?;
        Ok(entry.client.get_all_diagnostics().await)
    }

    pub async fn send_request(
        &self,
        key: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, LspError> {
        let mut clients = self.clients.write().await;
        let entry = clients
            .get_mut(key)
            .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?;
        entry.client.send_request(method, params).await
    }

    pub async fn client_keys(&self) -> Vec<String> {
        let clients = self.clients.read().await;
        clients.keys().cloned().collect()
    }
}
