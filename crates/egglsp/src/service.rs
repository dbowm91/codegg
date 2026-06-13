use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use tracing::{debug, info, warn};
use url::Url;

use super::client::LspClient;
use super::config::{LspConfig, LspRule};
use super::download;
use super::error::{LspError, SharedInitError};
use super::language::{detect_language, language_id_to_server_id};
use super::root;
use super::server::{self, LspServerDef};

type ClientMap = Arc<RwLock<HashMap<String, Arc<LspClient>>>>;

type InitResult = Result<Arc<LspClient>, SharedInitError>;
type InitCompletionSender = tokio::sync::oneshot::Sender<InitResult>;
type InitCompletionReceiver = tokio::sync::oneshot::Receiver<InitResult>;

// ── Phase 1: InitSlot with explicit leader/waiter election ───────────

/// Tracks an in-progress initialization attempt for single-flight semantics.
struct InitSlot {
    attempt_id: u64,
    leader: InitCompletionSender,
    waiters: Vec<InitCompletionSender>,
}

impl InitSlot {
    fn into_senders(self) -> Vec<InitCompletionSender> {
        let mut senders = Vec::with_capacity(1 + self.waiters.len());
        senders.push(self.leader);
        senders.extend(self.waiters);
        senders
    }
}

type InitMap = Arc<Mutex<HashMap<String, InitSlot>>>;

/// Global attempt ID counter — monotonically increasing per service lifetime.
static ATTEMPT_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Result of electing a role for a given initialization slot.
enum InitRole {
    /// We are the leader: the slot was just created for this attempt.
    Leader {
        attempt_id: u64,
        completion: InitCompletionReceiver,
    },
    /// We are a waiter: a slot was already running.
    Waiter { completion: InitCompletionReceiver },
}

#[cfg(test)]
struct TestPauseGate {
    entered: tokio::sync::watch::Sender<bool>,
    release: std::sync::Arc<tokio::sync::Notify>,
}

#[cfg(test)]
struct TestHooks {
    leader_spawn_gate: Option<std::sync::Arc<TestPauseGate>>,
    shutdown_gate: Option<std::sync::Arc<TestPauseGate>>,
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
/// clients            (RwLock<HashMap<String, Arc<LspClient>>>)
/// initializing       (Mutex<HashMap<String, InitSlot>>)
/// document_owners    (RwLock<HashMap<String, String>>)
/// client.opened_files        (Mutex<HashMap<String, i32>>)
/// client.transport_state     (Arc<Mutex<ClientTransportState>>)
/// client.pending             (Arc<Mutex<HashMap<JsonRpcId, ...>>>)
/// client.writer              (LspWriter — serialized via Arc<Mutex<...>>)
/// ```
///
/// Coordinator paths hold the client map lock through slot election to
/// keep publication and slot creation atomic. No client/process I/O occurs
/// while lifecycle, client-map, or initialization-map locks are held.
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
    #[cfg(test)]
    test_hooks: Option<std::sync::Arc<TestHooks>>,
}

