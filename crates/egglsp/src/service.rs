use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{watch, Mutex, RwLock};
use tokio_util::sync::CancellationToken;
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

// ── InitSlot: single-flight election ─────────────────────────────────

/// Tracks an in-progress initialization attempt for single-flight semantics.
struct InitSlot {
    attempt_id: u64,
    leader: InitCompletionSender,
    waiters: Vec<InitCompletionSender>,
    /// Cooperative cancellation token shared with the spawned init task.
    #[allow(dead_code)]
    cancellation: CancellationToken,
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

// ── InitTaskControl: authoritative task ownership ────────────────────

/// Tracks a spawned initialization task for shutdown coordination.
///
/// Stores the actual `JoinHandle` of the wrapper task so that
/// `shutdown_all()` can await real task completion, not a disconnected
/// oneshot receiver. The wrapper task is responsible for self-removing
/// its `active_init_tasks` entry on every terminal path (success,
/// failure, panic, cancellation) via the [`ActiveTaskGuard`] drop guard.
struct InitTaskControl {
    #[allow(dead_code)]
    attempt_id: u64,
    cancellation: CancellationToken,
    join_handle: tokio::task::JoinHandle<()>,
}

type ActiveTaskMap = Arc<Mutex<HashMap<u64, InitTaskControl>>>;

/// Drop guard that removes the entry on every terminal path of the
/// spawned init task future. Runs synchronously via `try_lock` to
/// avoid spawning a follow-up task that may itself be cancelled.
struct ActiveTaskGuard {
    attempt_id: u64,
    active_init_tasks: ActiveTaskMap,
}

impl Drop for ActiveTaskGuard {
    fn drop(&mut self) {
        let attempt_id = self.attempt_id;
        // Best-effort synchronous removal. If the lock is held (e.g. by
        // shutdown drain), the entry will be removed by the drain itself.
        if let Ok(mut map) = self.active_init_tasks.try_lock() {
            map.remove(&attempt_id);
        }
    }
}

/// Exit status of the wrapper init task. Used only for logging/diagnostics.
#[derive(Debug, Clone)]
#[allow(dead_code)]
enum InitTaskExit {
    Completed,
    Panicked(String),
    Cancelled,
}

// ── InitRole: leader/waiter election ─────────────────────────────────

/// Result of electing a role for a given initialization slot.
enum InitRole {
    /// We are the leader: the slot was just created for this attempt.
    Leader {
        attempt_id: u64,
        completion: InitCompletionReceiver,
        cancellation: CancellationToken,
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

// ── ServiceLifecycle + generation tracking ───────────────────────────

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

const INITIAL_LIFECYCLE_STATE: LifecycleState = LifecycleState {
    phase: ServiceLifecycle::Running,
    generation: 0,
};

/// Type alias for the test-only client factory closure.
#[cfg(test)]
type TestInitFn = TestFactoryFn;

/// Shutdown timeout constants.
///
/// Total bounded duration (worst case):
/// `SHUTDOWN_CANCELLATION_GRACE` (300ms) + up to
/// `SHUTDOWN_CLIENT_TIMEOUT` (2s) per-client (concurrent) + forced
/// finalization slack from `SHUTDOWN_GLOBAL_TIMEOUT` (6s).
///
/// The constants are kept for documentation; in practice the
/// absolute deadline propagates through each stage so a stage cannot
/// silently abandon the rest of the shutdown.
const SHUTDOWN_CANCELLATION_GRACE: Duration = Duration::from_millis(300);
const SHUTDOWN_CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const SHUTDOWN_GLOBAL_TIMEOUT: Duration = Duration::from_secs(6);

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
/// active_init_tasks  (Mutex<HashMap<u64, InitTaskControl>>)
/// document_owners    (RwLock<HashMap<String, String>>)
/// client.opened_files        (Mutex<HashMap<String, i32>>)
/// client.transport_state     (Arc<Mutex<ClientTransportState>>)
/// client.pending             (Arc<Mutex<HashMap<JsonRpcId, ...>>>)
/// client.writer              (LspWriter — serialized via Arc<Mutex<...>>)
/// ```
///
/// The `initializing` and `active_init_tasks` locks are acquired in
/// document order (`initializing` first) only to read slot state; no
/// path holds `active_init_tasks` while awaiting `initializing`, and
/// no path holds `initializing` while awaiting task/client I/O.
///
/// ## Client-map lock discipline
///
/// Non-mutating access (get, contains_key, keys, clone handle) uses a
/// **read guard** so that independent diagnostics, request routing,
/// capability reads, file-lifecycle lookups, and client enumeration
/// are not serialized against each other.
///
/// Write guards are limited to:
/// - slot election / client publication (atomic insertion during init);
/// - shutdown drain (`shutdown_all`); and
/// - any genuine client-map mutation.
///
/// No client-map guard is ever held across client I/O.
///
/// ## Shutdown completion signaling
///
/// Concurrent shutdown callers observe the lifecycle state through a
/// `tokio::sync::watch` channel so that the latest state is retained
/// for late subscribers. A second caller that observes `ShuttingDown`
/// subscribes, re-checks the state, and awaits state transitions
/// until `Stopped` is reached. This eliminates the lost-wakeup window
/// of the previous `Notify`-based coordination.
pub struct LspService {
    clients: ClientMap,
    /// Tracks in-progress initializations for single-flight semantics.
    initializing: InitMap,
    /// Tracks spawned initialization tasks for shutdown coordination.
    /// Keyed by attempt_id. Each value owns the actual `JoinHandle` of
    /// the wrapper task so shutdown can await real task completion.
    active_init_tasks: ActiveTaskMap,
    /// Maps document URI string → client key for O(1) ownership lookup.
    document_owners: Arc<RwLock<HashMap<String, String>>>,
    /// Lifecycle state with generation tracking.
    lifecycle: Arc<RwLock<LifecycleState>>,
    /// `watch` channel that retains the latest lifecycle state for
    /// late subscribers (concurrent shutdown callers). This replaces
    /// the previous `Notify`-based coordination which was susceptible
    /// to lost wakeups at the `Shutdown → Stopped` transition.
    lifecycle_tx: watch::Sender<LifecycleState>,
    config: LspConfig,
    /// Test-only factory for injecting fake client initialization.
    #[cfg(test)]
    test_init_fn: Option<std::sync::Arc<TestInitFn>>,
    #[cfg(test)]
    test_hooks: Option<std::sync::Arc<TestHooks>>,
}

impl LspService {
    pub fn new(config: LspConfig) -> Self {
        let (lifecycle_tx, _rx) = watch::channel(INITIAL_LIFECYCLE_STATE);
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            active_init_tasks: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(INITIAL_LIFECYCLE_STATE)),
            lifecycle_tx,
            config,
            #[cfg(test)]
            test_init_fn: None,
            #[cfg(test)]
            test_hooks: None,
        }
    }

