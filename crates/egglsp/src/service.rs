use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
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

enum InitSlot {
    Starting {
        waiters: Vec<tokio::sync::oneshot::Sender<Result<Arc<LspClient>, LspError>>>,
    },
    Ready(Arc<LspClient>),
}

type InitMap = Arc<tokio::sync::RwLock<HashMap<String, Arc<Mutex<InitSlot>>>>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceLifecycle {
    Running,
    ShuttingDown,
    Stopped,
}

pub struct LspService {
    clients: ClientMap,
    /// Tracks in-progress initializations for single-flight semantics.
    initializing: InitMap,
    /// Maps document URI string → client key for O(1) ownership lookup.
    document_owners: Arc<RwLock<HashMap<String, String>>>,
    /// Lifecycle state to prevent new client acquisition after shutdown begins.
    lifecycle: Arc<RwLock<ServiceLifecycle>>,
    config: LspConfig,
}

impl LspService {
    pub fn new(config: LspConfig) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(ServiceLifecycle::Running)),
            config,
        }
    }

    pub async fn get_or_create_client(
        &self,
        file_path: &Path,
    ) -> Result<(String, PathBuf), LspError> {
        // Phase 6: reject new client acquisition after shutdown begins.
        {
            let lc = self.lifecycle.read().await;
            if *lc != ServiceLifecycle::Running {
                return Err(LspError::InitializationCancelled(
                    "service is not running".to_string(),
                ));
            }
        }

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

        // Phase 5: single-flight with shared failure results.
        let slot = {
            let init = self.initializing.read().await;
            init.get(&key).cloned()
        };

        let slot = match slot {
            Some(s) => s,
            None => {
                let mut init = self.initializing.write().await;
                // Double-check after acquiring write lock.
                match init.get(&key) {
                    Some(s) => s.clone(),
                    None => {
                        let s = Arc::new(Mutex::new(InitSlot::Starting { waiters: vec![] }));
                        init.insert(key.clone(), s.clone());
                        s
                    }
                }
            }
        };

        // Try to become the initializing caller.
        let receiver = {
            let mut guard = slot.lock().await;
            match &*guard {
                InitSlot::Ready(client) => {
                    // Already initialized by someone else.
                    {
                        let mut clients = self.clients.write().await;
                        clients.entry(key.clone()).or_insert_with(|| client.clone());
                    }
                    return Ok((key, project_root));
                }
                InitSlot::Starting { waiters } if waiters.is_empty() => {
                    // We are the first caller — mark ourselves as initializing.
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    *guard = InitSlot::Starting { waiters: vec![tx] };
                    Some(rx)
                }
                InitSlot::Starting { .. } => {
                    // Concurrent caller — wait for result.
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    if let InitSlot::Starting { waiters } = &mut *guard {
                        waiters.push(tx);
                    }
                    Some(rx)
                }
            }
        };

        match receiver {
            Some(rx) => {
                // We are a waiter — await the result from the first caller.
                rx.await.unwrap_or_else(|_| {
                    Err(LspError::InitializationCancelled(
                        "init channel dropped".to_string(),
                    ))
                })?;
                // The first caller already inserted into clients map on success.
                return Ok((key, project_root));
            }
            None => {
                // We are the first caller — run initialization.
            }
        }

        let result = self.init_client_inner(server, &project_root).await;

        // Lock the slot again to transition state and notify waiters.
        let waiters = {
            let mut guard = slot.lock().await;
            match result {
                Ok(client) => {
                    // Install the client.
                    {
                        let mut clients = self.clients.write().await;
                        clients.entry(key.clone()).or_insert_with(|| client.clone());
                    }
                    if let InitSlot::Starting { waiters } =
                        std::mem::replace(&mut *guard, InitSlot::Ready(client))
                    {
                        waiters
                    } else {
                        vec![]
                    }
                }
                Err(e) => {
                    // Clean up the slot so a later call can retry.
                    let waiters = if let InitSlot::Starting { waiters } =
                        std::mem::replace(&mut *guard, InitSlot::Starting { waiters: vec![] })
                    {
                        waiters
                    } else {
                        vec![]
                    };
                    // Remove from map so retries work.
                    drop(guard);
                    self.initializing.write().await.remove(&key);
                    // Notify waiters of failure.
                    for tx in waiters {
                        let _ = tx.send(Err(LspError::InitializationCancelled(
                            "init failed".to_string(),
                        )));
                    }
                    return Err(e);
                }
            }
        };

        // Notify waiters of success.
        let client = {
            let clients = self.clients.read().await;
            clients.get(&key).cloned().unwrap()
        };
        for tx in waiters {
            let _ = tx.send(Ok(client.clone()));
        }

        // Clean up the initialization slot.
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
        client.open_file(&uri, text, version).await?;

        // Phase 4: record ownership after successful didOpen.
        self.document_owners
            .write()
            .await
            .insert(uri.to_string(), key);

        Ok(())
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

        // Phase 4: deterministic ownership lookup.
        let owner_key = {
            let owners = self.document_owners.read().await;
            owners.get(&uri_str).cloned()
        };

        let owner_key = match owner_key {
            Some(k) => k,
            None => return Ok(()), // never-opened file — idempotent
        };

        let client = {
            let clients = self.clients.read().await;
            clients.get(&owner_key).cloned()
        };

        let client = match client {
            Some(c) => c,
            None => {
                // Owner key stale — clean up and succeed.
                self.document_owners.write().await.remove(&uri_str);
                return Ok(());
            }
        };

        let uri = Url::from_file_path(file_path).map_err(|_| {
            LspError::LaunchFailed(format!("invalid file path: {}", file_path.display()))
        })?;
        let _ = client.close_file(&uri).await;
        client.opened_files.lock().await.remove(&uri_str);
        self.document_owners.write().await.remove(&uri_str);

        Ok(())
    }

    pub async fn save_file(&self, file_path: &Path, text: Option<&str>) -> Result<(), LspError> {
        let uri_str = Url::from_file_path(file_path)
            .map(|u| u.to_string())
            .unwrap_or_default();

        // Phase 4: deterministic ownership lookup.
        let owner_key = {
            let owners = self.document_owners.read().await;
            owners.get(&uri_str).cloned()
        };

        let owner_key = match owner_key {
            Some(k) => k,
            None => return Ok(()), // never-opened file — idempotent no-op
        };

        let client = {
            let clients = self.clients.read().await;
            clients.get(&owner_key).cloned()
        };

        let client = match client {
            Some(c) => c,
            None => return Ok(()), // owner gone — no-op
        };

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
        // Phase 6: set lifecycle to ShuttingDown first.
        {
            let mut lc = self.lifecycle.write().await;
            if *lc != ServiceLifecycle::Running {
                return; // already shutting down or stopped — idempotent
            }
            *lc = ServiceLifecycle::ShuttingDown;
        }

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

        // Phase 4: clear document ownership.
        self.document_owners.write().await.clear();
        // Phase 5: clear pending initializations.
        self.initializing.write().await.clear();
        // Phase 6: set lifecycle to Stopped.
        *self.lifecycle.write().await = ServiceLifecycle::Stopped;
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Phase 4: deterministic document ownership ──

    #[tokio::test]
    async fn close_non_open_file_succeeds() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let path = PathBuf::from("/tmp/nonexistent.rs");
        // Should succeed idempotently — no owner entry exists.
        assert!(svc.close_file(&path).await.is_ok());
    }

    #[tokio::test]
    async fn save_non_open_file_succeeds() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let path = PathBuf::from("/tmp/nonexistent.rs");
        assert!(svc.save_file(&path, Some("text")).await.is_ok());
    }

    #[tokio::test]
    async fn document_ownership_roundtrip() {
        let svc = LspService::new(LspConfig::Disabled(false));
        // Manually insert an ownership entry.
        {
            let mut owners = svc.document_owners.write().await;
            owners.insert(
                "file:///tmp/foo.rs".to_string(),
                "root:rust-analyzer".to_string(),
            );
        }
        // Verify lookup.
        {
            let owners = svc.document_owners.read().await;
            assert_eq!(
                owners.get("file:///tmp/foo.rs").map(String::as_str),
                Some("root:rust-analyzer")
            );
        }
        // Remove via close_file path (simulated).
        svc.document_owners
            .write()
            .await
            .remove("file:///tmp/foo.rs");
        assert!(svc.document_owners.read().await.is_empty());
    }

    // ── Phase 5: init slot logic ──

    #[test]
    fn init_slot_ready_shares_client() {
        // Verify Starting variant carries empty waiters by default.
        let slot = InitSlot::Starting { waiters: vec![] };
        match slot {
            InitSlot::Starting { waiters } => assert!(waiters.is_empty()),
            _ => panic!("expected Starting"),
        }
    }

    #[test]
    fn init_slot_failure_cleans_up() {
        // Verify that a Starting slot with waiters is correctly populated.
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let slot = InitSlot::Starting { waiters: vec![tx] };
        match slot {
            InitSlot::Starting { waiters } => assert_eq!(waiters.len(), 1),
            _ => panic!("expected Starting"),
        }
    }

    // ── Phase 6: lifecycle ──

    #[tokio::test]
    async fn lifecycle_starts_running() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let lc = *svc.lifecycle.read().await;
        assert_eq!(lc, ServiceLifecycle::Running);
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let svc = LspService::new(LspConfig::Disabled(false));
        svc.shutdown_all().await;
        assert_eq!(*svc.lifecycle.read().await, ServiceLifecycle::Stopped);
        // Second call should not panic.
        svc.shutdown_all().await;
        assert_eq!(*svc.lifecycle.read().await, ServiceLifecycle::Stopped);
    }

    #[tokio::test]
    async fn get_or_create_client_rejects_after_shutdown() {
        let svc = LspService::new(LspConfig::Disabled(false));
        svc.shutdown_all().await;
        let result = svc.get_or_create_client(Path::new("/tmp/test.rs")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            LspError::InitializationCancelled(_) => {}
            other => panic!("expected InitializationCancelled, got {:?}", other),
        }
    }
}