impl LspService {
    pub fn new(config: LspConfig) -> Self {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(LifecycleState {
                phase: ServiceLifecycle::Running,
                generation: 0,
            })),
            config,
            #[cfg(test)]
            test_init_fn: None,
            #[cfg(test)]
            test_hooks: None,
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
            initializing: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(LifecycleState {
                phase: ServiceLifecycle::Running,
                generation: 0,
            })),
            config,
            test_init_fn: Some(std::sync::Arc::new(Box::new(factory))),
            test_hooks: None,
        }
    }

    #[cfg(test)]
    fn test_new_with_hooks<F>(
        config: LspConfig,
        factory: F,
        test_hooks: std::sync::Arc<TestHooks>,
    ) -> Self
    where
        F: Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static,
    {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(LifecycleState {
                phase: ServiceLifecycle::Running,
                generation: 0,
            })),
            config,
            test_init_fn: Some(std::sync::Arc::new(Box::new(factory))),
            test_hooks: Some(test_hooks),
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

        // Fast path and slot election share the same client-map lock so
        // that publication cannot race with slot creation.
        let role = {
            let clients = self.clients.write().await;
            if clients.contains_key(&key) {
                return Ok((key, project_root));
            }

            let mut init = self.initializing.lock().await;
            match init.get_mut(&key) {
                Some(slot) => {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    slot.waiters.push(tx);
                    InitRole::Waiter { completion: rx }
                }
                None => {
                    let attempt_id = ATTEMPT_COUNTER.fetch_add(1, Ordering::Relaxed);
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    init.insert(
                        key.clone(),
                        InitSlot {
                            attempt_id,
                            leader: tx,
                            waiters: vec![],
                        },
                    );
                    InitRole::Leader {
                        attempt_id,
                        completion: rx,
                    }
                }
            }
        };

        match role {
            InitRole::Waiter { completion } => {
                let result = completion.await.unwrap_or_else(|_| {
                    Err(SharedInitError {
                        kind: super::error::SharedInitErrorKind::Cancelled,
                        message: "init channel dropped".to_string(),
                    })
                });

                match result {
                    Ok(_client) => Ok((key, project_root)),
                    Err(e) => Err(e.into_lsp_error()),
                }
            }
            InitRole::Leader {
                attempt_id,
                completion,
            } => {
                #[cfg(test)]
                if let Some(hooks) = &self.test_hooks {
                    if let Some(gate) = &hooks.leader_spawn_gate {
                        let _ = gate.entered.send(true);
                        gate.release.notified().await;
                    }
                }

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

                let key_for_monitor = key.clone();
                let initializing_for_monitor = initializing.clone();
                tokio::spawn(async move {
                    if let Err(join_err) = task.await {
                        let message = if join_err.is_panic() {
                            format!(
                                "initialization task panicked for {}:{}: {}",
                                key_for_monitor, attempt_id, join_err
                            )
                        } else {
                            format!(
                                "initialization task cancelled for {}:{}: {}",
                                key_for_monitor, attempt_id, join_err
                            )
                        };
                        if let Some(senders) =
                            take_attempt(&initializing_for_monitor, &key_for_monitor, attempt_id)
                                .await
                        {
                            let err = SharedInitError {
                                kind: super::error::SharedInitErrorKind::Other,
                                message,
                            };
                            for tx in senders {
                                let _ = tx.send(Err(err.clone()));
                            }
                        }
                    }
                });

                match completion.await {
                    Ok(Ok(_client)) => Ok((key, project_root)),
                    Ok(Err(e)) => Err(e.into_lsp_error()),
                    Err(_) => Err(LspError::InitializationCancelled(
                        "init channel dropped".to_string(),
                    )),
                }
            }
        }
    }

    pub async fn open_file(&self, file_path: &Path, text: &str) -> Result<(), LspError> {
        let (key, _root) = self.get_or_create_client(file_path).await?;

        let client = {
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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
        #[cfg(test)]
        if let Some(hooks) = &self.test_hooks {
            if let Some(gate) = &hooks.shutdown_gate {
                let _ = gate.entered.send(true);
                gate.release.notified().await;
            }
        }

        // Phase 6: atomically transition to ShuttingDown and increment generation.
        {
            let mut lc = self.lifecycle.write().await;
            if lc.phase != ServiceLifecycle::Running {
                return; // already shutting down or stopped — idempotent
            }
            lc.phase = ServiceLifecycle::ShuttingDown;
            lc.generation = lc.generation.wrapping_add(1);
        }

        // Clear document ownership first so file-level cleanup cannot race
        // with shutting down the client map.
        self.document_owners.write().await.clear();

        let clients_to_shutdown: Vec<(String, Arc<LspClient>)> = {
            let mut clients = self.clients.write().await;
            clients.drain().collect()
        };

        let attempts_to_cancel = drain_attempts(&self.initializing).await;

        for (key, client) in clients_to_shutdown {
            info!(server = %key, "shutting down LSP client");
            if let Err(e) = client.shutdown().await {
                warn!(server = %key, error = %e, "error shutting down LSP client");
            }
        }

        for (key, attempt_id, senders) in attempts_to_cancel {
            info!(server = %key, attempt_id, "cancelling in-flight LSP init during shutdown");
            let cancel_err = SharedInitError {
                kind: super::error::SharedInitErrorKind::Cancelled,
                message: "service is shutting down".to_string(),
            };
            send_completion_result(senders, Err(cancel_err));
        }

        // Phase 6: set lifecycle to Stopped.
        let mut lc = self.lifecycle.write().await;
        lc.phase = ServiceLifecycle::Stopped;
    }

    pub async fn is_file_open(&self, key: &str, uri_str: &str) -> Result<bool, LspError> {
        let client = {
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
            clients
                .get(key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Lock released.
        client.send_request(method, params).await
    }

    pub async fn client_keys(&self) -> Vec<String> {
        let clients = self.clients.write().await;
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
            let clients = self.clients.write().await;
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

fn send_completion_result(senders: Vec<InitCompletionSender>, result: InitResult) {
    for tx in senders {
        let _ = tx.send(result.clone());
    }
}

async fn take_attempt(
    initializing: &InitMap,
    key: &str,
    attempt_id: u64,
) -> Option<Vec<InitCompletionSender>> {
    let mut init = initializing.lock().await;
    let should_remove = match init.get(key) {
        Some(slot) => slot.attempt_id == attempt_id,
        None => false,
    };
    if !should_remove {
        return None;
    }
    init.remove(key).map(InitSlot::into_senders)
}

async fn drain_attempts(initializing: &InitMap) -> Vec<(String, u64, Vec<InitCompletionSender>)> {
    let mut init = initializing.lock().await;
    init.drain()
        .map(|(key, slot)| (key, slot.attempt_id, slot.into_senders()))
        .collect()
}

async fn dispose_unpublished_client(client: Arc<LspClient>, reason: &str) {
    let dispose_result =
        tokio::time::timeout(std::time::Duration::from_secs(2), client.shutdown()).await;

    match dispose_result {
        Ok(Ok(())) => {
            info!(reason, "disposed unpublished LSP client");
        }
        Ok(Err(err)) => {
            warn!(reason, error = %err, "failed to gracefully dispose unpublished LSP client");
        }
        Err(_) => {
            warn!(reason, "timed out disposing unpublished LSP client");
        }
    }
}

/// Runs the full LSP initialization in a spawned task.
///
/// The initialization task is authoritative for publishing results to all
/// callers. Leader and waiters both consume the same completion channel.
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

    let captured_generation = {
        let lc = lifecycle.read().await;
        if lc.phase != ServiceLifecycle::Running {
            if let Some(senders) = take_attempt(&initializing, &key, attempt_id).await {
                let cancel_err = SharedInitError {
                    kind: super::error::SharedInitErrorKind::Cancelled,
                    message: "service is not running".to_string(),
                };
                send_completion_result(senders, Err(cancel_err));
            }
            return;
        }
        lc.generation
    };

    let result = async {
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

    let shared_result = result.map_err(|e| SharedInitError::from(&e));

    match shared_result {
        Ok(client) => {
            let publish_outcome = {
                let lc = lifecycle.read().await;
                let lifecycle_state = *lc;
                if lc.phase != ServiceLifecycle::Running || lc.generation != captured_generation {
                    PublishOutcome::Invalidated(lifecycle_state)
                } else {
                    let mut clients = clients.write().await;
                    match clients.entry(key.clone()) {
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(client.clone());
                            PublishOutcome::Published
                        }
                        std::collections::hash_map::Entry::Occupied(entry) => {
                            PublishOutcome::Existing(entry.get().clone())
                        }
                    }
                }
            };

            let senders = take_attempt(&initializing, &key, attempt_id).await;
            match (publish_outcome, senders) {
                (PublishOutcome::Published, Some(senders)) => {
                    send_completion_result(senders, Ok(client.clone()));
                }
                (PublishOutcome::Existing(existing), Some(senders)) => {
                    let reason =
                        format!("publication lost to existing client for {key}:{attempt_id}");
                    dispose_unpublished_client(client, &reason).await;
                    send_completion_result(senders, Ok(existing));
                }
                (PublishOutcome::Invalidated(lifecycle_state), Some(senders)) => {
                    debug!(
                        server = %key,
                        attempt_id,
                        phase = ?lifecycle_state.phase,
                        generation = lifecycle_state.generation,
                        "publication invalidated before client insertion"
                    );
                    let reason = format!("publication invalidated for {key}:{attempt_id}");
                    dispose_unpublished_client(client, &reason).await;
                    let cancel_err = SharedInitError {
                        kind: super::error::SharedInitErrorKind::Cancelled,
                        message: "service lifecycle changed during initialization".to_string(),
                    };
                    send_completion_result(senders, Err(cancel_err));
                }
                (_, None) => {
                    debug!(
                        server = %key,
                        attempt_id,
                        "successful initialization completed after slot was removed"
                    );
                    let reason = format!("publication slot missing for {key}:{attempt_id}");
                    dispose_unpublished_client(client, &reason).await;
                }
            }
        }
        Err(err) => {
            if let Some(senders) = take_attempt(&initializing, &key, attempt_id).await {
                send_completion_result(senders, Err(err));
            }
        }
    }
}

enum PublishOutcome {
    Published,
    Existing(Arc<LspClient>),
    Invalidated(LifecycleState),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::{watch, Barrier, Notify};

    fn rust_file() -> &'static Path {
        Path::new("/tmp/test.rs")
    }

    fn pause_gate() -> (std::sync::Arc<TestPauseGate>, watch::Receiver<bool>) {
        let (entered, rx) = watch::channel(false);
        (
            std::sync::Arc::new(TestPauseGate {
                entered,
                release: std::sync::Arc::new(Notify::new()),
            }),
            rx,
        )
    }

    enum FactoryOutcome {
        Success,
        LaunchFailed(String),
    }

    struct BlockingFactoryState {
        invocations: AtomicUsize,
        entered: watch::Sender<bool>,
        release: Notify,
        outcome: Mutex<FactoryOutcome>,
        shutdown_count: std::sync::Arc<AtomicUsize>,
    }

    fn blocking_factory(
        state: std::sync::Arc<BlockingFactoryState>,
    ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static {
        move |server, root| {
            let state = state.clone();
            let root = root.to_path_buf();
            Box::pin(async move {
                state.invocations.fetch_add(1, Ordering::SeqCst);
                let _ = state.entered.send(true);
                state.release.notified().await;

                let outcome = {
                    let guard = state.outcome.lock().await;
                    match &*guard {
                        FactoryOutcome::Success => FactoryOutcome::Success,
                        FactoryOutcome::LaunchFailed(msg) => {
                            FactoryOutcome::LaunchFailed(msg.clone())
                        }
                    }
                };

                match outcome {
                    FactoryOutcome::Success => {
                        let client =
                            LspClient::test_stub(server.id, &root, state.shutdown_count.clone())
                                .await?;
                        Ok(Arc::new(client))
                    }
                    FactoryOutcome::LaunchFailed(msg) => Err(LspError::LaunchFailed(msg)),
                }
            })
        }
    }

    fn counting_fail_factory(
        counter: std::sync::Arc<AtomicUsize>,
    ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static {
        move |_server, _root| {
            let counter = counter.clone();
            Box::pin(async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err(LspError::LaunchFailed("test".into()))
            })
        }
    }

    fn panic_factory(
    ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static {
        move |_server, _root| Box::pin(async move { panic!("initialization panic") })
    }

    async fn await_join<T: Send + 'static>(handle: tokio::task::JoinHandle<T>) -> T {
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("task timed out")
            .expect("task panicked")
    }

    async fn expect_init_cancelled(result: Result<(String, PathBuf), LspError>) {
        match result {
            Err(LspError::InitializationCancelled(_)) => {}
            other => panic!("expected InitializationCancelled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn close_non_open_file_succeeds() {
        let svc = LspService::new(LspConfig::Disabled(false));
        assert!(svc.close_file(rust_file()).await.is_ok());
    }

    #[tokio::test]
    async fn save_non_open_file_succeeds() {
        let svc = LspService::new(LspConfig::Disabled(false));
        assert!(svc.save_file(rust_file(), Some("text")).await.is_ok());
    }

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
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
        svc.shutdown_all().await;
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    #[tokio::test]
    async fn get_or_create_client_rejects_after_shutdown() {
        let svc = LspService::new(LspConfig::Disabled(false));
        svc.shutdown_all().await;
        let result = svc.get_or_create_client(rust_file()).await;
        assert!(matches!(result, Err(LspError::InitializationCancelled(_))));
    }

    #[tokio::test]
    async fn shutdown_increments_generation() {
        let svc = LspService::new(LspConfig::Disabled(false));
        assert_eq!(svc.lifecycle.read().await.generation, 0);
        svc.shutdown_all().await;
        assert_eq!(svc.lifecycle.read().await.generation, 1);
    }

    #[tokio::test]
    async fn same_key_concurrent_cold_start_invokes_factory_once() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState {
            invocations: AtomicUsize::new(0),
            entered: entered_tx,
            release: Notify::new(),
            outcome: Mutex::new(FactoryOutcome::LaunchFailed("test".into())),
            shutdown_count: std::sync::Arc::new(AtomicUsize::new(0)),
        });
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        let barrier = std::sync::Arc::new(Barrier::new(21));
        let mut handles = Vec::new();
        for _ in 0..20 {
            let svc = svc.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                svc.get_or_create_client(rust_file()).await
            }));
        }

        barrier.wait().await;
        entered_rx.changed().await.expect("factory should enter");
        assert!(*entered_rx.borrow());
        assert_eq!(state.invocations.load(Ordering::SeqCst), 1);
        state.release.notify_waiters();

        for handle in handles {
            let result = await_join(handle).await;
            match result {
                Err(LspError::LaunchFailed(msg)) => assert_eq!(msg, "test"),
                other => panic!("expected LaunchFailed, got {other:?}"),
            }
        }

        assert!(svc.initializing.lock().await.is_empty());
    }

    #[tokio::test]
    async fn second_caller_becomes_waiter_before_leader_spawn() {
        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let (leader_gate, mut leader_rx) = pause_gate();
        let hooks = std::sync::Arc::new(TestHooks {
            leader_spawn_gate: Some(leader_gate.clone()),
            shutdown_gate: None,
        });
        let svc = std::sync::Arc::new(LspService::test_new_with_hooks(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
            hooks,
        ));

        let first = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        leader_rx.changed().await.expect("leader gate should trip");
        assert!(*leader_rx.borrow());

        let second = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        assert_eq!(counter.load(Ordering::SeqCst), 0);
        leader_gate.release.notify_waiters();

        let first_result = await_join(first).await;
        let second_result = await_join(second).await;
        assert!(matches!(first_result, Err(LspError::LaunchFailed(_))));
        assert!(matches!(second_result, Err(LspError::LaunchFailed(_))));
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shared_failure_is_identical_for_all_callers() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState {
            invocations: AtomicUsize::new(0),
            entered: entered_tx,
            release: Notify::new(),
            outcome: Mutex::new(FactoryOutcome::LaunchFailed("shared".into())),
            shutdown_count: std::sync::Arc::new(AtomicUsize::new(0)),
        });
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        let barrier = std::sync::Arc::new(Barrier::new(21));
        let mut handles = Vec::new();
        for _ in 0..20 {
            let svc = svc.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                svc.get_or_create_client(rust_file()).await
            }));
        }

        barrier.wait().await;
        entered_rx.changed().await.expect("factory should enter");
        state.release.notify_waiters();

        let mut first_err: Option<(String, String)> = None;
        for handle in handles {
            let result = await_join(handle).await;
            let err = match result {
                Err(err) => err,
                Ok(_) => panic!("expected error"),
            };
            let shared = match err {
                LspError::LaunchFailed(msg) => ("LaunchFailed".to_string(), msg),
                other => panic!("expected LaunchFailed, got {other:?}"),
            };
            if let Some((kind, msg)) = &first_err {
                assert_eq!(kind, &shared.0);
                assert_eq!(msg, &shared.1);
            } else {
                first_err = Some(shared);
            }
        }

        assert_eq!(state.invocations.load(Ordering::SeqCst), 1);
        assert!(svc.initializing.lock().await.is_empty());
    }

    #[tokio::test]
    async fn retry_after_failure_invokes_factory_again() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState {
            invocations: AtomicUsize::new(0),
            entered: entered_tx,
            release: Notify::new(),
            outcome: Mutex::new(FactoryOutcome::LaunchFailed("first".into())),
            shutdown_count: std::sync::Arc::new(AtomicUsize::new(0)),
        });
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        let first = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });
        entered_rx
            .changed()
            .await
            .expect("first attempt should enter");
        state.release.notify_waiters();
        let first_result = await_join(first).await;
        assert!(matches!(first_result, Err(LspError::LaunchFailed(msg)) if msg == "first"));
        assert_eq!(state.invocations.load(Ordering::SeqCst), 1);

        *state.outcome.lock().await = FactoryOutcome::LaunchFailed("second".into());
        let second = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });
        entered_rx
            .changed()
            .await
            .expect("second attempt should enter");
        state.release.notify_waiters();
        let second_result = await_join(second).await;
        assert!(matches!(second_result, Err(LspError::LaunchFailed(msg)) if msg == "second"));
        assert_eq!(state.invocations.load(Ordering::SeqCst), 2);
        assert!(svc.initializing.lock().await.is_empty());
    }

    #[tokio::test]
    async fn shutdown_during_init_cancels_waiters_and_disposes_client() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let shutdown_count = std::sync::Arc::new(AtomicUsize::new(0));
        let state = std::sync::Arc::new(BlockingFactoryState {
            invocations: AtomicUsize::new(0),
            entered: entered_tx,
            release: Notify::new(),
            outcome: Mutex::new(FactoryOutcome::Success),
            shutdown_count: shutdown_count.clone(),
        });
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        let barrier = std::sync::Arc::new(Barrier::new(4));
        let mut handles = Vec::new();
        for _ in 0..3 {
            let svc = svc.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                svc.get_or_create_client(rust_file()).await
            }));
        }

        barrier.wait().await;
        entered_rx.changed().await.expect("factory should enter");
        assert_eq!(state.invocations.load(Ordering::SeqCst), 1);

        svc.shutdown_all().await;
        state.release.notify_waiters();

        for handle in handles {
            let result = await_join(handle).await;
            expect_init_cancelled(result).await;
        }

        assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
        assert!(svc.clients.write().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        assert!(svc.document_owners.write().await.is_empty());
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    #[tokio::test]
    async fn publish_before_shutdown_drains_published_client() {
        let (shutdown_gate, mut shutdown_rx) = pause_gate();
        let hooks = std::sync::Arc::new(TestHooks {
            leader_spawn_gate: None,
            shutdown_gate: Some(shutdown_gate.clone()),
        });
        let shutdown_count = std::sync::Arc::new(AtomicUsize::new(0));
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState {
            invocations: AtomicUsize::new(0),
            entered: entered_tx,
            release: Notify::new(),
            outcome: Mutex::new(FactoryOutcome::Success),
            shutdown_count: shutdown_count.clone(),
        });
        let svc = std::sync::Arc::new(LspService::test_new_with_hooks(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
            hooks,
        ));

        let shutdown_handle = {
            let svc = svc.clone();
            tokio::spawn(async move {
                svc.shutdown_all().await;
            })
        };

        shutdown_rx
            .changed()
            .await
            .expect("shutdown gate should trip");
        assert!(*shutdown_rx.borrow());

        let init_handle = {
            let svc = svc.clone();
            tokio::spawn(async move { svc.get_or_create_client(rust_file()).await })
        };

        entered_rx.changed().await.expect("factory should enter");
        state.release.notify_waiters();

        let init_result = await_join(init_handle).await;
        let (key, _root) = match init_result {
            Ok(pair) => pair,
            Err(err) => panic!("expected published client before shutdown, got {err:?}"),
        };

        assert!(svc.clients.write().await.contains_key(&key));

        shutdown_gate.release.notify_waiters();
        await_join(shutdown_handle).await;

        assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
        assert!(svc.clients.write().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    #[tokio::test]
    async fn factory_panic_resolves_all_callers() {
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            panic_factory(),
        ));

        let barrier = std::sync::Arc::new(Barrier::new(6));
        let mut handles = Vec::new();
        for _ in 0..5 {
            let svc = svc.clone();
            let barrier = barrier.clone();
            handles.push(tokio::spawn(async move {
                barrier.wait().await;
                svc.get_or_create_client(rust_file()).await
            }));
        }

        barrier.wait().await;

        for handle in handles {
            let result = await_join(handle).await;
            match result {
                Err(LspError::InitializationCancelled(msg)) => {
                    assert!(msg.contains("panicked") || msg.contains("cancelled"));
                }
                other => panic!("expected InitializationCancelled, got {other:?}"),
            }
        }

        assert!(svc.clients.write().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
    }
}