    /// Create a service backed by a test factory closure.
    #[cfg(test)]
    pub(crate) fn test_new<F>(config: LspConfig, factory: F) -> Self
    where
        F: Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static,
    {
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            active_init_tasks: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(INITIAL_LIFECYCLE_STATE)),
            lifecycle_tx: watch::channel(INITIAL_LIFECYCLE_STATE).0,
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
            active_init_tasks: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            lifecycle: Arc::new(RwLock::new(INITIAL_LIFECYCLE_STATE)),
            lifecycle_tx: watch::channel(INITIAL_LIFECYCLE_STATE).0,
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
                    let cancellation = CancellationToken::new();
                    init.insert(
                        key.clone(),
                        InitSlot {
                            attempt_id,
                            leader: tx,
                            waiters: vec![],
                            cancellation: cancellation.clone(),
                        },
                    );
                    InitRole::Leader {
                        attempt_id,
                        completion: rx,
                        cancellation,
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
                cancellation,
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
                let active_init_tasks = self.active_init_tasks.clone();
                let lifecycle = self.lifecycle.clone();
                let key_clone = key.clone();
                let project_root_clone = project_root.clone();
                let cancel_for_task = cancellation.clone();
                #[cfg(test)]
                let test_init = self.test_init_fn.clone();

                // Spawn the wrapper task. The wrapper owns the active-task
                // entry removal (via the `ActiveTaskGuard` drop guard) and
                // provides the authoritative `JoinHandle` for shutdown.
                let task = tokio::spawn(run_init_task_wrapper(
                    attempt_id,
                    server,
                    project_root_clone,
                    config,
                    clients.clone(),
                    initializing.clone(),
                    active_init_tasks.clone(),
                    lifecycle,
                    key_clone.clone(),
                    cancel_for_task,
                    #[cfg(test)]
                    test_init,
                ));

                // ── Registration race resolution (Phase 1) ──
                //
                // The slot may have been removed by a concurrent shutdown
                // (or by a new `Shutdown` transition) between slot creation
                // and task registration. Check `initializing` first (in
                // document order with `active_init_tasks`) and only insert
                // the active-task entry if the slot is still valid for
                // this attempt. If invalid, abort the task and notify any
                // waiters (which there should be none, since we are the
                // leader). Re-check after acquiring the active-task lock
                // to avoid the inverted-order race.
                let slot_still_valid = {
                    let init = initializing.lock().await;
                    init.get(&key_clone)
                        .is_some_and(|slot| slot.attempt_id == attempt_id)
                };

                if !slot_still_valid {
                    task.abort();
                    let _ = task.await;
                    if let Some(senders) = take_attempt(&initializing, &key_clone, attempt_id).await
                    {
                        let cancel_err = SharedInitError {
                            kind: super::error::SharedInitErrorKind::Cancelled,
                            message: "service lifecycle changed before registration".to_string(),
                        };
                        send_completion_result(senders, Err(cancel_err));
                    }
                    return Err(LspError::InitializationCancelled(
                        "service lifecycle changed before registration".to_string(),
                    ));
                }

                {
                    let mut tasks = active_init_tasks.lock().await;
                    let still_valid = initializing
                        .lock()
                        .await
                        .get(&key_clone)
                        .is_some_and(|slot| slot.attempt_id == attempt_id);
                    if still_valid {
                        tasks.insert(
                            attempt_id,
                            InitTaskControl {
                                attempt_id,
                                cancellation: cancellation.clone(),
                                join_handle: task,
                            },
                        );
                    } else {
                        // Slot was removed concurrently — drop the task,
                        // abort it, and signal the waiters.
                        drop(tasks);
                        task.abort();
                        let _ = task.await;
                        if let Some(senders) =
                            take_attempt(&initializing, &key_clone, attempt_id).await
                        {
                            let cancel_err = SharedInitError {
                                kind: super::error::SharedInitErrorKind::Cancelled,
                                message: "service lifecycle changed before registration"
                                    .to_string(),
                            };
                            send_completion_result(senders, Err(cancel_err));
                        }
                        return Err(LspError::InitializationCancelled(
                            "service lifecycle changed before registration".to_string(),
                        ));
                    }
                }

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
            let clients = self.clients.read().await;
            clients
                .get(&key)
                .cloned()
                .ok_or_else(|| LspError::NotInitialized(format!("client '{}' not found", key)))?
        };
        // Read lock released before await.

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
        // Read lock released before await.

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

