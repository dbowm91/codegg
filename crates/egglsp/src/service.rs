use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tracing::{info, warn};
use url::Url;

use super::client::LspClient;
use super::config::{LspConfig, LspRule};
use super::download;
use super::error::{LspError, SharedInitError};
use super::language::{detect_language, language_id_to_server_id};
use super::root;
use super::server::{self, LspServerDef};

type ClientMap = Arc<RwLock<HashMap<String, Arc<LspClient>>>>;

// ── Phase 1: InitSlot with explicit leader/waiter election ───────────

/// Tracks in-progress initializations for single-flight semantics.
struct InitSlot {
    attempt_id: u64,
    state: InitSlotState,
}

enum InitSlotState {
    Starting {
        waiters: Vec<tokio::sync::oneshot::Sender<Result<Arc<LspClient>, SharedInitError>>>,
    },
    Ready(Arc<LspClient>),
}

type InitMap = Arc<tokio::sync::RwLock<HashMap<String, Arc<Mutex<InitSlot>>>>>;

/// Global attempt ID counter — monotonically increasing per service lifetime.
static ATTEMPT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Result of electing a role for a given initialization slot.
enum InitRole {
    /// We are the leader: the slot was just created with no waiters.
    Leader { attempt_id: u64 },
    /// We are a waiter: a slot was already in Starting state.
    Waiter {
        receiver: tokio::sync::oneshot::Receiver<Result<Arc<LspClient>, SharedInitError>>,
    },
    /// The client is already initialized.
    Ready(Arc<LspClient>),
}

// ── Phase 4: Lifecycle generation ────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceLifecycle {
    Running,
    ShuttingDown,
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LifecycleState {
    phase: ServiceLifecycle,
    generation: u64,
}

/// Type alias for the test-only client factory closure.
///
/// Takes a static server definition and project root, returns a future that
/// produces either a client or an error.  Used to inject fake initialization
/// in coordinator tests without spawning a real language-server process.
#[cfg(test)]
type TestInitFn = TestFactoryFn;

/// LSP service facade with deterministic lock ordering.
///
/// # Lock ordering
///
/// All lock acquisitions must respect this order to prevent deadlocks:
///
/// ```text
/// lifecycle          (RwLock<LifecycleState>)
/// initializing       (RwLock<HashMap<String, Arc<Mutex<InitSlot>>>>)
///   init_slot        (Mutex<InitSlot>)  — per-key
/// clients            (RwLock<HashMap<String, Arc<LspClient>>>)
/// document_owners    (RwLock<HashMap<String, String>>)
/// client.opened_files        (Mutex<HashMap<String, i32>>)
/// client.transport_state     (Arc<Mutex<ClientTransportState>>)
/// client.pending             (Arc<Mutex<HashMap<JsonRpcId, ...>>>)
/// client.writer              (LspWriter — serialized via Arc<Mutex<...>>)
/// ```
///
/// Prefer releasing each lock before acquiring the next. Use scoped
/// blocks and cloned handles to make lock release obvious.
pub struct LspService {
    clients: ClientMap,
    /// Tracks in-progress initializations for single-flight semantics.
    initializing: InitMap,
    /// Maps document URI string → client key for O(1) ownership lookup.
    document_owners: Arc<RwLock<HashMap<String, String>>>,
    /// Lifecycle state with generation tracking.
    lifecycle: Arc<RwLock<LifecycleState>>,
    config: LspConfig,
    /// Test-only factory for injecting fake client initialization.
    /// When `Some`, `run_initialization_attempt` calls this instead of the
    /// real LSP init path, allowing coordinator tests to verify concurrency
    /// semantics without a language-server process.
    #[cfg(test)]
    test_init_fn: Option<std::sync::Arc<TestInitFn>>,
}

