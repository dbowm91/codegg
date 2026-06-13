use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{OnceCell, RwLock};
use tracing::{info, warn};
use url::Url;

use super::client::LspClient;
use super::config::{LspConfig, LspRule};
use super::download;
use super::error::LspError;
use super::language::{detect_language, language_id_to_server_id};
use super::root;
use super::server::{self, LspServerDef};

type ClientMap = Arc<RwLock<HashMap<String, Arc<LspClient>>>>;
type InitCell = Arc<OnceCell<Arc<LspClient>>>;
type InitMap = Arc<RwLock<HashMap<String, InitCell>>>;

pub struct LspService {
    clients: ClientMap,
    /// Tracks in-progress initializations for single-flight semantics.
    initializing: InitMap,
    config: LspConfig,
}

impl LspService {
    pub fn new(config: LspConfig) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(RwLock::new(HashMap::new())),
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

        // Fast path: client already initialized.
        {
            let clients = self.clients.read().await;
            if clients.contains_key(&key) {
                return Ok((key, project_root));
            }
        }

        // Single-flight: create or reuse a OnceCell for this key.
        let cell = {
            let mut init = self.initializing.write().await;
            init.entry(key.clone())
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };

        // All concurrent callers for the same key await this single cell.
        let client = cell
            .get_or_try_init(|| async { self.init_client_inner(server, &project_root).await })
            .await?;

        // Install the client if not already installed.
        {
            let mut clients = self.clients.write().await;
            clients.entry(key.clone()).or_insert_with(|| client.clone());
        }

        // Clean up the initialization cell.
        self.initializing.write().await.remove(&key);