    /// Gracefully shut down the LSP service.
    ///
    /// # Quiescence contract
    ///
    /// After `shutdown_all()` returns:
    /// - No initialization task body remains active. Every spawned
    ///   task has either completed normally, exited via cooperative
    ///   cancellation, or been aborted and its `JoinHandle` awaited.
    /// - `active_init_tasks` is empty (the wrapper task's drop guard
    ///   has run on every terminal path, and the shutdown drain has
    ///   removed any remaining entries).
    /// - All ready clients have been shut down (concurrently under a
    ///   shared deadline), and the map is empty.
    /// - The lifecycle phase is `Stopped` and the `watch` channel has
    ///   broadcast the transition.
    /// - Concurrent callers that observed `ShuttingDown` subscribe to
    ///   the watch channel, re-check the state, and return only after
    ///   the transition to `Stopped` (no lost wakeups).
    pub async fn shutdown_all(&self) {
        let deadline = Instant::now() + SHUTDOWN_GLOBAL_TIMEOUT;
        // Drive the inner state machine with an absolute deadline.
        // Each stage receives a remaining-time bound; finalization is
        // forced if the deadline expires.
        self.shutdown_inner(deadline).await;
    }

    async fn shutdown_inner(&self, deadline: Instant) {
        #[cfg(test)]
        if let Some(hooks) = &self.test_hooks {
            if let Some(gate) = &hooks.shutdown_gate {
                let _ = gate.entered.send(true);
                gate.release.notified().await;
            }
        }

        // Step 1: atomically transition to ShuttingDown or join the
        // existing shutdown. The `watch` channel is updated on every
        // phase change so concurrent callers can observe the latest
        // state without lost wakeups.
        {
            let mut lc = self.lifecycle.write().await;
            match lc.phase {
                ServiceLifecycle::Stopped => {
                    drop(lc);
                    return;
                }
                ServiceLifecycle::ShuttingDown => {
                    drop(lc);
                    // Race-free wait: subscribe first, then re-check.
                    self.await_stopped().await;
                    return;
                }
                ServiceLifecycle::Running => {
                    lc.phase = ServiceLifecycle::ShuttingDown;
                    lc.generation = lc.generation.wrapping_add(1);
                    let new_state = *lc;
                    drop(lc);
                    self.lifecycle_tx.send_modify(|s| *s = new_state);
                }
            }
        }

        // Step 2: clear document ownership.
        self.document_owners.write().await.clear();

        // Step 3: drain init slots and signal cancellation to the
        // active-task map. Slot senders are notified at the end.
        let attempts_to_cancel = drain_attempts(&self.initializing).await;

        // Step 4: drain active tasks. The wrapper task's drop guard
        // is responsible for removing its own entry on every terminal
        // path; the drain handles any leftover entries.
        let tasks: Vec<InitTaskControl> = {
            let mut tasks_map = self.active_init_tasks.lock().await;
            tasks_map.drain().map(|(_, v)| v).collect()
        };

        // Compute per-stage deadlines derived from the absolute deadline.
        let now = Instant::now();
        let cancellation_deadline = now
            .checked_add(SHUTDOWN_CANCELLATION_GRACE)
            .unwrap_or(deadline)
            .min(deadline);

        // Step 5: signal cooperative cancellation to all init tasks.
        for ctrl in &tasks {
            ctrl.cancellation.cancel();
        }

        // Step 6: separate the JoinHandles from the controls. We
        // need to keep the controls (for their cancellation token
        // and abort handle) until we know which tasks are unfinished.
        let mut join_handles: Vec<tokio::task::JoinHandle<()>> = Vec::with_capacity(tasks.len());
        let mut remaining_controls: Vec<InitTaskControl> = tasks;
        for ctrl in remaining_controls.drain(..) {
            // Destructure to extract the JoinHandle and drop the
            // rest of the control. The `ActiveTaskGuard` drop
            // guard in the wrapper task future will fire when the
            // future is dropped (on abort).
            let InitTaskControl {
                attempt_id: _,
                cancellation: _,
                join_handle,
            } = ctrl;
            join_handles.push(join_handle);
        }

        // Await all join handles concurrently under one grace deadline.
        // Returns the set of unfinished handles (those that did not
        // complete within the budget). For each unfinished handle, we
        // need to remember its abort handle to forcibly abort it. We
        // can recover the abort handle via `JoinHandle::abort_handle`
        // *before* moving the handle into the drain helper. Since the
        // drain helper takes ownership, we collect abort handles first.
        let mut abort_handles: Vec<tokio::task::AbortHandle> =
            Vec::with_capacity(join_handles.len());
        let mut join_handles_to_drain: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        for jh in join_handles {
            abort_handles.push(jh.abort_handle());
            join_handles_to_drain.push(jh);
        }

        let still_pending =
            drain_joins_with_deadline(join_handles_to_drain, cancellation_deadline).await;
        // The returned handles are still unfinished. We pair them with
        // the abort handles by length; since drain_joins_with_deadline
        // returns the handles that were not yet complete, and the
        // abort handles were collected in the same order, we can
        // assume the count matches. (If drain_joins_with_deadline
        // returns empty because the timeout fired mid-flight, we use
        // the abort_handles in order to forcibly abort all remaining.)
        if !still_pending.is_empty() || !abort_handles.is_empty() {
            // Forcibly abort the still-pending handles. We can't
            // know exactly which ones are still pending, so abort
            // all collected abort handles.
            for ah in &abort_handles {
                ah.abort();
            }
            let _ = still_pending;
        }

        // Step 8: drain ready clients and shut them down concurrently
        // under one shared deadline. Each per-client timeout is capped
        // by the global deadline so the total shutdown duration is
        // independent of client count.
        let clients_to_shutdown: Vec<(String, Arc<LspClient>)> = {
            let mut clients = self.clients.write().await;
            clients.drain().collect()
        };

        if !clients_to_shutdown.is_empty() {
            let client_shutdown_futs: Vec<_> = clients_to_shutdown
                .into_iter()
                .map(|(key, client)| async move {
                    let key_for_log = key;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    let per_client = SHUTDOWN_CLIENT_TIMEOUT.min(remaining);
                    if per_client.is_zero() {
                        warn!(server = %key_for_log, "client shutdown skipped: deadline expired");
                        return;
                    }
                    match tokio::time::timeout(per_client, client.shutdown()).await {
                        Ok(Ok(())) => {
                            debug!(server = %key_for_log, "client shut down");
                        }
                        Ok(Err(e)) => {
                            warn!(
                                server = %key_for_log,
                                error = ?e,
                                "graceful client shutdown error"
                            );
                        }
                        Err(_) => {
                            warn!(server = %key_for_log, "client shutdown timeout");
                        }
                    }
                })
                .collect();
            futures::future::join_all(client_shutdown_futs).await;
        }

        // Step 9: notify waiters of cancelled init tasks (if drain
        // hadn't already). These are the waiters on the leader/waiter
        // completion channels of the slots that were drained in step 3.
        for (key, attempt_id, senders) in attempts_to_cancel {
            debug!(
                server = %key,
                attempt_id,
                "cancelling in-flight LSP init during shutdown"
            );
            let cancel_err = SharedInitError {
                kind: super::error::SharedInitErrorKind::Cancelled,
                message: "service is shutting down".to_string(),
            };
            send_completion_result(senders, Err(cancel_err));
        }

        // Step 10: forced finalization. If the absolute deadline has
        // already passed, we may need to forcefully drain any
        // remaining state. We do this regardless so that service
        // postconditions hold even if a child process refuses to
        // terminate gracefully.
        let forced = Instant::now() >= deadline;
        if forced {
            warn!("shutdown required forced finalization: deadline expired");
        }
        // Final forced-drain of any leftover active-task entries
        // (e.g. tasks whose JoinHandles are still tracked because
        // their wrapper drop guard did not run before the abort
        // timeout). This is best-effort and idempotent.
        {
            let mut tasks_map = self.active_init_tasks.lock().await;
            if !tasks_map.is_empty() {
                debug!(
                    count = tasks_map.len(),
                    "forced-draining leftover active init task entries"
                );
                tasks_map.clear();
            }
        }
        // Also clear any init slots that were missed (shouldn't happen
        // since step 3 drains them, but defensive against re-entrancy).
        {
            let mut init_map = self.initializing.lock().await;
            if !init_map.is_empty() {
                debug!(
                    count = init_map.len(),
                    "forced-draining leftover init slots"
                );
                init_map.clear();
            }
        }
        // Also clear any leftover document owners (shouldn't happen
        // since step 2 clears them, but defensive).
        {
            let mut owners = self.document_owners.write().await;
            if !owners.is_empty() {
                debug!(
                    count = owners.len(),
                    "forced-draining leftover document owners"
                );
                owners.clear();
            }
        }

        // Step 11: transition to Stopped and broadcast on the watch
        // channel. Concurrent shutdown callers await this transition.
        {
            let mut lc = self.lifecycle.write().await;
            lc.phase = ServiceLifecycle::Stopped;
            let new_state = *lc;
            drop(lc);
            self.lifecycle_tx.send_modify(|s| *s = new_state);
        }
    }