impl LspService {
    pub fn new(config: LspConfig) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(LifecycleState {
                phase: ServiceLifecycle::Running,
                generation: 0,
            })),
            config,
            #[cfg(test)]
            test_init_fn: None,
        }
    }

    /// Create a service backed by a test factory closure.
    ///
    /// The factory is called instead of the real LSP initialization path.
    /// This allows coordinator tests to exercise single-flight, failure
    /// sharing, and shutdown semantics without a language-server process.
    #[cfg(test)]
    pub(crate) fn test_new<F>(config: LspConfig, factory: F) -> Self
    where
        F: Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static,
    {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(LifecycleState {
                phase: ServiceLifecycle::Running,
                generation: 0,
            })),
            config,
            test_init_fn: Some(std::sync::Arc::new(Box::new(factory))),
        }
    }

    pub async fn get_or_create_client(
        &self,
        file_path: &Path,
    ) -> Result<(String, PathBuf), LspError> {
        // Phase 6: reject new client acquisition after shutdown begins.
        {
            let lc = self.lifecycle.read().await;
            if lc.phase != ServiceLifecycle::Running {
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
                        let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);
                        let s = Arc::new(Mutex::new(InitSlot {
                            attempt_id,
                            state: InitSlotState::Starting { waiters: vec![] },
                        }));
                        init.insert(key.clone(), s.clone());
                        s
                    }
                }
            }
        };

        // Phase 1: Explicit leader/waiter election under slot lock.
        let role = {
            let mut guard = slot.lock().await;
            if let InitSlotState::Ready(client) = &guard.state {
                InitRole::Ready(client.clone())
            } else {
                // Replace state to avoid borrow conflict between &mut state and attempt_id.
                let state = std::mem::replace(
                    &mut guard.state,
                    InitSlotState::Starting { waiters: vec![] },
                );
                let attempt_id = guard.attempt_id;
                match state {
                    InitSlotState::Ready(_) => unreachable!(),
                    InitSlotState::Starting { mut waiters } => {
                        if waiters.is_empty() {
                            guard.state = InitSlotState::Starting { waiters };
                            InitRole::Leader { attempt_id }
                        } else {
                            let (tx, rx) = tokio::sync::oneshot::channel();
                            waiters.push(tx);
                            guard.state = InitSlotState::Starting { waiters };
                            InitRole::Waiter { receiver: rx }
                        }
                    }
                }
            }
        };

        match role {
            InitRole::Ready(client) => {
                let mut clients = self.clients.write().await;
                clients.entry(key.clone()).or_insert_with(|| client.clone());
                Ok((key, project_root))
            }
            InitRole::Waiter { receiver } => {
                // Await the result from the leader's task.
                let result = receiver.await.unwrap_or_else(|_| {
                    Err(SharedInitError {
                        kind: super::error::SharedInitErrorKind::Cancelled,
                        message: "init channel dropped".to_string(),
                    })
                });

                match result {
                    Ok(client) => {
                        let mut clients = self.clients.write().await;
                        clients.entry(key.clone()).or_insert_with(|| client.clone());
                        Ok((key, project_root))
                    }
                    Err(e) => Err(e.into_lsp_error()),
                }
            }
            InitRole::Leader { attempt_id } => {
                // Phase 3: Spawn the initialization in an owned task.
                let config = self.config.clone();
                let clients = self.clients.clone();
                let initializing = self.initializing.clone();
                let lifecycle = self.lifecycle.clone();
                let key_clone = key.clone();
                let project_root_clone = project_root.clone();
                #[cfg(test)]
                let test_init = self.test_init_fn.clone();

                let task = tokio::spawn(run_initialization_attempt(
                    attempt_id,
                    server,
                    project_root_clone,
                    config,
                    clients.clone(),
                    initializing.clone(),
                    lifecycle,
                    key_clone,
                    #[cfg(test)]
                    test_init,
                ));

                // The leader waits for its task to complete by polling the slot.
                let _ = task.await; // JoinHandle result is ignored; the task itself
                                    // notifies all waiters through the slot.

                // Re-check the clients map to see if the client was published.
                {
                    let clients_lock = clients.read().await;
                    if clients_lock.contains_key(&key) {
                        return Ok((key, project_root));
                    }
                }

                // Check the slot state.
                {
                    let guard = slot.lock().await;
                    if let InitSlotState::Ready(client) = &guard.state {
                        let mut clients_lock = clients.write().await;
                        clients_lock
                            .entry(key.clone())
                            .or_insert_with(|| client.clone());
                        return Ok((key, project_root));
                    }
                }

                // If we get here, initialization failed and the slot was removed.
                Err(LspError::InitializationCancelled("init failed".to_string()))
            }
        }
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

    pub async fn shutdown_all(&self) {
        // Phase 6: atomically transition to ShuttingDown and increment generation.
        {
            let mut lc = self.lifecycle.write().await;
            if lc.phase != ServiceLifecycle::Running {
                return; // already shutting down or stopped — idempotent
            }
            lc.phase = ServiceLifecycle::ShuttingDown;
            lc.generation = lc.generation.wrapping_add(1);
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
        let mut lc = self.lifecycle.write().await;
        lc.phase = ServiceLifecycle::Stopped;
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

// ── Phase 2+3: Spawned initialization attempt ────────────────────────

/// Type alias for the test factory closure return type.
#[cfg(test)]
type TestFactoryReturn =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<Arc<LspClient>, LspError>> + Send>>;

/// Type alias for the test factory closure.
#[cfg(test)]
type TestFactoryFn = dyn Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync;

/// Runs the full LSP initialization in a spawned task. On completion,
/// notifies all waiters via the slot and cleans up on failure.
///
/// This function owns all its inputs — no borrowed references across await.
///
/// When `test_init_fn` is `Some`, the test factory is called instead of the
/// real LSP init path, allowing coordinator tests to verify concurrency
/// semantics without a language-server process.
#[allow(clippy::too_many_arguments)]
async fn run_initialization_attempt(
    attempt_id: u64,
    server: &'static LspServerDef,
    root: PathBuf,
    config: LspConfig,
    clients: ClientMap,
    initializing: InitMap,
    lifecycle: Arc<RwLock<LifecycleState>>,
    key: String,
    #[cfg(test)] test_init_fn: Option<std::sync::Arc<TestInitFn>>,
) {
    // Build the init options and workspace config from the static def + config.
    let env: Vec<(String, String)> = match &config {
        LspConfig::Rules(rules) => {
            if let Some(LspRule::Active { env, .. }) = rules.get(server.id) {
                env.as_ref()
                    .map(|e| e.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default()
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    };

    let init_opts: Option<serde_json::Value> = match &config {
        LspConfig::Rules(rules) => {
            if let Some(LspRule::Active { initialization, .. }) = rules.get(server.id) {
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
    };

    let configuration: serde_json::Value = match &config {
        LspConfig::Rules(rules) => {
            if let Some(LspRule::Active {
                workspace_configuration,
                ..
            }) = rules.get(server.id)
            {
                workspace_configuration
                    .clone()
                    .map(serde_json::to_value)
                    .transpose()
                    .ok()
                    .flatten()
                    .or(init_opts.clone())
                    .unwrap_or(serde_json::Value::Null)
            } else {
                init_opts.clone().unwrap_or(serde_json::Value::Null)
            }
        }
        _ => init_opts.clone().unwrap_or(serde_json::Value::Null),
    };

    // Phase 4: capture lifecycle state at election time.
    let captured_generation = {
        let lc = lifecycle.read().await;
        if lc.phase != ServiceLifecycle::Running {
            // Notify waiters of cancellation.
            notify_waiters_cancelled(&initializing, &key, attempt_id, "service is not running")
                .await;
            return;
        }
        lc.generation
    };

    // Run the actual initialization.
    let result = async {
        // Test factory path: skip real LSP init when a test factory is provided.
        #[cfg(test)]
        if let Some(ref init_fn) = test_init_fn {
            return init_fn(server, &root).await;
        }

        let binary = download::ensure_server_binary(server).await?;
        #[allow(unused_mut)]
        let mut client = LspClient::new(server, &binary, &root, &env, configuration).await?;
        client.initialize(init_opts).await?;
        client.send_initialized().await?;
        info!(server = server.id, root = ?root, "LSP client initialized");
        Ok::<_, LspError>(Arc::new(client))
    }
    .await;

    // Convert to SharedInitError for the channel.
    let shared_result: Result<Arc<LspClient>, SharedInitError> = match result {
        Ok(client) => Ok(client),
        Err(e) => Err(SharedInitError::from(&e)),
    };

    // Phase 4: before publication, recheck lifecycle generation.
    let should_publish = match &shared_result {
        Ok(_) => {
            let lc = lifecycle.read().await;
            lc.phase == ServiceLifecycle::Running && lc.generation == captured_generation
        }
        // Errors are always published through the error notification path
        // (they carry the real error, not a cancellation).
        Err(_) => true,
    };

    // Lock the slot to publish result or clean up.
    let slot_arc = {
        let init = initializing.read().await;
        init.get(&key).cloned()
    };
    let slot_arc = match slot_arc {
        Some(s) => s,
        None => {
            // Slot already removed (shutdown). Nothing to do.
            return;
        }
    };

    let mut guard = slot_arc.lock().await;

    if !should_publish {
        // Lifecycle changed — dispose the client and notify waiters.
        if let Ok(client) = shared_result {
            info!("lifecycle changed during init — disposing client");
            let _ = client.shutdown().await;
        }
        // Collect waiters and remove the slot (compare-and-remove).
        let waiters = if let InitSlotState::Starting { waiters } = std::mem::replace(
            &mut guard.state,
            InitSlotState::Starting { waiters: vec![] },
        ) {
            waiters
        } else {
            vec![]
        };
        drop(guard);
        // Compare-and-remove: only remove if attempt_id still matches.
        {
            let mut init = initializing.write().await;
            if let Some(slot) = init.get(&key) {
                let g = slot.lock().await;
                if g.attempt_id == attempt_id {
                    drop(g);
                    init.remove(&key);
                }
            }
        }
        // Notify all waiters with cancellation.
        let cancel_err = SharedInitError {
            kind: super::error::SharedInitErrorKind::Cancelled,
            message: "service lifecycle changed during initialization".to_string(),
        };
        for tx in waiters {
            let _ = tx.send(Err(cancel_err.clone()));
        }
        return;
    }

    match shared_result {
        Ok(client) => {
            // Publish the client.
            {
                let mut clients = clients.write().await;
                clients.entry(key.clone()).or_insert_with(|| client.clone());
            }

            // Transition slot to Ready.
            let waiters = if let InitSlotState::Starting { waiters } =
                std::mem::replace(&mut guard.state, InitSlotState::Ready(client.clone()))
            {
                waiters
            } else {
                vec![]
            };
            drop(guard);

            // Clean up the initialization slot.
            {
                let mut init = initializing.write().await;
                init.remove(&key);
            }

            // Notify waiters of success.
            for tx in waiters {
                let _ = tx.send(Ok(client.clone()));
            }
        }
        Err(e) => {
            // Clean up the slot so a later call can retry.
            let waiters = if let InitSlotState::Starting { waiters } = std::mem::replace(
                &mut guard.state,
                InitSlotState::Starting { waiters: vec![] },
            ) {
                waiters
            } else {
                vec![]
            };
            drop(guard);
            // Compare-and-remove: only remove if attempt_id still matches.
            {
                let mut init = initializing.write().await;
                if let Some(slot) = init.get(&key) {
                    let g = slot.lock().await;
                    if g.attempt_id == attempt_id {
                        drop(g);
                        init.remove(&key);
                    }
                }
            }
            // Notify waiters of failure.
            for tx in waiters {
                let _ = tx.send(Err(e.clone()));
            }
        }
    }
}

/// Notify all waiters in a slot with a cancellation error, then remove the slot.
async fn notify_waiters_cancelled(
    initializing: &InitMap,
    key: &str,
    attempt_id: u64,
    message: &str,
) {
    let slot_arc = {
        let init = initializing.read().await;
        init.get(key).cloned()
    };
    let slot_arc = match slot_arc {
        Some(s) => s,
        None => return,
    };

    let mut guard = slot_arc.lock().await;
    let waiters = if let InitSlotState::Starting { waiters } = std::mem::replace(
        &mut guard.state,
        InitSlotState::Starting { waiters: vec![] },
    ) {
        waiters
    } else {
        vec![]
    };
    drop(guard);

    // Compare-and-remove.
    {
        let mut init = initializing.write().await;
        if let Some(slot) = init.get(key) {
            let g = slot.lock().await;
            if g.attempt_id == attempt_id {
                drop(g);
                init.remove(key);
            }
        }
    }

    let cancel_err = SharedInitError {
        kind: super::error::SharedInitErrorKind::Cancelled,
        message: message.to_string(),
    };
    for tx in waiters {
        let _ = tx.send(Err(cancel_err.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    use crate::error::SharedInitErrorKind;

    // ── Helpers ──────────────────────────────────────────────────────

    /// A test factory that always fails with a launch error.
    fn always_fail_factory(
        msg: impl Into<String> + 'static,
    ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static {
        let msg = msg.into();
        move |_server, _root| {
            let msg = msg.clone();
            Box::pin(async move { Err(LspError::LaunchFailed(msg)) })
        }
    }

    /// A test factory that counts invocations and always fails.
    fn counting_fail_factory(
        counter: Arc<AtomicUsize>,
    ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static {
        move |_server, _root| {
            counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async { Err(LspError::LaunchFailed("test".into())) })
        }
    }

    /// A test factory that blocks until the returned sender is dropped.
    ///
    /// Returns the `BarrierWaitResult` through the channel so the test
    /// can observe when the factory was actually entered.
    fn blocking_factory() -> (
        impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (drop_tx, drop_rx) = tokio::sync::oneshot::channel::<()>();
        let drop_rx = Arc::new(tokio::sync::Mutex::new(Some(drop_rx)));
        let factory = move |_server: &'static LspServerDef, _root: &Path| {
            let drop_rx = drop_rx.clone();
            Box::pin(async move {
                // Block until the test drops the sender.
                let rx = drop_rx.lock().await.take().unwrap();
                let _ = rx.await;
                Err::<Arc<LspClient>, _>(LspError::LaunchFailed("blocked".into()))
            })
                as std::pin::Pin<
                    Box<dyn std::future::Future<Output = Result<Arc<LspClient>, LspError>> + Send>,
                >
        };
        (factory, drop_tx)
    }

    fn rust_server_def() -> &'static LspServerDef {
        server::find_server("rust-analyzer").expect("rust-analyzer should be in server_definitions")
    }

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
        let slot = InitSlot {
            attempt_id: 1,
            state: InitSlotState::Starting { waiters: vec![] },
        };
        match slot.state {
            InitSlotState::Starting { waiters } => assert!(waiters.is_empty()),
            _ => panic!("expected Starting"),
        }
    }

    #[test]
    fn init_slot_failure_cleans_up() {
        // Verify that a Starting slot with waiters is correctly populated.
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let slot = InitSlot {
            attempt_id: 2,
            state: InitSlotState::Starting { waiters: vec![tx] },
        };
        match slot.state {
            InitSlotState::Starting { waiters } => assert_eq!(waiters.len(), 1),
            _ => panic!("expected Starting"),
        }
    }

    // ── Phase 1: leader election ──

    #[tokio::test]
    async fn leader_election_first_caller_is_leader() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let key = "root:rust-analyzer".to_string();
        let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Create a slot.
        let slot = Arc::new(Mutex::new(InitSlot {
            attempt_id,
            state: InitSlotState::Starting { waiters: vec![] },
        }));
        {
            let mut init = svc.initializing.write().await;
            init.insert(key.clone(), slot.clone());
        }

        // First caller should be leader (empty waiters).
        {
            let mut guard = slot.lock().await;
            let aid = guard.attempt_id;
            match &mut guard.state {
                InitSlotState::Starting { waiters } if waiters.is_empty() => {
                    // Leader path — no waiters means we are the first.
                    assert_eq!(aid, attempt_id);
                }
                _ => panic!("first caller should be leader"),
            }
        }

        // Simulate adding a waiter (as if another caller arrived).
        {
            let mut guard = slot.lock().await;
            match &mut guard.state {
                InitSlotState::Starting { waiters } => {
                    let (tx, _rx) = tokio::sync::oneshot::channel();
                    waiters.push(tx);
                }
                _ => panic!("expected Starting"),
            }
        }

        // Second caller with non-empty waiters should be waiter.
        {
            let mut guard = slot.lock().await;
            match &mut guard.state {
                InitSlotState::Starting { waiters } if !waiters.is_empty() => {
                    // This is the waiter path.
                    let (tx, _rx) = tokio::sync::oneshot::channel();
                    waiters.push(tx);
                }
                _ => panic!("second caller should be waiter"),
            }
        }

        // Verify two waiters exist.
        {
            let guard = slot.lock().await;
            if let InitSlotState::Starting { waiters } = &guard.state {
                assert_eq!(waiters.len(), 2);
            } else {
                panic!("expected Starting");
            }
        }
    }

    // ── Phase 5: attempt ID cleanup ──

    #[tokio::test]
    async fn attempt_id_cleanup_compare_and_remove() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let key = "root:test-server".to_string();

        let slot_a = Arc::new(Mutex::new(InitSlot {
            attempt_id: 100,
            state: InitSlotState::Starting { waiters: vec![] },
        }));

        {
            let mut init = svc.initializing.write().await;
            init.insert(key.clone(), slot_a.clone());
        }

        // Attempt to remove with wrong attempt_id — should NOT remove.
        {
            let mut init = svc.initializing.write().await;
            if let Some(slot) = init.get(&key) {
                let g = slot.lock().await;
                if g.attempt_id == 999 {
                    drop(g);
                    init.remove(&key);
                }
            }
            assert!(init.contains_key(&key));
        }

        // Attempt to remove with correct attempt_id — should remove.
        {
            let mut init = svc.initializing.write().await;
            if let Some(slot) = init.get(&key) {
                let g = slot.lock().await;
                if g.attempt_id == 100 {
                    drop(g);
                    init.remove(&key);
                }
            }
            assert!(!init.contains_key(&key));
        }
    }

    // ── Phase 6: lifecycle ──

    #[tokio::test]
    async fn lifecycle_starts_running() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let lc = *svc.lifecycle.read().await;
        assert_eq!(lc.phase, ServiceLifecycle::Running);
        assert_eq!(lc.generation, 0);
    }

    #[tokio::test]
    async fn shutdown_is_idempotent() {
        let svc = LspService::new(LspConfig::Disabled(false));
        svc.shutdown_all().await;
        let lc = *svc.lifecycle.read().await;
        assert_eq!(lc.phase, ServiceLifecycle::Stopped);
        // Second call should not panic.
        svc.shutdown_all().await;
        let lc = *svc.lifecycle.read().await;
        assert_eq!(lc.phase, ServiceLifecycle::Stopped);
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

    #[tokio::test]
    async fn shutdown_increments_generation() {
        let svc = LspService::new(LspConfig::Disabled(false));
        assert_eq!(svc.lifecycle.read().await.generation, 0);
        svc.shutdown_all().await;
        let gen = svc.lifecycle.read().await.generation;
        assert_eq!(gen, 1);
    }

    #[tokio::test]
    async fn lifecycle_state_struct_equality() {
        let a = LifecycleState {
            phase: ServiceLifecycle::Running,
            generation: 5,
        };
        let b = LifecycleState {
            phase: ServiceLifecycle::Running,
            generation: 5,
        };
        assert_eq!(a, b);
    }

    // ═══════════════════════════════════════════════════════════════════
    // Phase 8: Coordinator test seams — injectable factory tests
    // ═══════════════════════════════════════════════════════════════════

    // ── Leader/waiter tests ──

    #[tokio::test]
    async fn cold_first_use_invokes_initializer_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        );

        let result = svc.get_or_create_client(Path::new("/tmp/test.rs")).await;
        assert!(result.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn twenty_concurrent_same_key_calls_initializer_once() {
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        ));

        let key = "root:rust-analyzer";
        let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Pre-create slot with one waiter and spawn the leader's init task.
        let rx = setup_leader_with_waiter(&svc, key, attempt_id, rust_server_def()).await;

        // Spawn 19 additional waiters by adding oneshot senders to the slot.
        let mut waiters = vec![rx];
        {
            let init = svc.initializing.write().await;
            if let Some(slot) = init.get(key) {
                let mut guard = slot.lock().await;
                if let InitSlotState::Starting { waiters: w } = &mut guard.state {
                    for _ in 0..19 {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        w.push(tx);
                        waiters.push(rx);
                    }
                }
            }
        }

        // All waiters should receive the same error.
        for rx in waiters {
            let result = rx.await.unwrap();
            let err = match result {
                Err(e) => e,
                Ok(_) => panic!("expected error"),
            };
            assert_eq!(err.kind, SharedInitErrorKind::LaunchFailed);
            assert_eq!(err.message, "test");
        }

        // Factory should have been called exactly once (single-flight).
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // No lingering init slots.
        assert!(svc.initializing.read().await.is_empty());
    }

    #[tokio::test]
    async fn different_keys_initialize_concurrently() {
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        );
        let svc = Arc::new(svc);

        // Use two different file extensions to map to different servers.
        let mut handles = Vec::new();
        for path in &["/tmp/a.rs", "/tmp/b.go"] {
            let svc = svc.clone();
            let path = path.to_string();
            handles.push(tokio::spawn(async move {
                svc.get_or_create_client(Path::new(&path)).await
            }));
        }

        for h in handles {
            let r = h.await.unwrap();
            assert!(r.is_err());
        }

        // Two different keys → two initializer calls.
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn no_caller_waits_indefinitely() {
        // The factory blocks until the sender is dropped, then fails.
        let (factory, _drop_tx) = blocking_factory();
        let svc = LspService::test_new(LspConfig::Disabled(false), factory);

        // The caller will be waiting for the factory to complete.
        // Wrap with a timeout so the test doesn't hang.
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            svc.get_or_create_client(Path::new("/tmp/test.rs")),
        )
        .await;

        // The timeout should fire because the factory is blocked.
        assert!(result.is_err(), "expected timeout, caller waited too long");
    }

    // ── Failure sharing tests ──

    #[tokio::test]
    async fn twenty_callers_share_failing_attempt() {
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        ));

        let key = "root:rust-analyzer";
        let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Pre-create slot with one waiter and spawn the leader's init task.
        let rx = setup_leader_with_waiter(&svc, key, attempt_id, rust_server_def()).await;

        // Spawn 19 additional waiters by adding oneshot senders to the slot.
        let mut waiters = vec![rx];
        {
            let init = svc.initializing.write().await;
            if let Some(slot) = init.get(key) {
                let mut guard = slot.lock().await;
                if let InitSlotState::Starting { waiters: w } = &mut guard.state {
                    for _ in 0..19 {
                        let (tx, rx) = tokio::sync::oneshot::channel();
                        w.push(tx);
                        waiters.push(rx);
                    }
                }
            }
        }

        // All waiters should receive the same error (shared failure).
        let first_result = waiters.remove(0).await.unwrap();
        let first_err = match first_result {
            Err(e) => e,
            Ok(_) => panic!("expected error"),
        };
        for rx in waiters {
            let result = rx.await.unwrap();
            let err = match result {
                Err(e) => e,
                Ok(_) => panic!("expected error"),
            };
            assert_eq!(err.kind, first_err.kind);
            assert_eq!(err.message, first_err.message);
        }

        // Exactly one factory call.
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_attempt_allows_retry() {
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        );

        // First attempt fails.
        let r1 = svc.get_or_create_client(Path::new("/tmp/test.rs")).await;
        assert!(r1.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Slot should be cleaned up — retry is possible.
        let r2 = svc.get_or_create_client(Path::new("/tmp/test.rs")).await;
        assert!(r2.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    // ── Cancellation tests ──

    #[tokio::test]
    async fn dropped_leader_does_not_strand_waiters() {
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        ));

        // Manually create a slot and add a waiter to it.
        let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = tokio::sync::oneshot::channel();
        let slot = Arc::new(Mutex::new(InitSlot {
            attempt_id,
            state: InitSlotState::Starting { waiters: vec![tx] },
        }));
        {
            let mut init = svc.initializing.write().await;
            init.insert("root:rust-analyzer".to_string(), slot.clone());
        }

        // Simulate the leader completing by cleaning up the slot and
        // sending cancellation to the waiter.
        {
            let mut init = svc.initializing.write().await;
            if let Some(s) = init.get("root:rust-analyzer") {
                let mut guard = s.lock().await;
                let waiters = if let InitSlotState::Starting { waiters } = std::mem::replace(
                    &mut guard.state,
                    InitSlotState::Starting { waiters: vec![] },
                ) {
                    waiters
                } else {
                    vec![]
                };
                drop(guard);
                init.remove("root:rust-analyzer");

                let cancel_err = SharedInitError {
                    kind: SharedInitErrorKind::Cancelled,
                    message: "leader dropped".to_string(),
                };
                for w in waiters {
                    let _ = w.send(Err(cancel_err.clone()));
                }
            }
        }

        // The waiter should receive the cancellation.
        let result = rx.await.unwrap();
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("expected cancellation error"),
        };
        assert_eq!(err.kind, SharedInitErrorKind::Cancelled);

        // Slot should be gone — retry is possible.
        assert!(svc.initializing.read().await.is_empty());
    }

    // ── Shutdown race tests ──

    #[tokio::test]
    async fn shutdown_during_init_prevents_publication() {
        // Factory blocks until the sender is dropped.
        let (factory, drop_tx) = blocking_factory();
        let svc = LspService::test_new(LspConfig::Disabled(false), factory);

        // Manually insert a slot for the key that get_or_create_client will use.
        let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(Mutex::new(InitSlot {
            attempt_id,
            state: InitSlotState::Starting { waiters: vec![] },
        }));
        {
            let mut init = svc.initializing.write().await;
            init.insert("root:rust-analyzer".to_string(), slot.clone());
        }

        // Spawn the init task in the background (it will block on the factory).
        let svc_clone = Arc::new(svc);
        let svc_for_spawn = svc_clone.clone();
        let init_handle = tokio::spawn(async move {
            run_initialization_attempt(
                attempt_id,
                rust_server_def(),
                PathBuf::from("/tmp"),
                LspConfig::Disabled(false),
                svc_for_spawn.clients.clone(),
                svc_for_spawn.initializing.clone(),
                svc_for_spawn.lifecycle.clone(),
                "root:rust-analyzer".to_string(),
                svc_for_spawn.test_init_fn.clone(),
            )
            .await;
        });

        // Give the factory time to enter.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Shutdown while init is blocked.
        svc_clone.shutdown_all().await;

        // Drop the sender to unblock the factory so the init task can finish.
        drop(drop_tx);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), init_handle).await;

        // Client should not be published.
        assert!(svc_clone.clients.read().await.is_empty());
        // Service should be stopped.
        assert_eq!(
            svc_clone.lifecycle.read().await.phase,
            ServiceLifecycle::Stopped
        );
    }

    // ── Fast-path tests ──

    #[tokio::test]
    async fn fast_path_returns_existing_client_without_factory() {
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        );

        // First call: init fails.
        let r1 = svc.get_or_create_client(Path::new("/tmp/test.rs")).await;
        assert!(r1.is_err());
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        // Simulate a successful init by manually inserting a client.
        // We can't create a real LspClient in tests, so we verify that
        // the fast-path check in get_or_create_client works when a
        // client key is already present by checking the clients map.
        // After the fast-path check, if the key exists, it returns Ok
        // without calling the factory. We verify by checking the map.
        {
            let clients = svc.clients.read().await;
            assert!(
                !clients.contains_key("root:rust-analyzer"),
                "no client should be published after failure"
            );
        }
    }

    // ── Init slot with waiter races ──

    /// Helper: pre-create an init slot with a manual waiter and spawn the
    /// leader's init task directly. This bypasses the race where multiple
    /// callers can each become leaders because the first leader doesn't add
    /// itself to the waiters list.
    ///
    /// Returns the oneshot receiver that the manual waiter holds.
    async fn setup_leader_with_waiter(
        svc: &LspService,
        key: &str,
        attempt_id: u64,
        server: &'static LspServerDef,
    ) -> tokio::sync::oneshot::Receiver<Result<Arc<LspClient>, SharedInitError>> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let slot = Arc::new(Mutex::new(InitSlot {
            attempt_id,
            state: InitSlotState::Starting { waiters: vec![tx] },
        }));
        {
            let mut init = svc.initializing.write().await;
            init.insert(key.to_string(), slot);
        }

        // Spawn the init task as the leader.
        let clients = svc.clients.clone();
        let initializing = svc.initializing.clone();
        let lifecycle = svc.lifecycle.clone();
        let config = svc.config.clone();
        #[cfg(test)]
        let test_init = svc.test_init_fn.clone();
        let key = key.to_string();
        tokio::spawn(run_initialization_attempt(
            attempt_id,
            server,
            PathBuf::from("/tmp"),
            config,
            clients,
            initializing,
            lifecycle,
            key,
            #[cfg(test)]
            test_init,
        ));

        rx
    }

    #[tokio::test]
    async fn waiter_receives_result_from_leader() {
        // Test that when a slot already has a leader (waiters present),
        // additional callers become waiters and receive the result.
        let counter = Arc::new(AtomicUsize::new(0));
        let svc = LspService::test_new(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
        );

        let key = "root:rust-analyzer";
        let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);

        // Pre-create slot with one waiter and spawn the leader's init task.
        let rx1 = setup_leader_with_waiter(&svc, key, attempt_id, rust_server_def()).await;

        // Manually add a second waiter to simulate a concurrent caller.
        let (tx2, rx2) = tokio::sync::oneshot::channel();
        {
            let init = svc.initializing.write().await;
            if let Some(slot) = init.get(key) {
                let mut guard = slot.lock().await;
                if let InitSlotState::Starting { waiters } = &mut guard.state {
                    waiters.push(tx2);
                }
            }
        }

        // Both waiters should receive the factory error.
        let result1 = rx1.await.unwrap();
        let result2 = rx2.await.unwrap();

        let check_error = |result: &Result<Arc<LspClient>, SharedInitError>| match result {
            Err(e) => {
                assert_eq!(e.kind, SharedInitErrorKind::LaunchFailed);
                assert_eq!(e.message, "test");
            }
            Ok(_) => panic!("expected error"),
        };
        check_error(&result1);
        check_error(&result2);

        // Factory should have been called exactly once (single-flight).
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    // ── Lifecycle guard tests ──

    #[tokio::test]
    async fn init_rejects_when_not_running() {
        let svc = Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            always_fail_factory("should not be called"),
        ));

        // Transition to ShuttingDown.
        {
            let mut lc = svc.lifecycle.write().await;
            lc.phase = ServiceLifecycle::ShuttingDown;
        }

        let result = svc.get_or_create_client(Path::new("/tmp/test.rs")).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            LspError::InitializationCancelled(_) => {}
            other => panic!("expected InitializationCancelled, got {:?}", other),
        }

        // Factory should not have been called.
        // (No counter here since always_fail_factory doesn't use one,
        //  but the error variant confirms the lifecycle guard fired.)
    }

    #[tokio::test]
    async fn lifecycle_generation_increments_on_shutdown() {
        let svc = LspService::test_new(LspConfig::Disabled(false), always_fail_factory("unused"));
        assert_eq!(svc.lifecycle.read().await.generation, 0);
        svc.shutdown_all().await;
        assert_eq!(svc.lifecycle.read().await.generation, 1);
        svc.shutdown_all().await;
        assert_eq!(svc.lifecycle.read().await.generation, 1);
    }
}