        Ok((key, project_root))
    }

    async fn init_client_inner(
        &self,
        server: &'static LspServerDef,
        root: &Path,
    ) -> Result<Arc<LspClient>, LspError> {
        let binary = download::ensure_server_binary(server).await?;

        let env = self.get_env_overrides(server.id);
        let init_opts = self.get_init_opts(server.id);

        // workspace_configuration takes precedence for server-request responses;
        // fall back to initialization for backward compatibility.
        let configuration = self
            .get_workspace_configuration(server.id)
            .or(init_opts.clone())
            .unwrap_or(serde_json::Value::Null);

        #[allow(unused_mut)]
        let mut client = LspClient::new(server, &binary, root, &env, configuration).await?;
        client.initialize(init_opts).await?;
        client.send_initialized().await?;

        info!(server = server.id, root = ?root, "LSP client initialized");
        Ok(Arc::new(client))
    }

    pub async fn open_file(&self, file_path: &Path, text: &str) -> Result<(), LspError> {
        let (key, _root) = self.get_or_create_client(file_path).await?;

        let client = {
            let clients = self.clients.read().await;
            clients
                .get(&key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.

        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let version = client
            .opened_files
            .lock()
            .await
            .get(&uri.to_string())
            .cloned()
            .unwrap_or(0)
            + 1;
        client.open_file(&uri, text, version).await
    }

    pub async fn update_file(&self, file_path: &Path, text: &str) -> Result<(), LspError> {
        let (key, _root) = self.get_or_create_client(file_path).await?;

        let client = {
            let clients = self.clients.read().await;
            clients
                .get(&key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.

        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;

        let version = client
            .opened_files
            .lock()
            .await
            .get(&uri.to_string())
            .cloned()
            .unwrap_or(0)
            + 1;
        client.update_file(&uri, text, version).await
    }

    pub async fn close_file(&self, file_path: &Path) -> Result<(), LspError> {
        let uri_str = Url::from_file_path(file_path)
            .map(|u| u.to_string())
            .unwrap_or_default();

        // Find the client that has this file open.
        let client = {
            let clients = self.clients.read().await;
            let mut found = None;
            for (_, c) in clients.iter() {
                if c.opened_files.lock().await.contains_key(&uri_str) {
                    found = Some(c.clone());
                    break;
                }
            }
            found.ok_or_else(|| {
                LspError::NotInitialized(format!("no client has file '{}' open", uri_str))
            })?
        };
        // Lock released.

        let was_open = client.opened_files.lock().await.contains_key(&uri_str);
        if was_open {
            let uri = Url::from_file_path(file_path).map_err(|_| {
                LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
            })?;
            let _ = client.close_file(&uri).await;
            client.opened_files.lock().await.remove(&uri_str);
        }
        Ok(())
    }

    pub async fn save_file(&self, file_path: &Path, text: Option<&str>) -> Result<(), LspError> {
        let uri_str = Url::from_file_path(file_path)
            .map(|u| u.to_string())
            .unwrap_or_default();

        // Find the client that has this file open.
        let client = {
            let clients = self.clients.read().await;
            let mut found = None;
            for (_, c) in clients.iter() {
                if c.opened_files.lock().await.contains_key(&uri_str) {
                    found = Some(c.clone());
                    break;
                }
            }
            found.ok_or_else(|| {
                LspError::NotInitialized(format!("no client has file '{}' open", uri_str))
            })?
        };
        // Lock released.

        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;
        client.save_file(&uri, text).await
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

    fn get_workspace_configuration(&self, server_id: &str) -> Option<serde_json::Value> {
        match &self.config {
            LspConfig::Rules(rules) => {
                if let Some(LspRule::Active {
                    workspace_configuration,
                    ..
                }) = rules.get(server_id)
                {
                    workspace_configuration
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
        let clients_to_shutdown: Vec<(String, Arc<LspClient>)> = {
            let mut clients = self.clients.write().await;
            clients.drain().collect()
        };
        // Lock released.

        for (key, client) in clients_to_shutdown {
            info!(server = %key, "shutting down LSP client");
            if let Err(e) = client.shutdown().await {
                warn!(server = %key, error = %e, "error shutting down LSP client");
            }
        }
    }

    pub async fn is_file_open(&self, key: &str, uri_str: &str) -> Result<bool, LspError> {
        let client = {
            let clients = self.clients.read().await;
            clients
                .get(key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.
        let result = client.opened_files.lock().await.contains_key(uri_str);
        Ok(result)
    }

    pub async fn ensure_file_open_from_disk(
        &self,
        file_path: &Path,
    ) -> Result<(String, String), LspError> {
        let (key, _root) = self.get_or_create_client(file_path).await?;
        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;
        let uri_str = uri.to_string();

        let text = tokio::fs::read_to_string(file_path).await.map_err(|e| {
            LspError::RequestFailed(format!(
                "failed to read file {}: {}",
                file_path.display(),
                e
            ))
        })?;

        let is_open = self.is_file_open(&key, &uri_str).await?;

        if is_open {
            self.update_file(file_path, &text).await?;
        } else {
            self.open_file(file_path, &text).await?;
        }

        Ok((key, uri_str))
    }

    pub async fn get_diagnostics_for_key(
        &self,
        key: &str,
        uri_str: &str,
    ) -> Result<Vec<lsp_types::Diagnostic>, LspError> {
        let client = {
            let clients = self.clients.read().await;
            clients
                .get(key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.
        Ok(client.get_diagnostics(uri_str).await)
    }

    pub async fn get_all_diagnostics_for_key(
        &self,
        key: &str,
    ) -> Result<HashMap<String, Vec<lsp_types::Diagnostic>>, LspError> {
        let client = {
            let clients = self.clients.read().await;
            clients
                .get(key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.
        Ok(client.get_all_diagnostics().await)
    }

    pub async fn diagnostics_may_still_be_warming(&self, key: &str, uri: &str) -> bool {
        let client = {
            let clients = self.clients.read().await;
            clients.get(key).cloned()
        };
        // Lock released.
        match client {
            Some(c) => c.diagnostics_may_still_be_warming(uri).await,
            None => false,
        }
    }

    pub async fn get_diagnostic_snapshot_for_key(
        &self,
        key: &str,
        uri_str: &str,
    ) -> Result<crate::diagnostics::LspDiagnosticSnapshot, LspError> {
        let client = {
            let clients = self.clients.read().await;
            clients
                .get(key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.
        Ok(client.diagnostic_snapshot(uri_str).await)
    }

    pub async fn send_request(
        &self,
        key: &str,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, LspError> {
        let client = {
            let clients = self.clients.read().await;
            clients
                .get(key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.
        client.send_request(method, params).await
    }

    pub async fn client_keys(&self) -> Vec<String> {
        let clients = self.clients.read().await;
        clients.keys().cloned().collect()
    }

    /// Return a snapshot of the server capabilities for the given client key.
    ///
    /// Returns `None` if the client is not initialized or the key is unknown.
    pub async fn get_capabilities_for_key(
        &self,
        key: &str,
    ) -> Option<lsp_types::ServerCapabilities> {
        let cap_ref = {
            let clients = self.clients.read().await;
            let entry = clients.get(key)?;
            entry.capabilities.clone()
        };
        let x = cap_ref.lock().await.clone();
        x
    }

    pub async fn get_or_create_client_for_file(
        &self,
        file_path: &Path,
    ) -> Result<(String, PathBuf), LspError> {
        self.get_or_create_client(file_path).await
    }

    pub async fn find_existing_client_for_root_hint(
        &self,
        root_hint: Option<&Path>,
        server_id: Option<&str>,
    ) -> Result<(String, PathBuf), LspError> {
        let root = root_hint
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        let keys = self.client_keys().await;

        if let Some(sid) = server_id {
            let key = format!("{}:{}", root.display(), sid);
            if keys.contains(&key) {
                return Ok((key, root));
            }
            return Err(LspError::ServerNotFound(format!(
                "no LSP client for server '{sid}' at root {}; provide file_path to initialize one",
                root.display()
            )));
        }

        let matching: Vec<_> = keys
            .iter()
            .filter(|k| k.starts_with(&format!("{}:", root.display())))
            .cloned()
            .collect();

        if matching.len() == 1 {
            return Ok((matching.into_iter().next().unwrap(), root));
        }

        if matching.is_empty() {
            return Err(LspError::ServerNotFound(format!(
                "no LSP client for root {}; provide file_path to initialize one",
                root.display()
            )));
        }

        Err(LspError::ServerNotFound(format!(
            "multiple LSP clients for root {}; specify server_id to disambiguate",
            root.display()
        )))
    }
}