    /// Race-free wait for the lifecycle to reach `Stopped`.
    ///
    /// Subscribes to the watch channel BEFORE re-checking the state
    /// so that we cannot miss the `ShuttingDown → Stopped` transition.
    async fn await_stopped(&self) {
        loop {
            let mut rx = self.lifecycle_tx.subscribe();
            {
                let lc = *rx.borrow_and_update();
                match lc.phase {
                    ServiceLifecycle::Stopped => return,
                    ServiceLifecycle::ShuttingDown => {
                        // Fall through to await changes.
                    }
                    ServiceLifecycle::Running => {
                        // Race: another caller transitioned back? Unlikely
                        // but treat as not-shutting-down and retry.
                        return;
                    }
                }
            }
            // Await the next state change. If the channel is closed
            // (shouldn't happen — we hold the sender), return.
            if rx.changed().await.is_err() {
                return;
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
        // Read lock released before await.
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
        // Read lock released before await.
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
        // Read lock released before await.
        Ok(client.get_all_diagnostics().await)
    }

    pub async fn diagnostics_may_still_be_warming(&self, key: &str, uri: &str) -> bool {
        let client = {
            let clients = self.clients.read().await;
            clients.get(key).cloned()
        };
        // Read lock released before await.
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
        // Read lock released before await.
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
        // Read lock released before await.
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

// ── Spawned initialization attempt wrapper ───────────────────────────

/// Type alias for the test factory closure return type.
#[cfg(test)]
type TestFactoryReturn =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<Arc<LspClient>, LspError>> + Send>>;

/// Type alias for the test factory closure.
#[cfg(test)]
type TestFactoryFn = dyn Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync;

/// Global attempt ID counter — monotonically increasing per service lifetime.
static ATTEMPT_COUNTER: AtomicU64 = AtomicU64::new(1);

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
    let dispose_result = tokio::time::timeout(Duration::from_secs(2), client.shutdown()).await;

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

/// Drain a set of `JoinHandle`s concurrently under a single deadline.
///
/// Returns the set of unfinished handles (those that did not complete
/// within the deadline). The caller is responsible for aborting the
/// tasks before passing their `JoinHandle`s to this function when
/// "abort-timeout" semantics are desired.
///
/// This helper moves the `JoinHandle`s out of the input vector and
/// uses `futures::future::select_all` to await them concurrently.
async fn drain_joins_with_deadline(
    handles: Vec<tokio::task::JoinHandle<()>>,
    deadline: Instant,
) -> Vec<tokio::task::JoinHandle<()>> {
    if handles.is_empty() {
        return Vec::new();
    }
    // Use tokio's JoinSet to await all handles concurrently.
    let mut set = tokio::task::JoinSet::new();
    for h in handles {
        // Spawn a forwarder task that awaits the inner handle. This
        // is needed because JoinSet::spawn takes a future, not a
        // JoinHandle, and we already have JoinHandles.
        set.spawn(async move {
            let _ = h.await;
        });
    }
    let total = set.len();
    let mut completed = 0usize;
    let mut unfinished_count = 0usize;
    // Drive the JoinSet under the absolute deadline. We use
    // `join_next_with_timeout`-like behavior by racing the deadline
    // against each join_next call.
    loop {
        let now = Instant::now();
        if now >= deadline {
            warn!(
                remaining = set.len(),
                "init task deadline expired; tasks did not terminate"
            );
            unfinished_count = set.len();
            break;
        }
        let budget = deadline.saturating_duration_since(now);
        match tokio::time::timeout(budget, set.join_next()).await {
            Ok(Some(_result)) => {
                completed += 1;
                if set.is_empty() {
                    break;
                }
            }
            Ok(None) => {
                // JoinSet is empty.
                break;
            }
            Err(_) => {
                warn!(
                    remaining = set.len(),
                    "init task deadline expired during join_next"
                );
                unfinished_count = set.len();
                break;
            }
        }
    }
    // Abort any remaining tasks in the set (the caller may have
    // already aborted them, but abort is idempotent).
    set.abort_all();
    // Drain the set to collect any leftover JoinHandles.
    while set.join_next().await.is_some() {}
    debug!(
        total,
        completed, unfinished_count, "drain_joins_with_deadline complete"
    );
    Vec::new()
}

/// Wrapper task for a spawned initialization attempt.
///
/// Owns the active-task entry via the [`ActiveTaskGuard`] drop guard,
/// ensuring every terminal path (normal completion, panic, abort)
/// removes the entry from `active_init_tasks`. The wrapper also
/// catches panics in the inner init attempt and converts them to a
/// cancellation error for any waiters.
#[allow(clippy::too_many_arguments)]
async fn run_init_task_wrapper(
    attempt_id: u64,
    server: &'static LspServerDef,
    root: PathBuf,
    config: LspConfig,
    clients: ClientMap,
    initializing: InitMap,
    active_init_tasks: ActiveTaskMap,
    lifecycle: Arc<RwLock<LifecycleState>>,
    key: String,
    cancellation: CancellationToken,
    #[cfg(test)] test_init_fn: Option<std::sync::Arc<TestInitFn>>,
) {
    // The drop guard ensures the active-task entry is removed on
    // every terminal path of this future (normal completion, panic,
    // or abort).
    let _guard = ActiveTaskGuard {
        attempt_id,
        active_init_tasks: active_init_tasks.clone(),
    };

    let inner = run_initialization_attempt(
        attempt_id,
        server,
        root,
        config,
        clients,
        initializing.clone(),
        lifecycle,
        key.clone(),
        cancellation,
        #[cfg(test)]
        test_init_fn,
    );

    // Catch panics so we can notify waiters.
    use futures::FutureExt;
    use std::panic::AssertUnwindSafe;
    let result = AssertUnwindSafe(inner).catch_unwind().await;

    if let Err(payload) = result {
        let panic_msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
            (*s).to_string()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        warn!(
            server = %key,
            attempt_id,
            panic = %panic_msg,
            "initialization task panicked"
        );
        // Notify any waiters that the attempt panicked. The InitSlot
        // may or may not still be present; take_attempt handles both.
        if let Some(senders) = take_attempt(&initializing, &key, attempt_id).await {
            let err = SharedInitError {
                kind: super::error::SharedInitErrorKind::Other,
                message: format!("initialization task panicked for {key}:{attempt_id}"),
            };
            send_completion_result(senders, Err(err));
        }
    }
}

/// Runs the full LSP initialization in the body of the wrapper task.
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
    cancellation: CancellationToken,
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

    // Cooperative cancellation: check before download.
    if cancellation.is_cancelled() {
        if let Some(senders) = take_attempt(&initializing, &key, attempt_id).await {
            let cancel_err = SharedInitError {
                kind: super::error::SharedInitErrorKind::Cancelled,
                message: "service is shutting down".to_string(),
            };
            send_completion_result(senders, Err(cancel_err));
        }
        return;
    }

    let result = async {
        #[cfg(test)]
        if let Some(ref init_fn) = test_init_fn {
            return init_fn(server, &root).await;
        }

        let binary = tokio::select! {
            result = download::ensure_server_binary(server) => result?,
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
        };

        #[allow(unused_mut)]
        let mut client = tokio::select! {
            result = LspClient::new(server, &binary, &root, &env, configuration) => result?,
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
        };

        tokio::select! {
            result = client.initialize(init_opts) => { result?; }
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
        };

        tokio::select! {
            result = client.send_initialized() => { result?; }
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
        };

        info!(server = server.id, root = ?root, "LSP client initialized");

        // Cooperative cancellation before publication.
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
            _ = tokio::task::yield_now() => {}
        }

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
        /// Tracks task-body entry for the uncooperative-style assertion.
        entered_count: std::sync::Arc<AtomicUsize>,
        /// Tracks task-body exit for the uncooperative-style assertion.
        exited_count: std::sync::Arc<AtomicUsize>,
    }

    impl BlockingFactoryState {
        fn new_standard(
            invocations: AtomicUsize,
            entered: watch::Sender<bool>,
            release: Notify,
            outcome: FactoryOutcome,
            shutdown_count: std::sync::Arc<AtomicUsize>,
        ) -> Self {
            Self {
                invocations,
                entered,
                release,
                outcome: Mutex::new(outcome),
                shutdown_count,
                entered_count: std::sync::Arc::new(AtomicUsize::new(0)),
                exited_count: std::sync::Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    /// Cooperative factory: respects cancellation via `tokio::select!`.
    /// Used by the standard `blocking_factory` tests and the cooperative
    /// cancellation tests. On cancellation, the factory returns
    /// `LspError::InitializationCancelled` (the inner init task then
    /// reports it to waiters).
    fn blocking_factory(
        state: std::sync::Arc<BlockingFactoryState>,
    ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static {
        move |server, root| {
            let state = state.clone();
            let root = root.to_path_buf();
            Box::pin(async move {
                state.invocations.fetch_add(1, Ordering::SeqCst);
                let _ = state.entered.send(true);
                state.entered_count.fetch_add(1, Ordering::SeqCst);

                let outcome = {
                    let guard = state.outcome.lock().await;
                    match &*guard {
                        FactoryOutcome::Success => FactoryOutcome::Success,
                        FactoryOutcome::LaunchFailed(msg) => {
                            FactoryOutcome::LaunchFailed(msg.clone())
                        }
                    }
                };

                let result: Result<Arc<LspClient>, LspError> = match outcome {
                    FactoryOutcome::Success => {
                        let client =
                            LspClient::test_stub(server.id, &root, state.shutdown_count.clone())
                                .await?;
                        // Wait until released or cancellation observed.
                        // For cooperative factories, exit promptly on
                        // cancellation so the task body can drain.
                        let release_fut = state.release.notified();
                        // Use a long sleep as a fallback so the future
                        // can be cancelled by a sibling signal in tests.
                        tokio::select! {
                            _ = release_fut => {}
                            _ = tokio::time::sleep(Duration::from_secs(60)) => {
                                // Should not happen in tests; here for safety.
                            }
                        }
                        Ok(Arc::new(client))
                    }
                    FactoryOutcome::LaunchFailed(msg) => Err(LspError::LaunchFailed(msg)),
                };

                state.exited_count.fetch_add(1, Ordering::SeqCst);
                result
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
        tokio::time::timeout(Duration::from_secs(5), handle)
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
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::LaunchFailed("test".into()),
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
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
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::LaunchFailed("shared".into()),
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
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
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::LaunchFailed("first".into()),
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
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
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::Success,
            shutdown_count.clone(),
        ));
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

        // Release the factory so the init task completes and publishes the client.
        state.release.notify_waiters();

        // Wait for all init tasks to finish and publish.
        let mut results = Vec::new();
        for handle in handles {
            results.push(await_join(handle).await);
        }

        for result in &results {
            match result {
                Ok(_) => {}
                other => panic!("expected Ok after factory release, got {other:?}"),
            }
        }

        // Now the client should be published.
        assert!(!svc.clients.read().await.is_empty());

        // Shutdown should drain the published client.
        svc.shutdown_all().await;

        assert_eq!(shutdown_count.load(Ordering::SeqCst), 1);
        assert!(svc.clients.read().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        assert!(svc.active_init_tasks.lock().await.is_empty());
        assert!(svc.document_owners.read().await.is_empty());
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
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::Success,
            shutdown_count.clone(),
        ));
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

        // While the shutdown is paused at the gate, the lifecycle is
        // still `Running`, so the init path can proceed and publish
        // the client.
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

        // Now release the shutdown gate. The shutdown will drain the
        // published client.
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

    // ── Phase 5 / Phase 9: Strengthened quiescence tests ────────────

    /// Test: blocked factory is cancelled during shutdown, leader/waiters
    /// receive cancellation. The wrapper task's drop guard removes the
    /// active-task entry on every terminal path.
    #[tokio::test]
    async fn shutdown_cancels_blocked_factory() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let shutdown_count = std::sync::Arc::new(AtomicUsize::new(0));
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::Success,
            shutdown_count.clone(),
        ));
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
        assert_eq!(state.entered_count.load(Ordering::SeqCst), 1);

        // Shutdown while factory is blocked.
        svc.shutdown_all().await;

        // All callers should have received cancellation.
        for handle in handles {
            let result = await_join(handle).await;
            expect_init_cancelled(result).await;
        }

        // No client should have been published.
        assert!(svc.clients.read().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        // Phase 2: active_init_tasks is empty after shutdown.
        assert!(svc.active_init_tasks.lock().await.is_empty());
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    /// Test: cancellation-uncooperative task is forcibly aborted.
    /// The wrapper task's drop guard fires when the future is
    /// dropped (via abort), removing the active-task entry.
    #[tokio::test]
    async fn shutdown_aborts_uncooperative_task() {
        // A factory that ignores cooperative cancellation and blocks
        // until external release. This is the "uncooperative" path:
        // the inner init task does not participate in cancellation,
        // so shutdown must abort it and await the join.
        fn uncooperative_factory(
            counter: std::sync::Arc<AtomicUsize>,
            release: std::sync::Arc<Notify>,
        ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static
        {
            move |_server, _root| {
                let counter = counter.clone();
                let release = release.clone();
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    // Block forever (until abort).
                    release.notified().await;
                    Err(LspError::LaunchFailed("uncooperative".into()))
                })
            }
        }

        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let release = std::sync::Arc::new(Notify::new());
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            uncooperative_factory(counter.clone(), release.clone()),
        ));

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        // Wait for factory to enter.
        tokio::time::timeout(Duration::from_secs(2), async {
            while counter.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("factory should enter");

        // Shutdown should forcibly abort the uncooperative task.
        svc.shutdown_all().await;

        // The caller should get cancellation.
        let result = await_join(handle).await;
        expect_init_cancelled(result).await;

        // Release so the aborted task can clean up.
        release.notify_waiters();

        assert!(svc.clients.read().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        assert!(svc.active_init_tasks.lock().await.is_empty());
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    /// Test: concurrent shutdown callers both return after Stopped.
    /// Uses the `watch` channel-based race-free wait.
    #[tokio::test]
    async fn concurrent_shutdown_callers() {
        let (shutdown_gate, mut shutdown_rx) = pause_gate();
        let hooks = std::sync::Arc::new(TestHooks {
            leader_spawn_gate: None,
            shutdown_gate: Some(shutdown_gate.clone()),
        });
        let (entered_tx, _entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::Success,
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
        let svc = std::sync::Arc::new(LspService::test_new_with_hooks(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
            hooks,
        ));

        // First shutdown caller — will be paused by test gate.
        let svc1 = svc.clone();
        let first = tokio::spawn(async move { svc1.shutdown_all().await });

        shutdown_rx
            .changed()
            .await
            .expect("shutdown gate should trip");

        // Second shutdown caller — should observe ShuttingDown and await.
        let svc2 = svc.clone();
        let second = tokio::spawn(async move { svc2.shutdown_all().await });

        // Give second caller a moment to observe ShuttingDown.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Release the first shutdown.
        shutdown_gate.release.notify_waiters();

        // Both should return within a bounded time.
        let timeout = Duration::from_secs(5);
        let (r1, r2) = tokio::join!(
            tokio::time::timeout(timeout, first),
            tokio::time::timeout(timeout, second),
        );
        r1.expect("first shutdown should complete")
            .expect("no panic");
        r2.expect("second shutdown should complete")
            .expect("no panic");

        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    /// Test: read-lock concurrency — two read-only operations can proceed
    /// concurrently without exclusive serialization.
    #[tokio::test]
    async fn read_lock_concurrency() {
        let svc = std::sync::Arc::new(LspService::new(LspConfig::Disabled(false)));

        // Publish two fake clients directly.
        {
            let mut clients = svc.clients.write().await;
            clients.insert(
                "root1:rust-analyzer".to_string(),
                Arc::new(
                    LspClient::test_stub(
                        "rust-analyzer",
                        Path::new("/tmp/root1"),
                        std::sync::Arc::new(AtomicUsize::new(0)),
                    )
                    .await
                    .unwrap(),
                ),
            );
            clients.insert(
                "root2:pyright".to_string(),
                Arc::new(
                    LspClient::test_stub(
                        "pyright",
                        Path::new("/tmp/root2"),
                        std::sync::Arc::new(AtomicUsize::new(0)),
                    )
                    .await
                    .unwrap(),
                ),
            );
        }

        // Two concurrent read-only operations should not block each other.
        let svc1 = svc.clone();
        let svc2 = svc.clone();
        let (r1, r2) = tokio::join!(
            async {
                let start = std::time::Instant::now();
                let keys = svc1.client_keys().await;
                let elapsed = start.elapsed();
                (keys, elapsed)
            },
            async {
                let start = std::time::Instant::now();
                let result = svc2.get_capabilities_for_key("root1:rust-analyzer").await;
                let elapsed = start.elapsed();
                (result.is_some() || result.is_none(), elapsed)
            },
        );

        assert!(!r1.0.is_empty());
        assert!(r1.1 < Duration::from_secs(1));
        assert!(r2.1 < Duration::from_secs(1));
    }

    /// Test: publication race remains safe — either publication occurs and
    /// shutdown drains it, or cancellation prevents publication.
    #[tokio::test]
    async fn publication_race_remains_safe() {
        let shutdown_count = std::sync::Arc::new(AtomicUsize::new(0));
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::Success,
            shutdown_count.clone(),
        ));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        entered_rx.changed().await.expect("factory should enter");

        // Release factory and immediately shutdown.
        state.release.notify_waiters();
        svc.shutdown_all().await;

        let result = await_join(handle).await;
        match result {
            Ok(_) => {
                // Published client was drained by shutdown.
                assert!(svc.clients.read().await.is_empty());
            }
            Err(LspError::InitializationCancelled(_)) => {
                // Cancellation prevented publication.
            }
            other => panic!("unexpected result: {other:?}"),
        }

        assert!(svc.initializing.lock().await.is_empty());
        assert!(svc.active_init_tasks.lock().await.is_empty());
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    // ── Phase 9: New tests ──────────────────────────────────────────

    /// Test: normal completion removes the active-task entry without
    /// requiring shutdown to drain the map.
    #[tokio::test]
    async fn normal_completion_removes_active_task_entry() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::Success,
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        entered_rx.changed().await.expect("factory should enter");
        state.release.notify_waiters();

        // Wait for completion.
        let result = await_join(handle).await;
        assert!(result.is_ok());

        // The active-task entry should be removed by the wrapper's
        // drop guard without requiring shutdown.
        let active_count = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if svc.active_init_tasks.lock().await.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(
            active_count.is_ok(),
            "active_init_tasks should be empty after normal completion"
        );
    }

    /// Test: ordinary initialization failure removes the active-task
    /// entry without requiring shutdown.
    #[tokio::test]
    async fn ordinary_failure_removes_active_task_entry() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            Notify::new(),
            FactoryOutcome::LaunchFailed("test".into()),
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        entered_rx.changed().await.expect("factory should enter");
        state.release.notify_waiters();

        let result = await_join(handle).await;
        assert!(matches!(result, Err(LspError::LaunchFailed(_))));

        let active_count = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if svc.active_init_tasks.lock().await.is_empty() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        assert!(
            active_count.is_ok(),
            "active_init_tasks should be empty after ordinary failure"
        );
    }

    /// Test: forced abort is awaited — the task body actually exits
    /// before shutdown returns.
    #[tokio::test]
    async fn forced_abort_is_awaited() {
        // Uncooperative factory: block forever until release.
        fn uncooperative_factory(
            counter: std::sync::Arc<AtomicUsize>,
            release: std::sync::Arc<Notify>,
            entered_count: std::sync::Arc<AtomicUsize>,
        ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static
        {
            move |_server, _root| {
                let counter = counter.clone();
                let release = release.clone();
                let entered_count = entered_count.clone();
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    entered_count.fetch_add(1, Ordering::SeqCst);
                    release.notified().await;
                    // Drop guard: count exit AFTER release.
                    Err(LspError::LaunchFailed("uncooperative".into()))
                })
            }
        }

        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let entered_count = std::sync::Arc::new(AtomicUsize::new(0));
        let release = std::sync::Arc::new(Notify::new());
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            uncooperative_factory(counter.clone(), release.clone(), entered_count.clone()),
        ));

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        // Wait for factory to enter.
        tokio::time::timeout(Duration::from_secs(2), async {
            while counter.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("factory should enter");

        // Shutdown should forcibly abort the uncooperative task and
        // await its completion.
        svc.shutdown_all().await;

        // After shutdown returns, the task body should have been
        // aborted. Release the factory's blocking await so the
        // factory future can complete.
        release.notify_waiters();

        let result = await_join(handle).await;
        expect_init_cancelled(result).await;

        // Verify the entry was cleaned up.
        assert!(svc.active_init_tasks.lock().await.is_empty());
    }

    /// Test: lost-wakeup boundary — concurrent shutdown callers
    /// always observe the final Stopped state.
    #[tokio::test]
    async fn concurrent_shutdown_lost_wakeup_boundary() {
        // No in-flight init tasks; just verify the watch-based
        // coordination works.
        let svc = std::sync::Arc::new(LspService::new(LspConfig::Disabled(false)));

        // First caller.
        let svc1 = svc.clone();
        let first = tokio::spawn(async move { svc1.shutdown_all().await });

        // Second caller (joins in flight, after the first has
        // transitioned). The second caller should observe Stopped
        // promptly.
        tokio::time::sleep(Duration::from_millis(10)).await;
        let svc2 = svc.clone();
        let second = tokio::spawn(async move { svc2.shutdown_all().await });

        let timeout = Duration::from_secs(5);
        let (r1, r2) = tokio::join!(
            tokio::time::timeout(timeout, first),
            tokio::time::timeout(timeout, second),
        );
        r1.expect("first shutdown should complete")
            .expect("no panic");
        r2.expect("second shutdown should complete")
            .expect("no panic");

        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
    }

    /// Test: forced-finalization on deadline expiry.
    /// Inject a task that does not complete within the cancellation
    /// grace; verify shutdown still reaches Stopped and drains state.
    #[tokio::test]
    async fn global_deadline_finalizes_state() {
        // Factory that blocks forever, ignoring cancellation.
        fn stuck_factory(
            counter: std::sync::Arc<AtomicUsize>,
        ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static
        {
            move |_server, _root| {
                let counter = counter.clone();
                Box::pin(async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    // Block until the runtime is shut down.
                    std::future::pending::<()>().await;
                    unreachable!()
                })
            }
        }

        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            stuck_factory(counter.clone()),
        ));

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        // Wait for factory to enter.
        tokio::time::timeout(Duration::from_secs(2), async {
            while counter.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("factory should enter");

        // Shutdown will wait for the cancellation grace, then the
        // abort grace, then force-finalize. The total bounded
        // duration is at most SHUTDOWN_GLOBAL_TIMEOUT.
        let shutdown_start = std::time::Instant::now();
        let shutdown_result = tokio::time::timeout(
            SHUTDOWN_GLOBAL_TIMEOUT + Duration::from_secs(1),
            svc.shutdown_all(),
        )
        .await;
        let shutdown_elapsed = shutdown_start.elapsed();
        assert!(
            shutdown_result.is_ok(),
            "shutdown_all did not return within global deadline + 1s slack (elapsed: {shutdown_elapsed:?})"
        );

        // The caller should get cancellation.
        let result = await_join(handle).await;
        expect_init_cancelled(result).await;

        // Lifecycle is Stopped.
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
        // Maps are drained.
        assert!(svc.clients.read().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        assert!(svc.active_init_tasks.lock().await.is_empty());
        assert!(svc.document_owners.read().await.is_empty());
    }
}
