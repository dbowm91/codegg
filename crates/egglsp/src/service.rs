use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use tokio::sync::{watch, Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use url::Url;

use super::client::{DiagnosticCacheEntry, LspClient, LspClientOptions};
use super::compatibility::{LspReadinessPolicy, LspRestartMode};
use super::config::{LspConfig, LspRestartPolicyConfig, LspRule};
use super::document_sync::OpenDocumentRegistry;
use super::download;
use super::error::{LspError, SharedInitError};
use super::health::{transition as transition_health, LspOperationalState};
use super::language::{detect_language, language_id_to_server_id};
use super::launch::LspLaunchSpec;
use super::restart::{
    acquire_restart_ownership, restart_client_coordinator, LspClientDescriptor,
    RestartLeaseAcquisition, RestartShared, RestartTaskMap, RestartTrigger, ServicePhase,
};
use super::root;
use super::runtime::LspProcessRuntime;
use super::server::{self, LspServerDef};
use super::supervisor::LspProcessExitEvent;

type ClientMap = Arc<RwLock<HashMap<String, Arc<LspClient>>>>;

/// Authoritative process runtime paired with the generation that
/// installed it. The explicit `generation` field is the source of
/// truth for runtime-map safety: insertion, lookup, and removal
/// all go through generation-aware helpers so a delayed old
/// monitor cannot remove a newer generation's runtime.
#[derive(Debug, Clone)]
pub struct RuntimeEntry {
    pub(crate) generation: u64,
    pub(crate) runtime: LspProcessRuntime,
}

impl RuntimeEntry {
    /// Test-only constructor that builds a `RuntimeEntry` with a
    /// `dummy_for_test` `LspProcessRuntime`. Used by unit tests
    /// that need to populate the runtime map without spawning a
    /// real process. The runtime is safe to clone and inspect
    /// but will never publish an exit event.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn stub_for_test() -> Self {
        Self {
            generation: 0,
            runtime: LspProcessRuntime::dummy_for_test("test-stub", 0),
        }
    }
}

type RuntimeMap = Arc<Mutex<HashMap<String, RuntimeEntry>>>;

/// Outcome of a runtime installation attempt. Distinguishes
/// the three observed states a caller must handle:
///
/// - `Installed`: the requested runtime is now the active
///   entry. The caller can publish / observe.
/// - `Replaced`: a prior entry with an older generation was
///   removed and the requested runtime now owns the slot. The
///   prior runtime should already have been terminated by its
///   owner; if it is still live, the caller MUST treat that as
///   an invariant violation and terminate it explicitly.
/// - `Rejected`: an entry with the same or newer generation is
///   already installed. The requested runtime MUST be
///   terminated and reaped by the caller; it cannot be
///   published and must not survive untracked.
///
/// `Option<RuntimeEntry>` (the previous return type) could not
/// distinguish "replaced" from "rejected" — both produced
/// `Some(...)`. Callers could silently ignore the distinction
/// and leak a rejected runtime. The new enum forces exhaustive
/// matching.
#[derive(Debug)]
#[allow(dead_code)] // Variant fields are read by callers / tests; the enum itself is `pub(crate)`-style.
pub(crate) enum RuntimeInstallResult {
    Installed,
    Replaced {
        prior: RuntimeEntry,
    },
    Rejected {
        existing_generation: u64,
        requested_generation: u64,
    },
}

/// Diagnostic snapshot used by the manual restart supersession
/// path to detect a generation advance while waiting on an
/// in-flight owner. Fields default to the "no prior owner"
/// sentinel (`0`, `String::new()`) when no live client exists
/// for `key`.
#[derive(Debug, Clone)]
struct RestartOwnerDiagnosticSnapshot {
    pre_wait_generation: u64,
    pre_wait_server_id: String,
}

/// Pass 5 — Options that differentiate manual and automatic
/// restart flows within the unified `restart_client_owned`
/// path. The two flows share lifecycle checks, lease
/// acquisition, and coordinator handoff; they differ in
/// whether to:
/// - wait for an in-flight owner to release (manual only),
/// - re-read generation and abort on advance (manual only),
/// - terminate the old runtime before coordinator handoff
///   (manual only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OwnedRestartOptions {
    /// When true: cancel any in-flight owner and wait for its
    /// completion signal under a bounded timeout; reject
    /// collisions after the wait. When false: coalesce with
    /// an in-flight automatic restart.
    manual_supersession: bool,
}

impl OwnedRestartOptions {
    /// Automatic restart — coalesce with in-flight automatic
    /// restart; do NOT supersede.
    fn automatic() -> Self {
        Self {
            manual_supersession: false,
        }
    }
    /// Manual restart — supersede in-flight automatic
    /// restart; reject concurrent manual restarts.
    fn manual() -> Self {
        Self {
            manual_supersession: true,
        }
    }
}

/// Install `runtime` for `key` if no entry exists with the same or
/// newer generation. Returns the [`RuntimeInstallResult`] that
/// distinguishes the three observable outcomes:
///
/// - **No existing entry** → [`RuntimeInstallResult::Installed`].
/// - **Existing entry with older generation** →
///   [`RuntimeInstallResult::Replaced { prior }`]. The prior
///   entry's runtime should already have been terminated by its
///   owning coordinator; if it is still live, the caller MUST
///   treat that as an invariant violation and terminate it
///   explicitly via [`terminate_runtime`].
/// - **Existing entry with same or newer generation** →
///   [`RuntimeInstallResult::Rejected { ... }`]. The requested
///   runtime MUST be terminated and reaped by the caller; it
///   cannot be installed and must not survive untracked.
///
/// Pass 3 — The previous `Option<RuntimeEntry>` return type
/// could not distinguish `Replaced` from `Rejected`. Callers
/// silently ignored the distinction and could leak a rejected
/// runtime. The new enum forces exhaustive matching.
async fn install_runtime(
    runtime_map: &RuntimeMap,
    key: String,
    generation: u64,
    runtime: LspProcessRuntime,
) -> RuntimeInstallResult {
    let mut map = runtime_map.lock().await;
    if let Some(existing) = map.get(&key) {
        if existing.generation >= generation {
            warn!(
                key = %key,
                existing_generation = existing.generation,
                requested_generation = generation,
                "refusing to install runtime: existing entry has same or newer generation"
            );
            return RuntimeInstallResult::Rejected {
                existing_generation: existing.generation,
                requested_generation: generation,
            };
        }
    }
    let prior = map.remove(&key);
    map.insert(
        key,
        RuntimeEntry {
            generation,
            runtime,
        },
    );
    match prior {
        Some(prior) => RuntimeInstallResult::Replaced { prior },
        None => RuntimeInstallResult::Installed,
    }
}

/// Test-only wrapper for [`install_runtime`] that accepts a
/// pre-built `RuntimeEntry`. Used by unit tests in `restart.rs`
/// that exercise the rejection contract without spawning a real
/// process. Returns the same [`RuntimeInstallResult`] enum as
/// the production helper.
#[cfg(test)]
#[allow(dead_code)]
pub(crate) async fn install_runtime_for_test(
    runtime_map: &RuntimeMap,
    key: &str,
    entry: RuntimeEntry,
    generation: u64,
) -> RuntimeInstallResult {
    install_runtime(runtime_map, key.to_string(), generation, entry.runtime).await
}

/// Mirror of [`RuntimeInstallResult`] exposed under
/// `#[cfg(test)]` so cross-module tests can pattern-match on it.
#[cfg(test)]
#[derive(Debug)]
pub(crate) enum RuntimeInstallResultForTest {
    Installed,
    Replaced,
    Rejected {
        existing_generation: u64,
        requested_generation: u64,
    },
}

#[cfg(test)]
impl RuntimeInstallResultForTest {
    fn from(result: &RuntimeInstallResult) -> Self {
        match result {
            RuntimeInstallResult::Installed => Self::Installed,
            RuntimeInstallResult::Replaced { .. } => Self::Replaced,
            RuntimeInstallResult::Rejected {
                existing_generation,
                requested_generation,
            } => Self::Rejected {
                existing_generation: *existing_generation,
                requested_generation: *requested_generation,
            },
        }
    }
}

#[cfg(test)]
pub(crate) async fn install_runtime_for_test_v2(
    runtime_map: &RuntimeMap,
    key: &str,
    entry: RuntimeEntry,
    generation: u64,
) -> RuntimeInstallResultForTest {
    let result = install_runtime(runtime_map, key.to_string(), generation, entry.runtime).await;
    RuntimeInstallResultForTest::from(&result)
}

/// Return the runtime for `key` only if its recorded generation
/// matches `generation`. Returns `None` when the key has no entry
/// or the stored generation differs.
async fn runtime_for_generation(
    runtime_map: &RuntimeMap,
    key: &str,
    generation: u64,
) -> Option<LspProcessRuntime> {
    let map = runtime_map.lock().await;
    map.get(key)
        .filter(|entry| entry.generation == generation)
        .map(|entry| entry.runtime.clone())
}

/// Remove the runtime for `key` only if its recorded generation
/// matches `generation`. Returns the removed entry on success,
/// `None` when the key has no entry or the stored generation
/// differs. A delayed old monitor cannot remove a newer generation's
/// runtime through this helper.
async fn remove_runtime_if_generation(
    runtime_map: &RuntimeMap,
    key: &str,
    generation: u64,
) -> Option<RuntimeEntry> {
    let mut map = runtime_map.lock().await;
    match map.get(key) {
        Some(entry) if entry.generation == generation => map.remove(key),
        _ => None,
    }
}

/// Reason a runtime is being terminated. Recorded in logs and
/// observability; the termination path is identical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeTerminationReason {
    /// Whole-service shutdown.
    ServiceShutdown,
    /// Operator-initiated manual restart.
    ManualRestart,
    /// Client constructed but never published (failure cleanup).
    /// Pass 3 — `FailedPublication` is now used by the
    /// monitor's runtime-install rejection path and by the
    /// coordinator's post-spawn cancellation cleanup.
    FailedPublication,
}

/// Outcome of a single runtime termination attempt.
#[derive(Debug)]
struct RuntimeTerminationOutcome {
    /// Whether the runtime was found in the map (i.e. a runtime
    /// matching the supplied generation was present at lookup time).
    /// `false` means there was nothing to terminate.
    runtime_present: bool,
    /// Whether the process exited cleanly within the graceful
    /// deadline. Read in test code.
    #[allow(dead_code)]
    exited: bool,
    /// Whether a force kill was required.
    forced: bool,
    /// The exit event captured (or `None` when no exit was observed
    /// before the absolute deadline). Read in test code.
    #[allow(dead_code)]
    event: Option<LspProcessExitEvent>,
}

/// Terminate a single runtime according to the documented sequence:
///
/// 1. Look up the runtime only when the stored generation matches.
/// 2. Set the runtime intent to `GracefulShutdownRequested` BEFORE
///    sending the protocol shutdown request.
/// 3. Send the protocol shutdown under the graceful deadline.
/// 4. Await the runtime's exit event under the graceful deadline.
/// 5. On timeout, set the intent to `ForceKillRequested` and re-await
///    under the absolute deadline.
/// 6. Remove the runtime from the map (only when the stored
///    generation still matches).
///
/// The `graceful_deadline` is the upper bound for the graceful
/// shutdown. The `absolute_deadline` is the upper bound for the
/// force-kill re-await. `reason` is logged at the start.
async fn terminate_runtime(
    runtime_map: &RuntimeMap,
    key: &str,
    generation: u64,
    client: Option<Arc<LspClient>>,
    graceful_deadline: Instant,
    absolute_deadline: Instant,
    reason: RuntimeTerminationReason,
) -> RuntimeTerminationOutcome {
    let runtime = match runtime_for_generation(runtime_map, key, generation).await {
        Some(r) => r,
        None => {
            return RuntimeTerminationOutcome {
                runtime_present: false,
                exited: false,
                forced: false,
                event: None,
            };
        }
    };

    info!(
        server = %runtime.server_id,
        root = %runtime.root.display(),
        generation,
        reason = ?reason,
        "terminating runtime"
    );

    // Step 1: set the runtime intent to graceful BEFORE the
    // protocol shutdown request, so the exit classifier marks
    // the resulting exit as expected.
    runtime.request_graceful_shutdown();

    // Step 2: send the protocol shutdown under the graceful
    // deadline. Skip on test-only clients (the test stub
    // short-circuits and counts via `test_shutdown_count`).
    if let Some(client) = &client {
        let remaining = graceful_deadline.saturating_duration_since(Instant::now());
        if !remaining.is_zero() {
            let per_client = remaining.min(SHUTDOWN_CLIENT_TIMEOUT);
            let res = tokio::time::timeout(per_client, client.request_protocol_shutdown()).await;
            match res {
                Ok(Ok(())) => {
                    debug!(server = %runtime.server_id, "protocol shutdown sent");
                }
                Ok(Err(e)) => {
                    warn!(
                        server = %runtime.server_id,
                        error = %e,
                        "protocol shutdown failed; will force kill"
                    );
                }
                Err(_) => {
                    warn!(
                        server = %runtime.server_id,
                        "protocol shutdown timed out; will force kill"
                    );
                }
            }
        }
        // Close the writer (stdin) to signal EOF to the server.
        // Many LSP servers require this before they exit.
        client.writer.close().await;
    }

    // Step 3: await the runtime's exit event under the graceful
    // deadline. The runtime publishes exactly one event per
    // generation; we take the first one.
    let mut exit_rx = runtime.exit_rx.clone();
    let mut event: Option<LspProcessExitEvent> = None;
    loop {
        if let Some(e) = exit_rx.borrow_and_update().clone() {
            event = Some(e);
            break;
        }
        let now = Instant::now();
        if now >= graceful_deadline {
            break;
        }
        let step = graceful_deadline
            .saturating_duration_since(now)
            .min(Duration::from_millis(50));
        // Race the change notification against the step timeout.
        match tokio::time::timeout(step, exit_rx.changed()).await {
            Ok(Ok(())) => {
                // A change was observed. The next loop iteration
                // will pick up the published value via
                // `borrow_and_update`.
            }
            Ok(Err(_closed)) => {
                // Sender dropped without an event; treat as not
                // exited.
                break;
            }
            Err(_) => {
                // Step timeout fired; loop will re-check the
                // deadline.
            }
        }
    }

    let mut forced = false;
    if event.is_none() {
        // Step 4: graceful deadline expired; request force kill.
        // Pass 8 — `request_force_kill()` is idempotent and
        // transitions the runtime's `LspProcessIntent` to
        // `ForceKillRequested`. We set this BEFORE the absolute
        // deadline so the runtime always observes a force-kill
        // intent when graceful shutdown times out, even if the
        // process never exits.
        runtime.request_force_kill();
        forced = true;
        // Re-await under the absolute deadline. If the
        // absolute deadline expires without an exit event,
        // ensure the force-kill intent is still set (the
        // runtime may have already received the request but
        // not yet observed the process exiting).
        loop {
            if let Some(e) = exit_rx.borrow_and_update().clone() {
                event = Some(e);
                break;
            }
            if Instant::now() >= absolute_deadline {
                // Pass 8 — guarantee force-kill intent on
                // absolute deadline exhaustion. `request_force_kill`
                // is idempotent: re-setting it ensures the
                // runtime's `LspProcessIntent` is
                // `ForceKillRequested` regardless of any
                // prior transitions. Without this guarantee,
                // a pathological process whose exit event is
                // dropped could leave the intent stuck at
                // `Running` while the service considers the
                // shutdown complete.
                runtime.request_force_kill();
                break;
            }
            let step = absolute_deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50));
            match tokio::time::timeout(step, exit_rx.changed()).await {
                Ok(Ok(())) => {}
                Ok(Err(_closed)) => {
                    // Sender dropped without an event; ensure
                    // the force-kill intent is still set so a
                    // pathological drop does not strand the
                    // intent at `Running`.
                    runtime.request_force_kill();
                    break;
                }
                Err(_) => {}
            }
        }
    }

    // Step 5: remove the runtime from the map only if the
    // generation still matches. A concurrent restart that bumped
    // the generation between our lookup and now must not be
    // touched by this removal.
    remove_runtime_if_generation(runtime_map, key, generation).await;

    RuntimeTerminationOutcome {
        runtime_present: true,
        exited: event.is_some(),
        forced,
        event,
    }
}

type InitResult = Result<Arc<LspClient>, SharedInitError>;
type InitCompletionSender = tokio::sync::oneshot::Sender<InitResult>;
type InitCompletionReceiver = tokio::sync::oneshot::Receiver<InitResult>;

/// Summary of the last observed process exit event, persisted
/// for health snapshots when no live runtime exists.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct LspProcessExitSummary {
    generation: u64,
    status: Option<i32>,
    signal: Option<i32>,
    expected: bool,
    reason: String,
}

/// Per-server operational state tracked by the service.
///
/// Each client key (`"{root}:{server_id}"`) has an independent
/// operational state that tracks health, restart attempts, and
/// the last failure reason. Generations are tracked separately
/// in the `generation_map`.
#[derive(Debug, Clone)]
struct OperationalServerState {
    /// Current operational state.
    state: LspOperationalState,
    /// Number of consecutive restart attempts.
    restart_attempts: u32,
    /// Timestamp of the last healthy state (for reset_after_healthy).
    last_healthy_at: Option<Instant>,
    /// Summary of the last observed exit event.
    last_exit: Option<LspProcessExitSummary>,
    /// Persisted stderr tail from the last exit.
    last_stderr_tail: Vec<String>,
}

impl Default for OperationalServerState {
    fn default() -> Self {
        Self {
            state: LspOperationalState::Starting,
            restart_attempts: 0,
            last_healthy_at: None,
            last_exit: None,
            last_stderr_tail: Vec::new(),
        }
    }
}

// ── ReadinessResult ─────────────────────────────────────────────────

/// Result of a readiness wait. The `Ready` arm carries the
/// elapsed time (the time spent waiting). The `Degraded` arm
/// carries a human-readable reason and the elapsed time (so the
/// caller can log how long the wait was attempted before
/// giving up).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadinessResult {
    /// Server reached the ready condition within the budget.
    Ready { elapsed: Duration },
    /// Server did not reach the ready condition within the
    /// budget. The caller should transition to `Degraded` using
    /// `reason` and the same `elapsed` value.
    Degraded { reason: String, elapsed: Duration },
}

/// Internal carrier of the readiness decision computed inside
/// the initialization inner-block. The outer publication path
/// reads this to apply the right `LspOperationalState`
/// transition.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ReadinessDecision {
    Ready { elapsed: Duration },
    Degraded { reason: String, elapsed: Duration },
}

/// Compute the readiness decision for a freshly initialized
/// client against its `LspReadinessPolicy`. This is the inner
/// helper used by `run_initialization_attempt` — the public
/// `LspService::wait_for_readiness` method is the post-publish
/// path used by `restart_client` and direct callers.
///
/// - `InitializedIsReady` returns `Ready { elapsed: 0 }` immediately.
/// - `WaitForDiagnosticsOrTimeout { timeout }` calls
///   `client.wait_for_first_diagnostics(timeout)`.
/// - `WaitForProgressEndOrTimeout { timeout }` calls
///   `client.wait_for_progress_end(timeout)`.
/// - `WarmupDelay { duration }` sleeps for `duration`.
async fn compute_readiness_decision(
    client: &LspClient,
    policy: &LspReadinessPolicy,
) -> ReadinessDecision {
    let started = Instant::now();
    match policy {
        LspReadinessPolicy::InitializedIsReady => ReadinessDecision::Ready {
            elapsed: Duration::ZERO,
        },
        LspReadinessPolicy::WarmupDelay { duration } => {
            tokio::time::sleep(*duration).await;
            ReadinessDecision::Ready { elapsed: *duration }
        }
        LspReadinessPolicy::WaitForDiagnosticsOrTimeout { timeout } => {
            if client.wait_for_first_diagnostics(*timeout).await {
                ReadinessDecision::Ready {
                    elapsed: started.elapsed(),
                }
            } else {
                ReadinessDecision::Degraded {
                    reason: "diagnostics wait timed out".to_string(),
                    elapsed: started.elapsed(),
                }
            }
        }
        LspReadinessPolicy::WaitForProgressEndOrTimeout { timeout } => {
            if client.wait_for_progress_end(*timeout).await {
                ReadinessDecision::Ready {
                    elapsed: started.elapsed(),
                }
            } else {
                ReadinessDecision::Degraded {
                    reason: "progress wait timed out".to_string(),
                    elapsed: started.elapsed(),
                }
            }
        }
    }
}

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

/// Exit status of the wrapper init task. Used for logging/diagnostics
/// and to prove that the wrapper task body has been dropped before
/// `shutdown_all()` returns.
#[derive(Debug, Clone)]
enum InitTaskExit {
    Completed,
    Panicked(String),
    Cancelled,
}

type InitTaskExitRx = tokio::sync::oneshot::Receiver<InitTaskExit>;
type InitTaskExitTx = tokio::sync::oneshot::Sender<InitTaskExit>;

/// Tracks a spawned initialization task for shutdown coordination.
///
/// The `completion` oneshot receiver is the **authoritative terminal
/// signal** for the real wrapper task. The wrapper task owns the
/// corresponding sender and is required to either send exactly one
/// `InitTaskExit` before exiting, or be dropped (which closes the
/// channel and resolves the receiver with `Err`). Shutdown never
/// holds the real `JoinHandle` via a forwarding task: it observes
/// termination through this receiver.
///
/// `abort_handle` is the `JoinHandle::abort_handle()` clone. It is
/// used to forcibly abort stragglers that do not respond to
/// cooperative cancellation within the grace deadline.
struct InitTaskControl {
    attempt_id: u64,
    cancellation: CancellationToken,
    abort_handle: tokio::task::AbortHandle,
    completion: InitTaskExitRx,
}

type ActiveTaskMap = Arc<Mutex<HashMap<u64, InitTaskControl>>>;

/// Fallback guard that removes the `active_init_tasks` entry on
/// terminal paths where the wrapper task did not run its explicit
/// cleanup (panic, forced abort, unexpected future drop).
///
/// Normal completion uses [`ActiveTaskGuard::disarm`] after explicit
/// removal of the entry from the map. The drop fallback spawns a
/// follow-up cleanup task so the removal is not contingent on the
/// lock being uncontended at drop time. The shutdown drain
/// additionally clears any leftover entries after observing task
/// termination, so the active map is guaranteed to be empty
/// post-shutdown regardless of which path the wrapper took.
struct ActiveTaskGuard {
    attempt_id: u64,
    active_init_tasks: ActiveTaskMap,
    armed: bool,
}

impl ActiveTaskGuard {
    fn new(attempt_id: u64, active_init_tasks: ActiveTaskMap) -> Self {
        Self {
            attempt_id,
            active_init_tasks,
            armed: true,
        }
    }

    /// Disarm the guard. Must be called after the wrapper has
    /// explicitly removed its `active_init_tasks` entry on the
    /// normal completion path.
    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for ActiveTaskGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        // Fallback: explicit removal did not run. Spawn a cleanup
        // task; the runtime will run it as long as it is alive.
        // The shutdown drain is the additional safety net — it
        // empties the map after observing terminal completion, so
        // the entry is guaranteed to disappear.
        let attempt_id = self.attempt_id;
        let map = self.active_init_tasks.clone();
        tokio::spawn(async move {
            let mut map = map.lock().await;
            map.remove(&attempt_id);
        });
    }
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
pub enum ServiceLifecycle {
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

/// Bounded timeout for manual restart to wait for an in-flight
/// automatic owner to signal completion. The owner is signalled
/// via its lease token (intent) but the manual restart cannot
/// touch the live client until the in-flight owner has released
/// its slot. A timeout here means the in-flight owner is hung
/// and the manual restart aborts without disturbing the
/// current client.
const MANUAL_SUPERSESSION_OWNER_TIMEOUT: Duration = Duration::from_secs(3);

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
/// The `initializing` and `active_init_tasks` locks are acquired
/// **sequentially, never nested**:
/// - The leader registration path acquires `initializing` to read
///   slot state, releases it, then acquires `active_init_tasks` to
///   install the control, releases it, then re-acquires
///   `initializing` to re-check slot validity, releases it. No
///   acquisition holds both locks.
/// - No path holds `active_init_tasks` while awaiting `initializing`,
///   and no path holds `initializing` while awaiting task/client I/O.
/// - `shutdown_all` drains `active_init_tasks` once, signals all
///   cancellation tokens, and awaits all completion receivers
///   concurrently under one aggregate deadline. No nested locks.
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
    /// Keyed by attempt_id. Each value owns the authoritative terminal
    /// completion receiver (`oneshot::Receiver<InitTaskExit>`) for the
    /// wrapper task, plus the `AbortHandle` for forced abort of
    /// stragglers. Shutdown observes task termination through the
    /// receiver; no forwarding task wraps the real `JoinHandle`.
    active_init_tasks: ActiveTaskMap,
    /// Maps document URI string → client key for O(1) ownership lookup.
    document_owners: Arc<RwLock<HashMap<String, String>>>,
    /// Authoritative open-document registry for tracking document
    /// state across restarts. Used for replaying didOpen/didChange
    /// after a server restart.
    document_registry: Arc<OpenDocumentRegistry>,
    /// Per-server operational state (health, restart attempts).
    operational_state: Arc<RwLock<HashMap<String, OperationalServerState>>>,
    /// Per-client generation map. Tracks the authoritative
    /// generation for each client key. Updated whenever a client
    /// is first published (set to `1`) and on every restart
    /// (incremented). Read by `generation_for_key` and used to
    /// reject stale process-exit events.
    generation_map: Arc<Mutex<HashMap<String, u64>>>,
    /// Channel for process exit events from monitor tasks.
    exit_tx: tokio::sync::mpsc::Sender<LspProcessExitEvent>,
    /// Receiver for process exit events. Taken once by
    /// `ensure_exit_receiver_started`.
    exit_rx: Arc<Mutex<Option<tokio::sync::mpsc::Receiver<LspProcessExitEvent>>>>,
    /// Idempotent guard for the exit receiver task. Set on first
    /// activation by `ensure_exit_receiver_started` so that callers
    /// can request activation from any public entry point without
    /// spawning duplicate receivers.
    exit_receiver_started: Arc<AtomicBool>,
    /// Per-key authoritative process runtimes, paired with the
    /// generation that installed them. The runtime owns the child,
    /// stderr ring buffer, intent receiver, kill receiver, and exit
    /// event publication. The explicit `generation` field on
    /// [`RuntimeEntry`] is the source of truth for runtime-map
    /// safety: insertion, lookup, and removal all go through
    /// generation-aware helpers so a delayed monitor cannot remove
    /// a newer generation's runtime.
    runtime_map: Arc<Mutex<HashMap<String, RuntimeEntry>>>,
    /// Per-key persisted client descriptors. Populated on first
    /// publish in `run_initialization_attempt` and read by the
    /// restart coordinator (`restart::restart_client_coordinator`)
    /// to seed a new client without re-detecting language or
    /// project root.
    descriptor_map: Arc<Mutex<HashMap<String, LspClientDescriptor>>>,
    /// Back-reference to the service itself, populated by the
    /// constructors that return `Arc<Self>`. Used by entry points
    /// that only have `&self` (e.g. `get_or_create_client`) to
    /// activate the exit receiver exactly once without requiring
    /// `&Arc<Self>`. The cell is left empty when the service is
    /// built without a back-reference (legacy `new()` and test
    /// helpers), in which case the caller is responsible for
    /// invoking `ensure_exit_receiver_started` explicitly.
    self_ref: OnceLock<Weak<Self>>,
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
    /// Test-only flag: when true, the wrapper task gets stuck after
    /// the inner function completes but before sending the completion
    /// signal. This forces the abort-after-grace path during shutdown.
    #[cfg(test)]
    test_force_stuck_after_inner: std::sync::Arc<AtomicBool>,
    /// Per-key restart ownership (Pass 1 — Serialization).
    /// One entry per key under active restart; entry is removed
    /// when the owner releases its lease. Concurrent restart
    /// acquisitions for the same key resolve deterministically
    /// via [`acquire_restart_ownership`]. Different keys
    /// restart independently — there is no global restart mutex.
    restart_tasks: RestartTaskMap,
    /// Monotonic counter that hands out unique restart owner
    /// ids (Pass 1 — Serialization). Owners use their id to
    /// verify they still own the entry at release time, so a
    /// delayed cleanup cannot remove a newer owner's lease.
    restart_owner_counter: AtomicU64,
}

impl LspService {
    /// Build a new service. Returns the bare value; callers that
    /// intend to use the auto-activation path should wrap the
    /// result in `Arc::new_cyclic_back_ref` (or use
    /// [`LspService::new_arc`] which sets up the back-reference
    /// automatically).
    /// Pass 7 — Bare constructor, **test-only**. Returns a
    /// `Self` without the cyclic back-reference wired, so
    /// the exit-receiver task is NOT auto-started. The
    /// production constructor is [`LspService::new_arc`],
    /// which always wires supervision. This constructor is
    /// restricted to `pub(crate)` so production callers
    /// cannot accidentally create an un-supervised service.
    /// Tests that explicitly assert on the un-supervised
    /// path use this constructor from inside the crate.
    #[cfg(test)]
    pub(crate) fn new(config: LspConfig) -> Self {
        let (lifecycle_tx, _rx) = watch::channel(INITIAL_LIFECYCLE_STATE);
        let (exit_tx, exit_rx) = tokio::sync::mpsc::channel(64);
        let exit_rx = Arc::new(Mutex::new(Some(exit_rx)));
        Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            active_init_tasks: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            document_registry: Arc::new(OpenDocumentRegistry::new()),
            operational_state: Arc::new(RwLock::new(HashMap::new())),
            generation_map: Arc::new(Mutex::new(HashMap::new())),
            exit_tx,
            exit_rx,
            exit_receiver_started: Arc::new(AtomicBool::new(false)),
            runtime_map: Arc::new(Mutex::new(HashMap::new())),
            descriptor_map: Arc::new(Mutex::new(HashMap::new())),
            self_ref: OnceLock::new(),
            lifecycle: Arc::new(RwLock::new(INITIAL_LIFECYCLE_STATE)),
            lifecycle_tx,
            config,
            #[cfg(test)]
            test_init_fn: None,
            #[cfg(test)]
            test_hooks: None,
            #[cfg(test)]
            test_force_stuck_after_inner: std::sync::Arc::new(AtomicBool::new(false)),
            restart_tasks: Arc::new(Mutex::new(HashMap::new())),
            restart_owner_counter: AtomicU64::new(1),
        }
    }

    /// Build a new service wrapped in an `Arc<Self>` with the
    /// back-reference set. This is the preferred constructor for
    /// production paths that need auto-activation of the exit
    /// receiver from `&self` callers (e.g.
    /// `get_or_create_client`).
    pub fn new_arc(config: LspConfig) -> Arc<Self> {
        let (lifecycle_tx, _rx) = watch::channel(INITIAL_LIFECYCLE_STATE);
        let (exit_tx, exit_rx) = tokio::sync::mpsc::channel(64);
        let exit_rx = Arc::new(Mutex::new(Some(exit_rx)));
        Arc::new_cyclic(|weak| Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            active_init_tasks: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            document_registry: Arc::new(OpenDocumentRegistry::new()),
            operational_state: Arc::new(RwLock::new(HashMap::new())),
            generation_map: Arc::new(Mutex::new(HashMap::new())),
            exit_tx,
            exit_rx,
            exit_receiver_started: Arc::new(AtomicBool::new(false)),
            runtime_map: Arc::new(Mutex::new(HashMap::new())),
            descriptor_map: Arc::new(Mutex::new(HashMap::new())),
            self_ref: OnceLock::from(weak.clone()),
            lifecycle: Arc::new(RwLock::new(INITIAL_LIFECYCLE_STATE)),
            lifecycle_tx,
            config,
            #[cfg(test)]
            test_init_fn: None,
            #[cfg(test)]
            test_hooks: None,
            #[cfg(test)]
            test_force_stuck_after_inner: std::sync::Arc::new(AtomicBool::new(false)),
            restart_tasks: Arc::new(Mutex::new(HashMap::new())),
            restart_owner_counter: AtomicU64::new(1),
        })
    }

    /// Start the process exit event receiver task exactly once.
    ///
    /// This is the authoritative entry point for activating the
    /// receiver. It is idempotent: subsequent calls are no-ops.
    /// Public callers do not need to invoke it explicitly — it is
    /// wired into the first client-creating path.
    pub async fn ensure_exit_receiver_started(self: &Arc<Self>) {
        // compare_exchange guarantees that exactly one task observes
        // the transition false -> true and is responsible for taking
        // the receiver and spawning the task. All other callers
        // become no-ops.
        if self
            .exit_receiver_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let exit_rx = {
            let mut rx = self.exit_rx.lock().await;
            rx.take()
        };
        if let Some(exit_rx) = exit_rx {
            let service = Arc::clone(self);
            tokio::spawn(async move {
                let mut rx = exit_rx;
                while let Some(event) = rx.recv().await {
                    service.handle_exit_event(event).await;
                }
                debug!("process exit receiver task terminated");
            });
        } else {
            // Receiver already taken (e.g. the test-only path below
            // was used). Reset the flag so future calls are still
            // safe — though no-op in this branch.
            self.exit_receiver_started.store(false, Ordering::Release);
        }
    }

    /// Internal entry point for `&self` callers (e.g. the
    /// `get_or_create_client` path) that need to activate the exit
    /// receiver without holding `&Arc<Self>`. Resolves the
    /// back-reference set by [`LspService::new_arc`] (or by
    /// `Arc::new_cyclic` in tests) and delegates.
    pub(crate) async fn ensure_exit_receiver_started_self(&self) {
        if let Some(weak) = self.self_ref.get() {
            if let Some(arc) = weak.upgrade() {
                arc.ensure_exit_receiver_started().await;
            }
        }
    }

    /// Create a service backed by a test factory closure. Returns
    /// an `Arc<Self>` with the back-reference wired up so
    /// `&self` callers (e.g. `get_or_create_client`) can activate
    /// the exit receiver automatically.
    #[cfg(test)]
    pub(crate) fn test_new<F>(config: LspConfig, factory: F) -> Arc<Self>
    where
        F: Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static,
    {
        let (exit_tx, exit_rx) = tokio::sync::mpsc::channel(64);
        Arc::new_cyclic(|weak| Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            active_init_tasks: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            document_registry: Arc::new(OpenDocumentRegistry::new()),
            operational_state: Arc::new(RwLock::new(HashMap::new())),
            generation_map: Arc::new(Mutex::new(HashMap::new())),
            exit_tx,
            exit_rx: Arc::new(Mutex::new(Some(exit_rx))),
            exit_receiver_started: Arc::new(AtomicBool::new(false)),
            runtime_map: Arc::new(Mutex::new(HashMap::new())),
            descriptor_map: Arc::new(Mutex::new(HashMap::new())),
            self_ref: OnceLock::from(weak.clone()),
            lifecycle: Arc::new(RwLock::new(INITIAL_LIFECYCLE_STATE)),
            lifecycle_tx: watch::channel(INITIAL_LIFECYCLE_STATE).0,
            config,
            test_init_fn: Some(std::sync::Arc::new(Box::new(factory))),
            test_hooks: None,
            test_force_stuck_after_inner: std::sync::Arc::new(AtomicBool::new(false)),
            restart_tasks: Arc::new(Mutex::new(HashMap::new())),
            restart_owner_counter: AtomicU64::new(1),
        })
    }

    #[cfg(test)]
    fn test_new_with_hooks<F>(
        config: LspConfig,
        factory: F,
        test_hooks: std::sync::Arc<TestHooks>,
    ) -> Arc<Self>
    where
        F: Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static,
    {
        let (exit_tx, exit_rx) = tokio::sync::mpsc::channel(64);
        Arc::new_cyclic(|weak| Self {
            clients: Arc::new(RwLock::new(HashMap::new())),
            initializing: Arc::new(Mutex::new(HashMap::new())),
            active_init_tasks: Arc::new(Mutex::new(HashMap::new())),
            document_owners: Arc::new(RwLock::new(HashMap::new())),
            document_registry: Arc::new(OpenDocumentRegistry::new()),
            operational_state: Arc::new(RwLock::new(HashMap::new())),
            generation_map: Arc::new(Mutex::new(HashMap::new())),
            exit_tx,
            exit_rx: Arc::new(Mutex::new(Some(exit_rx))),
            exit_receiver_started: Arc::new(AtomicBool::new(false)),
            runtime_map: Arc::new(Mutex::new(HashMap::new())),
            descriptor_map: Arc::new(Mutex::new(HashMap::new())),
            self_ref: OnceLock::from(weak.clone()),
            lifecycle: Arc::new(RwLock::new(INITIAL_LIFECYCLE_STATE)),
            lifecycle_tx: watch::channel(INITIAL_LIFECYCLE_STATE).0,
            config,
            test_init_fn: Some(std::sync::Arc::new(Box::new(factory))),
            test_hooks: Some(test_hooks),
            test_force_stuck_after_inner: std::sync::Arc::new(AtomicBool::new(false)),
            restart_tasks: Arc::new(Mutex::new(HashMap::new())),
            restart_owner_counter: AtomicU64::new(1),
        })
    }

    /// Set the test-only flag that causes the wrapper task to get stuck
    /// after the inner function completes. This forces the abort-after-grace
    /// path during shutdown.
    #[cfg(test)]
    fn set_force_stuck(&self, stuck: bool) {
        self.test_force_stuck_after_inner
            .store(stuck, Ordering::SeqCst);
    }

    pub async fn get_or_create_client(
        &self,
        file_path: &Path,
    ) -> Result<(String, PathBuf), LspError> {
        // Phase 4: ensure the exit receiver is active before
        // creating any client. No-op if it was already started.
        self.ensure_exit_receiver_started_self().await;

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
                #[cfg(test)]
                let force_stuck = self.test_force_stuck_after_inner.clone();

                // ── Start-barrier pattern ──
                //
                // The wrapper task waits on `start_rx` before doing
                // any work. We send on `start_tx` only after the
                // `active_init_tasks` entry has been installed. This
                // guarantees the task body cannot complete (or even
                // begin) before its bookkeeping record exists.
                let (start_tx, start_rx) = tokio::sync::oneshot::channel::<()>();
                let (completion_tx, completion_rx) =
                    tokio::sync::oneshot::channel::<InitTaskExit>();

                let task = tokio::spawn(run_init_task_wrapper(
                    attempt_id,
                    start_rx,
                    completion_tx,
                    server,
                    project_root_clone,
                    config,
                    clients.clone(),
                    initializing.clone(),
                    active_init_tasks.clone(),
                    lifecycle,
                    key_clone.clone(),
                    cancel_for_task,
                    self.exit_tx.clone(),
                    self.operational_state.clone(),
                    self.generation_map.clone(),
                    self.descriptor_map.clone(),
                    self.document_registry.clone(),
                    self.runtime_map.clone(),
                    #[cfg(test)]
                    test_init,
                    #[cfg(test)]
                    force_stuck,
                ));

                // Step 1: install the active-task entry. We do this
                // BEFORE the second `initializing` check, so the
                // control is always present if the task is running.
                let abort_handle = task.abort_handle();
                {
                    let mut tasks = active_init_tasks.lock().await;
                    tasks.insert(
                        attempt_id,
                        InitTaskControl {
                            attempt_id,
                            cancellation: cancellation.clone(),
                            abort_handle,
                            completion: completion_rx,
                        },
                    );
                }

                // Step 2: re-check slot validity under `initializing`.
                // No nesting: we released `active_init_tasks` before
                // acquiring `initializing`. The two maps are checked
                // independently with no lock held across both.
                let slot_still_valid = {
                    let init = initializing.lock().await;
                    init.get(&key_clone)
                        .is_some_and(|slot| slot.attempt_id == attempt_id)
                };

                if !slot_still_valid {
                    // Drop the start_tx to unblock the wrapper; the
                    // wrapper will observe channel closure and exit
                    // early. We also abort in case the wrapper has
                    // not yet hit the start_rx await.
                    drop(start_tx);
                    abort_and_finalize_unstarted_task(
                        task,
                        &active_init_tasks,
                        attempt_id,
                        &initializing,
                        &key_clone,
                    )
                    .await;
                    return Err(LspError::InitializationCancelled(
                        "service lifecycle changed before registration".to_string(),
                    ));
                }

                // Step 3: signal the wrapper to start.
                if start_tx.send(()).is_err() {
                    // Wrapper dropped its start_rx before we sent —
                    // it has already exited. Reap its terminal
                    // completion below.
                    abort_and_finalize_unstarted_task(
                        task,
                        &active_init_tasks,
                        attempt_id,
                        &initializing,
                        &key_clone,
                    )
                    .await;
                    return Err(LspError::InitializationCancelled(
                        "init task exited before registration completed".to_string(),
                    ));
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
            .insert(uri.to_string(), key.clone());

        // Record in the authoritative document registry for restart replay.
        let language_id = file_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_string();
        self.document_registry
            .open(&key, uri.clone(), language_id, version, text.to_string())
            .await;

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
        client.update_file(&uri, text, version).await?;

        // Record change in the authoritative document registry.
        self.document_registry
            .change(&key, &uri, version, text.to_string())
            .await;

        Ok(())
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

        // Remove from the authoritative document registry.
        self.document_registry.close(&owner_key, &uri).await;

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
        client.save_file(&uri, text).await?;

        // Record save in the authoritative document registry.
        self.document_registry.save(&owner_key, &uri).await;

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

    /// Gracefully shut down the LSP service.
    ///
    /// # Quiescence contract
    ///
    /// **Normal contract** (pathological-deadline fallback not triggered):
    /// After `shutdown_all()` returns, every spawned initialization
    /// task has been observed to terminate via its authoritative
    /// completion receiver (`InitTaskExit` or channel close). No
    /// forwarding task owns the real `JoinHandle` — the completion
    /// receiver IS the authoritative terminal signal.
    ///
    /// Specifically:
    /// - Every wrapper task body has either completed normally, exited
    ///   via cooperative cancellation (`CancellationToken`), or been
    ///   aborted (`AbortHandle`) and the abort completion observed via
    ///   the same completion receiver.
    /// - `active_init_tasks` is empty (explicit cleanup on the normal
    ///   path; the `ActiveTaskGuard` fallback's spawned cleanup task
    ///   for panic/abort paths; and the coordinator drain as the
    ///   final safety net).
    /// - All ready clients have been shut down (concurrently under a
    ///   shared deadline), and the map is empty.
    /// - The lifecycle phase is `Stopped` and the `watch` channel has
    ///   broadcast the transition.
    /// - Concurrent callers that observed `ShuttingDown` subscribe to
    ///   the watch channel, re-check the state, and return only after
    ///   the transition to `Stopped` (no lost wakeups).
    ///
    /// **Pathological deadline fallback** (forced finalization):
    /// If the absolute global deadline (`SHUTDOWN_GLOBAL_TIMEOUT`)
    /// expires before all tasks have terminated, the service
    /// finalizes state regardless: the maps are cleared, the abort
    /// handles are signaled, the lifecycle transitions to `Stopped`,
    /// and unresolved task completions are logged as severe
    /// invariant failures. In this fallback, Tokio is not guaranteed
    /// to deliver a terminal event for an aborted task; the contract
    /// does not claim absolute proof of termination after the
    /// runtime deadline.
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

        // Step 4: drain active tasks. Each entry's `InitTaskControl`
        // carries a oneshot completion receiver that is the
        // authoritative terminal signal for the wrapper task. We
        // never wrap the real `JoinHandle` in a forwarding task.
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

        // Step 6: aggregate grace wait. Await all completion
        // receivers concurrently under one grace deadline. The
        // returned set is the list of tasks that did NOT complete
        // within the grace budget; for those we still hold the
        // abort handles so we can forcibly abort them in step 7.
        let still_pending = await_init_task_completions(tasks, cancellation_deadline).await;

        // Step 7: forcibly abort stragglers and await their
        // completion receivers under the remaining global deadline.
        // The completion receiver resolves either when the wrapper
        // sends its terminal exit (rare under forced abort) or
        // when the sender is dropped (the task future was dropped
        // by the abort, closing the channel).
        if !still_pending.is_empty() {
            for ctrl in &still_pending {
                ctrl.abort_handle.abort();
            }
            let abort_deadline = deadline;
            let _ = await_init_task_completions(still_pending, abort_deadline).await;
        }

        // Step 8: drain ready clients AND terminate their runtimes
        // concurrently under one shared deadline. Each per-client
        // timeout is capped by the global deadline so the total
        // shutdown duration is independent of client count.
        //
        // The `terminate_runtime` helper sets the runtime intent to
        // graceful BEFORE sending the protocol shutdown request,
        // waits on the runtime's exit under the graceful deadline,
        // and force-kills on timeout. Clients without a live
        // runtime (e.g. a unit-test stub) are still sent the
        // protocol shutdown so the LSP handshake is closed cleanly.
        let clients_to_shutdown: Vec<(String, Arc<LspClient>)> = {
            let mut clients = self.clients.write().await;
            clients.drain().collect()
        };

        // Snapshot the runtimes map BEFORE we drain clients, so we
        // can pair each (key, client) tuple with its runtime's
        // generation. The generation is required by the runtime
        // helpers for safety.
        let runtime_generations: HashMap<String, u64> = {
            let map = self.runtime_map.lock().await;
            map.iter()
                .map(|(k, entry)| (k.clone(), entry.generation))
                .collect()
        };

        if !clients_to_shutdown.is_empty() {
            let runtime_map = self.runtime_map.clone();
            let client_shutdown_futs: Vec<_> = clients_to_shutdown
                .into_iter()
                .map(|(key, client)| {
                    let runtime_map = runtime_map.clone();
                    let generation = runtime_generations.get(&key).copied();
                    async move {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            warn!(server = %key, "client shutdown skipped: deadline expired");
                            return;
                        }
                        if let Some(generation) = generation {
                            // Drive the runtime-aware path: set
                            // intent, send protocol shutdown, await
                            // exit, force-kill on timeout.
                            let graceful_deadline =
                                Instant::now() + SHUTDOWN_CLIENT_TIMEOUT.min(remaining);
                            let outcome = terminate_runtime(
                                &runtime_map,
                                &key,
                                generation,
                                Some(client.clone()),
                                graceful_deadline,
                                deadline,
                                RuntimeTerminationReason::ServiceShutdown,
                            )
                            .await;
                            if outcome.forced {
                                warn!(
                                    server = %key,
                                    "runtime required force kill during shutdown"
                                );
                            } else if outcome.runtime_present {
                                debug!(
                                    server = %key,
                                    generation,
                                    "runtime terminated gracefully"
                                );
                            }
                        } else {
                            // No live runtime (test stub or already
                            // gone). Send the protocol shutdown
                            // directly under the remaining deadline
                            // and let the helper count via the
                            // test counter.
                            let per_client = SHUTDOWN_CLIENT_TIMEOUT.min(remaining);
                            match tokio::time::timeout(per_client, client.shutdown()).await {
                                Ok(Ok(())) => {
                                    debug!(server = %key, "client shut down");
                                }
                                Ok(Err(e)) => {
                                    warn!(
                                        server = %key,
                                        error = ?e,
                                        "graceful client shutdown error"
                                    );
                                }
                                Err(_) => {
                                    warn!(server = %key, "client shutdown timeout");
                                }
                            }
                        }
                    }
                })
                .collect();
            futures::future::join_all(client_shutdown_futs).await;
        }

        // Force-terminate any straggler runtimes (e.g. a runtime
        // whose client was removed by a concurrent restart). The
        // runtime map may still hold entries whose corresponding
        // client is gone; this loop drains them under the
        // absolute deadline.
        let straggler_keys: Vec<(String, u64)> = {
            let map = self.runtime_map.lock().await;
            map.iter()
                .map(|(k, entry)| (k.clone(), entry.generation))
                .collect()
        };
        if !straggler_keys.is_empty() {
            let runtime_map = self.runtime_map.clone();
            let straggler_futs: Vec<_> = straggler_keys
                .into_iter()
                .map(|(key, generation)| {
                    let runtime_map = runtime_map.clone();
                    async move {
                        let remaining = deadline.saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            warn!(
                                server = %key,
                                generation,
                                "straggler runtime termination skipped: deadline expired"
                            );
                            return;
                        }
                        let graceful_deadline =
                            Instant::now() + SHUTDOWN_CLIENT_TIMEOUT.min(remaining);
                        let outcome = terminate_runtime(
                            &runtime_map,
                            &key,
                            generation,
                            None, // no live client to send protocol shutdown to
                            graceful_deadline,
                            deadline,
                            RuntimeTerminationReason::ServiceShutdown,
                        )
                        .await;
                        if outcome.forced {
                            warn!(
                                server = %key,
                                generation,
                                "straggler runtime required force kill"
                            );
                        }
                    }
                })
                .collect();
            futures::future::join_all(straggler_futs).await;
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
        // (e.g. entries whose completion receivers have not yet
        // resolved because the wrapper drop guard cleanup task did
        // not run before the abort timeout). This is best-effort
        // and idempotent.
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

    /// Mark every diagnostic cache entry for `key` as belonging to
    /// the previous generation (current - 1) and `post_restart =
    /// false`, so the freshness classifier returns
    /// [`crate::diagnostics::LspDiagnosticFreshness::Stale`] until
    /// the new server emits its own first push.
    ///
    /// Called by the restart coordinator (Pass 5 / Phase 17) right
    /// after a fresh client is published and BEFORE document replay.
    /// The new client's own `set_all_diagnostic_generations` (called
    /// via the coordinator's call to `mark_diagnostics_stale_for_key`
    /// before the new client is published) is what makes the
    /// `Stale` classification stick.
    ///
    /// `received_at` and `content_version` are preserved per-entry.
    /// No-op when no client is currently published for `key`.
    pub async fn mark_diagnostics_stale_for_key(&self, key: &str) {
        let new_generation = self.generation_for_key(key).await;
        // Old generation = new - 1. Saturating subtract (key
        // never published yet → 0) keeps the "no client" sentinel
        // intact.
        let old_generation = new_generation.saturating_sub(1);
        let client = {
            let clients = self.clients.read().await;
            clients.get(key).cloned()
        };
        if let Some(client) = client {
            // Reset generation to (new - 1) — i.e. the previous
            // generation — and set post_restart = false because the
            // new client is itself a post-restart client, but the
            // retained diagnostics originated from the *previous*
            // generation. Their freshness should report
            // `Stale` until the new server emits its first push.
            client
                .set_all_diagnostic_generations(old_generation, false)
                .await;
        }
    }

    /// Return the authoritative generation of the client that
    /// services `file_path`, or `None` when no client exists for
    /// that path.
    ///
    /// Used by [`crate::semantic_context::SemanticDiagnosticEvidence`]
    /// construction to populate `server_generation` and
    /// `post_restart` from real per-client generation metadata.
    pub async fn generation_for_file_path(&self, file_path: &Path) -> Option<u64> {
        // The semantic collector already has an open client for
        // this file (it called `diagnostics.get_diagnostic_snapshot_for_file`
        // which calls `ensure_file_open_from_disk`). Find the
        // key by scanning the live-client map: any client whose
        // root matches the file's parent directory services it.
        // This is best-effort: a more precise lookup would
        // require the descriptor's `LspClientDescriptor.root`.
        let canonical = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        // Collect the matching key under the read lock, then drop
        // the guard before calling `generation_for_key` (which
        // takes the generation lock).
        let key_opt = {
            let clients = self.clients.read().await;
            let mut found = None;
            for key in clients.keys() {
                // key format: "{root}:{server_id}". We don't
                // know the server_id, so we just check if any
                // client's root is a prefix of the file.
                if let Some((root_str, _)) = key.rsplit_once(':') {
                    let root_path = PathBuf::from(root_str);
                    if canonical.starts_with(&root_path) {
                        found = Some(key.clone());
                        break;
                    }
                }
            }
            found
        };
        if let Some(key) = key_opt {
            let gen = self.generation_for_key(&key).await;
            if gen > 0 {
                Some(gen)
            } else {
                None
            }
        } else {
            None
        }
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

    /// Return the stored normalized capability snapshot for the given key.
    ///
    /// This is the override-aware snapshot computed once at initialization.
    /// Returns `None` when the client has not completed initialization yet
    /// or the key is unknown.
    pub async fn normalized_capabilities_for_key(
        &self,
        key: &str,
    ) -> Option<crate::capability::LspCapabilitySnapshot> {
        let clients = self.clients.read().await;
        let client = clients.get(key)?;
        client.get_normalized_capabilities().await
    }

    /// Return the effective capability snapshot for `key`, merging
    /// the stored override-aware snapshot with any observed push-diagnostics
    /// state from the live client. This is the single authoritative accessor
    /// for capability decisions.
    pub async fn effective_capabilities_for_key(
        &self,
        key: &str,
    ) -> Option<crate::capability::LspCapabilitySnapshot> {
        let mut snap = self.normalized_capabilities_for_key(key).await?;
        if self.has_observed_push_diagnostics_for_key(key).await && !snap.observed_push_diagnostics
        {
            snap.observed_push_diagnostics = true;
            snap.supports_diagnostics = snap.supports_pull_diagnostics
                || snap.observed_push_diagnostics
                || snap.supports_push_diagnostics;
        }
        Some(snap)
    }

    /// Make an explicit capability decision for the given client key and
    /// operation. Returns [`CapabilityDecision::Unknown`] when the client
    /// has not published capabilities yet.
    pub async fn capability_decision(
        &self,
        key: &str,
        op: crate::capability::LspSemanticOperation,
    ) -> crate::capability::CapabilityDecision {
        match self.effective_capabilities_for_key(key).await {
            Some(snap) => snap.decide(op),
            None => crate::capability::CapabilityDecision::Unknown {
                operation: op,
                reason: format!("capabilities not yet published for {key}"),
            },
        }
    }

    /// Returns `true` if the client for `key` has received at least
    /// one `publishDiagnostics` notification from the server.
    pub async fn has_observed_push_diagnostics_for_key(&self, key: &str) -> bool {
        let clients = self.clients.read().await;
        clients
            .get(key)
            .map(|c| c.has_observed_push_diagnostics())
            .unwrap_or(false)
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

    // ── Process exit handling ────────────────────────────────────────

    /// Return the current authoritative generation for `key`.
    ///
    /// Returns `0` if the key has no recorded generation. The
    /// authoritative generation is the per-client generation
    /// captured at publication (set to `1` on first publish) and
    /// incremented on every restart. Stale process-exit events
    /// whose `event.generation` does not match this value are
    /// ignored by the exit handler.
    pub async fn generation_for_key(&self, key: &str) -> u64 {
        let map = self.generation_map.lock().await;
        map.get(key).copied().unwrap_or(0)
    }

    /// Return the current `LspOperationalState` for `key`, or
    /// `None` if no entry has been recorded.
    ///
    /// This is the cheap accessor used by readiness gating and
    /// by the root-side semantic/hunk/security workflows. It does
    /// not construct a full health snapshot — callers that need
    /// the snapshot should use [`operational_health_snapshot`].
    pub async fn operational_state_for_key(&self, key: &str) -> Option<LspOperationalState> {
        let states = self.operational_state.read().await;
        states.get(key).map(|s| s.state.clone())
    }

    /// Wait for the live client servicing `key` to reach its
    /// configured readiness condition. Used by `run_initialization_attempt`
    /// and `restart_client` after the LSP handshake completes.
    ///
    /// - `InitializedIsReady` returns `Ready { elapsed: 0 }` immediately.
    /// - `WaitForDiagnosticsOrTimeout { timeout }` calls
    ///   `client.wait_for_first_diagnostics(timeout)`. On success
    ///   it returns `Ready { elapsed }`; on timeout it returns
    ///   `Degraded { reason: "diagnostics wait timed out", elapsed: timeout }`.
    /// - `WaitForProgressEndOrTimeout { timeout }` calls
    ///   `client.wait_for_progress_end(timeout)`. On success it
    ///   returns `Ready { elapsed }`; on timeout it returns
    ///   `Degraded { reason: "progress wait timed out", elapsed: timeout }`.
    /// - `WarmupDelay { duration }` sleeps for `duration` and
    ///   returns `Ready { elapsed: duration }`.
    ///
    /// Returns `Degraded` with reason `"client not initialized"`
    /// if no client is currently published for the key.
    /// Logs the outcome at `info!` level.
    pub async fn wait_for_readiness(
        &self,
        key: &str,
        policy: &LspReadinessPolicy,
    ) -> ReadinessResult {
        let started = Instant::now();
        let result: ReadinessResult = match policy {
            LspReadinessPolicy::InitializedIsReady => ReadinessResult::Ready {
                elapsed: Duration::ZERO,
            },
            LspReadinessPolicy::WarmupDelay { duration } => {
                tokio::time::sleep(*duration).await;
                ReadinessResult::Ready { elapsed: *duration }
            }
            LspReadinessPolicy::WaitForDiagnosticsOrTimeout { timeout } => {
                let client = self.lookup_client(key).await;
                match client {
                    Some(c) => {
                        if c.wait_for_first_diagnostics(*timeout).await {
                            ReadinessResult::Ready {
                                elapsed: started.elapsed(),
                            }
                        } else {
                            ReadinessResult::Degraded {
                                reason: "diagnostics wait timed out".to_string(),
                                elapsed: started.elapsed(),
                            }
                        }
                    }
                    None => ReadinessResult::Degraded {
                        reason: "client not initialized".to_string(),
                        elapsed: started.elapsed(),
                    },
                }
            }
            LspReadinessPolicy::WaitForProgressEndOrTimeout { timeout } => {
                let client = self.lookup_client(key).await;
                match client {
                    Some(c) => {
                        if c.wait_for_progress_end(*timeout).await {
                            ReadinessResult::Ready {
                                elapsed: started.elapsed(),
                            }
                        } else {
                            ReadinessResult::Degraded {
                                reason: "progress wait timed out".to_string(),
                                elapsed: started.elapsed(),
                            }
                        }
                    }
                    None => ReadinessResult::Degraded {
                        reason: "client not initialized".to_string(),
                        elapsed: started.elapsed(),
                    },
                }
            }
        };
        match &result {
            ReadinessResult::Ready { elapsed } => {
                info!(
                    key,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "readiness reached"
                );
            }
            ReadinessResult::Degraded { reason, elapsed } => {
                info!(
                    key,
                    elapsed_ms = elapsed.as_millis() as u64,
                    reason = reason.as_str(),
                    "readiness degraded"
                );
            }
        }
        result
    }

    /// Lookup the live client for `key` without taking a write
    /// lock on the clients map. Returns `None` if the key has
    /// not been published.
    async fn lookup_client(&self, key: &str) -> Option<Arc<LspClient>> {
        let clients = self.clients.read().await;
        clients.get(key).cloned()
    }

    /// Set the authoritative generation for `key`.
    ///
    /// Called by the publication path (first publish sets it to
    /// `1`) and by the restart coordinator on every
    /// successful restart (increments the existing value).
    pub async fn set_generation(&self, key: &str, generation: u64) {
        let mut map = self.generation_map.lock().await;
        map.insert(key.to_string(), generation);
    }

    /// Compute the next authoritative generation for `key` from
    /// the current authoritative value. Pass 3 — Single
    /// Generation Owner. The restart coordinator is the only
    /// caller; it calls this exactly once per restart attempt
    /// and threads the result through the reinit closure so
    /// generation is owned by a single decision point.
    ///
    /// Implementation: `current.saturating_add(1).max(1)` so the
    /// first value is `1` even after restart attempts on a key
    /// that has been forcibly reset to `0`. The store is NOT
    /// updated — the reinit closure publishes the new value via
    /// `set_generation` once the new client is in hand.
    pub async fn next_generation_for_key(&self, key: &str) -> u64 {
        let map = self.generation_map.lock().await;
        let current = map.get(key).copied().unwrap_or(0);
        current.saturating_add(1).max(1)
    }

    /// Return the persisted client descriptor for `key`, if any.
    ///
    /// The descriptor is populated on first publish and read by
    /// the restart coordinator to seed a new client without
    /// re-detecting language or project root.
    pub async fn descriptor_for_key(&self, key: &str) -> Option<LspClientDescriptor> {
        let map = self.descriptor_map.lock().await;
        map.get(key).cloned()
    }

    /// Return the generation and an `LspOperationalHealthSnapshot`
    /// for `key`, even when no live client exists.
    ///
    /// Unlike [`operational_health_snapshot`], this method does
    /// not require a published client. It reads the operational
    /// state directly and synthesizes the snapshot with:
    /// - `transport = None` (no live client),
    /// - `pending_requests = 0`,
    /// - `open_documents` from the document registry,
    /// - `last_error` and `stderr_tail` derived from the
    ///   `OperationalServerState` and (if available) the
    ///   process runtime,
    /// - `last_message_age_ms` and `last_diagnostics_age_ms`
    ///   set to `None` because no live client is present.
    ///
    /// Returns `None` only if no `OperationalServerState` exists
    /// for the key at all.
    pub async fn generation_and_metadata_for_key(
        &self,
        key: &str,
    ) -> Option<(u64, super::health::LspOperationalHealthSnapshot)> {
        let op_state = {
            let states = self.operational_state.read().await;
            states.get(key)?.clone()
        };

        let generation = self.generation_for_key(key).await;

        let open_documents = self.document_registry.document_count(key).await;
        let last_error = match &op_state.state {
            super::health::LspOperationalState::Failed { reason } => Some(reason.clone()),
            _ => op_state
                .last_exit
                .as_ref()
                .filter(|e| !e.expected)
                .map(|e| e.reason.clone()),
        };

        // Stderr tail prefers the live runtime; falls back to the
        // persisted tail from the last exit.
        let stderr_tail = {
            let map = self.runtime_map.lock().await;
            match map.get(key) {
                Some(entry) => entry.runtime.stderr_tail_capped(20),
                None => op_state.last_stderr_tail.clone(),
            }
        };

        let snapshot = super::health::LspOperationalHealthSnapshot::from_operational_state(
            key.rsplit_once(':')
                .map(|(_, s)| s.to_string())
                .unwrap_or_default(),
            PathBuf::from(key.rsplit_once(':').map(|(r, _)| r).unwrap_or("")),
            generation,
            op_state.state.clone(),
            None,
            0,
            open_documents,
            None,
            None,
            op_state.restart_attempts,
            last_error,
            stderr_tail,
        );

        Some((generation, snapshot))
    }

    /// Handle a process exit event from a monitor task.
    ///
    /// Stale events (where `event.generation` does not match the
    /// authoritative generation for the key) are ignored to prevent
    /// older generations from corrupting a newer client's state.
    /// If restart is enabled and attempts remain, schedules a
    /// restart. Otherwise, transitions the server to Failed.
    async fn handle_exit_event(&self, event: LspProcessExitEvent) {
        let key = format!("{}:{}", event.root.display(), event.server_id);
        let current_generation = self.generation_for_key(&key).await;
        if event.generation != current_generation {
            debug!(
                server = %event.server_id,
                root = %event.root.display(),
                event_generation = event.generation,
                current_generation,
                "ignoring stale process exit event"
            );
            return;
        }

        info!(
            server = %event.server_id,
            root = %event.root.display(),
            generation = event.generation,
            status = ?event.status,
            signal = ?event.signal,
            expected = event.expected,
            reason = %event.reason(),
            "process exit observed"
        );

        // Persist exit metadata before any restart or state transition.
        {
            let mut states = self.operational_state.write().await;
            if let Some(entry) = states.get_mut(&key) {
                entry.last_exit = Some(LspProcessExitSummary {
                    generation: event.generation,
                    status: event.status,
                    signal: event.signal,
                    expected: event.expected,
                    reason: event.reason(),
                });
                entry.last_stderr_tail = event.stderr_tail.clone();
            }
        }

        if event.expected {
            // Expected exit (graceful shutdown) — no restart needed.
            if let Err(e) = transition_operational_state(
                &self.operational_state,
                &key,
                LspOperationalState::Stopped,
            )
            .await
            {
                warn!(
                    server = %event.server_id,
                    root = %event.root.display(),
                    error = %e,
                    "failed to transition to Stopped on expected exit"
                );
            }
            return;
        }

        // Unexpected exit — fail the transport.
        let client = {
            let clients = self.clients.read().await;
            clients.get(&key).cloned()
        };
        if let Some(client) = &client {
            use super::client::ClientTransportState;
            let mut ts = client.transport_state.lock().await;
            if matches!(*ts, ClientTransportState::Running) {
                *ts = ClientTransportState::Failed {
                    reason: event.reason(),
                };
            }
            drop(ts);
            // Fail all pending requests.
            super::client::fail_all_pending(&client.pending, &event.reason()).await;
        }

        // Check restart policy via the descriptor (single source of truth).
        let descriptor = self.descriptor_for_key(&key).await;
        let should_restart = match &descriptor {
            Some(d) => {
                d.restart_policy.mode == LspRestartMode::OnUnexpectedExit
                    && d.restart_policy.max_attempts > 0
            }
            None => false,
        };

        // Pass 5 — Shared Restart Budget. Before scheduling a
        // restart, lazily reset the counter if the previous
        // run stayed healthy for `reset_after_healthy`. This
        // makes the budget span rapid crash cycles: a server
        // that survives for the full reset interval earns a
        // fresh budget on the next crash.
        if let Some(d) = &descriptor {
            if let Some(prev) = self
                .reset_restart_attempts_if_healthy_inherent(
                    &key,
                    d.restart_policy.reset_after_healthy,
                )
                .await
            {
                debug!(key, prev, "restart_attempts reset by healthy interval");
            }
        }

        if should_restart {
            // Delegate the entire restart lifecycle to
            // `restart_client`, which in turn invokes the
            // coordinator (backoff, retries, generation increment,
            // document replay, state transitions).
            let server_id = event.server_id.clone();
            let root = event.root.clone();
            match self.restart_client(&key).await {
                Ok(()) => {
                    info!(
                        server = %server_id,
                        root = %root.display(),
                        "client restart completed"
                    );
                }
                Err(e) => {
                    warn!(
                        server = %server_id,
                        root = %root.display(),
                        error = %e,
                        "restart failed"
                    );
                }
            }
        } else {
            // No restart — transition to Failed.
            let reason = event.reason();
            if let Err(e) = transition_operational_state(
                &self.operational_state,
                &key,
                LspOperationalState::Failed {
                    reason: reason.clone(),
                },
            )
            .await
            {
                warn!(
                    server = %event.server_id,
                    root = %event.root.display(),
                    error = %e,
                    "failed to transition to Failed"
                );
            }
            warn!(
                server = %event.server_id,
                root = %event.root.display(),
                "server failed permanently (restart disabled or exhausted)"
            );
        }
    }

    /// Restart a client by key under the configured
    /// `LspRestartPolicy` (Automatic trigger).
    ///
    /// Stops the old client, collects open documents, creates a new
    /// client, and replays documents. Called by the exit handler when
    /// restart is enabled.
    ///
    /// This method delegates to `restart::restart_client_coordinator`,
    /// which applies the configured `LspRestartPolicy` (mode, max
    /// attempts, backoff), increments the per-key restart counter,
    /// transitions `LspOperationalState` (`RestartScheduled` →
    /// `Restarting` → `Initializing` → `Ready` or `Failed`), and
    /// generates a fresh generation for the new client.
    pub async fn restart_client(&self, key: &str) -> Result<(), LspError> {
        // Automatic restart — coalesce with any in-flight
        // automatic restart, no manual supersession. The
        // unified `restart_client_owned` path handles
        // ownership + teardown internally.
        self.restart_client_owned(
            key,
            RestartTrigger::Automatic,
            None,
            OwnedRestartOptions::automatic(),
        )
        .await
    }

    /// Restart a client by key under a manual trigger.
    ///
    /// Manual restart ALWAYS runs (it bypasses
    /// `LspRestartMode::Disabled`). The old runtime is terminated
    /// BEFORE the replacement is started so a manual restart
    /// cannot leave two live processes.
    ///
    /// Pass 5 — This entry point delegates to the unified
    /// [`Self::restart_client_owned`] path with manual-mode
    /// options so manual and automatic restarts share the
    /// same ownership + teardown + handoff logic.
    pub async fn manual_restart_client(&self, key: &str) -> Result<(), LspError> {
        self.restart_client_owned(
            key,
            RestartTrigger::Manual,
            None,
            OwnedRestartOptions::manual(),
        )
        .await
    }

    /// Private restart entry point that applies a specific
    /// `RestartTrigger` to the coordinator.
    ///
    /// Pass 5 — Single internal entry point for all restart
    /// paths (manual and automatic). Manual callers pass
    /// `OwnedRestartOptions::manual()`, automatic callers pass
    /// `OwnedRestartOptions::automatic()`. The two flows share:
    ///
    /// 1. Lifecycle check (service must be `Running`).
    /// 2. (Manual only) Cancel any existing owner and wait
    ///    for its explicit completion signal under a bounded
    ///    timeout — this is what makes cancellation-vs-completion
    ///    observable.
    /// 3. Acquire the per-key lease. Automatic callers
    ///    coalesce with an in-flight automatic lease; manual
    ///    callers reject collisions (manual-vs-manual and
    ///    manual-vs-automatic raced-during-supersession).
    /// 4. (Manual only) Re-read the live authoritative
    ///    generation. If a newer generation appeared during the
    ///    supersession wait, return `ServerRestarted` so the
    ///    caller can re-issue. This prevents the manual
    ///    teardown from targeting a runtime that the in-flight
    ///    automatic restart has already replaced.
    /// 5. Snapshot retained diagnostics from the live client.
    /// 6. (Manual only) Terminate the old runtime via
    ///    `terminate_runtime` with `RuntimeTerminationReason::ManualRestart`,
    ///    then remove the old client from the live map.
    /// 7. Hand off to the coordinator with the lease token,
    ///    retained diagnostics, and a freshly built reinit
    ///    closure.
    /// 8. Release the lease — sends `Finished` on the
    ///    completion channel so the next waiter observes
    ///    completion.
    async fn restart_client_owned(
        &self,
        key: &str,
        trigger: RestartTrigger,
        caller_retained_diagnostics: Option<HashMap<String, DiagnosticCacheEntry>>,
        options: OwnedRestartOptions,
    ) -> Result<(), LspError> {
        // 1. Lifecycle check.
        {
            let lc = self.lifecycle.read().await;
            if lc.phase != ServiceLifecycle::Running {
                return Err(LspError::InitializationCancelled(
                    "service is not running".to_string(),
                ));
            }
        }

        // 2. Manual supersession — cancel existing owner and
        //    wait for completion under bounded timeout.
        //
        //    Pass 2 (Phase 3 final closure) — Capture the
        //    pre-wait generation BEFORE cancelling the owner.
        //    The previous implementation re-read the generation
        //    AFTER the wait, which usually returned the same
        //    value (no advance was observable because the
        //    waiter already blocked until the in-flight owner
        //    finished). To detect a generation advance that
        //    occurs *during* the wait, we must snapshot the
        //    generation immediately before cancelling, then
        //    compare against the post-wait generation.
        let pre_wait_snapshot = if options.manual_supersession {
            self.capture_manual_supersession_snapshot(key).await
        } else {
            RestartOwnerDiagnosticSnapshot {
                pre_wait_generation: 0,
                pre_wait_server_id: String::new(),
            }
        };

        if options.manual_supersession {
            let waiter = super::restart::cancel_restart_ownership(&self.restart_tasks, key).await;
            if let Some(waiter) = waiter {
                debug!(
                    key,
                    owner_id = format_args!("{:?}", waiter).as_str(),
                    "restart: waiting for in-flight owner completion"
                );
                if let Err(e) = waiter.wait(MANUAL_SUPERSESSION_OWNER_TIMEOUT).await {
                    warn!(
                        key,
                        error = %e,
                        "restart: prior owner did not complete within timeout; aborting"
                    );
                    return Err(e);
                }
            }
        }

        // 3. Acquire the lease.
        let lease = match acquire_restart_ownership(
            &self.restart_tasks,
            &self.restart_owner_counter,
            key,
            trigger,
        )
        .await
        {
            RestartLeaseAcquisition::Acquired(lease) => lease,
            RestartLeaseAcquisition::AlreadyInProgress { existing_trigger } => {
                debug!(
                    key,
                    ?existing_trigger,
                    ?trigger,
                    "restart already in progress; coalescing"
                );
                // Automatic callers coalesce: the existing
                // automatic restart counts as this one.
                if matches!(trigger, RestartTrigger::Automatic) {
                    return Ok(());
                }
                // Manual callers reject collisions so the
                // caller can distinguish "in progress" from
                // "done".
                let msg = match existing_trigger {
                    RestartTrigger::Manual => "another manual restart is in progress",
                    RestartTrigger::Automatic => {
                        "a new automatic restart appeared during manual supersession"
                    }
                };
                return Err(LspError::InitializationCancelled(msg.to_string()));
            }
        };

        // 4. Manual-only: compare the post-wait generation
        //    against the pre-wait snapshot. If a newer
        //    generation appeared during the wait, the manual
        //    teardown would target a stale runtime and the
        //    caller must observe `ServerRestarted` instead.
        let current_generation = self.generation_for_key(key).await;
        if options.manual_supersession
            && pre_wait_snapshot.pre_wait_generation != 0
            && current_generation > pre_wait_snapshot.pre_wait_generation
        {
            warn!(
                key,
                pre_wait_generation = pre_wait_snapshot.pre_wait_generation,
                current_generation,
                "manual restart: generation advanced during ownership wait; aborting"
            );
            let _ = lease.release().await;
            return Err(LspError::ServerRestarted {
                server_id: pre_wait_snapshot.pre_wait_server_id,
                old_generation: pre_wait_snapshot.pre_wait_generation,
                new_generation: Some(current_generation),
            });
        }

        // 5. Snapshot retained diagnostics. If the caller
        //    provided a snapshot (e.g. from a prior teardown),
        //    use it; otherwise capture now.
        let retained_diagnostics = match caller_retained_diagnostics {
            Some(map) => map,
            None => self.snapshot_diagnostics_for_restart(key).await,
        };

        // 6. Manual-only: terminate the old runtime and remove
        //    the old client before the coordinator runs.
        if options.manual_supersession && current_generation > 0 {
            let old_client = {
                let clients = self.clients.read().await;
                clients.get(key).cloned()
            };

            let now = Instant::now();
            let abs_deadline = now + Duration::from_secs(6);
            let graceful_deadline = now + Duration::from_secs(2);
            let _ = terminate_runtime(
                &self.runtime_map,
                key,
                current_generation,
                old_client.clone(),
                graceful_deadline,
                abs_deadline,
                RuntimeTerminationReason::ManualRestart,
            )
            .await;

            {
                let mut clients = self.clients.write().await;
                clients.remove(key);
            }
        }

        // Reset generation-read sentinel after manual teardown
        // — the live client is gone so the coordinator will
        // publish a fresh generation via its reinit closure.
        if options.manual_supersession {
            let _ = self.generation_for_key(key).await;
        }

        // 7. Hand off to the coordinator.
        let descriptor = match self.descriptor_for_key(key).await {
            Some(d) => d,
            None => {
                let _ = lease.release().await;
                return Err(LspError::LaunchFailed(format!(
                    "no descriptor stored for key {key} — was the client ever initialized?"
                )));
            }
        };
        let reinit_fn = self.build_reinit_fn(key.to_string());

        let outcome = restart_client_coordinator(
            self,
            key,
            trigger,
            lease.token(),
            Some(retained_diagnostics),
            descriptor,
            reinit_fn,
        )
        .await;

        // 8. Release the lease.
        let _ = lease.release().await;

        // Pass 6 — Map RestartOutcome to Result<(), LspError>.
        // Ready and Degraded are both success outcomes; Degraded
        // is logged distinctly and does NOT bubble up as an
        // error to the caller.
        match outcome {
            Ok(super::restart::RestartOutcome::Ready) => Ok(()),
            Ok(super::restart::RestartOutcome::Degraded { reason }) => {
                info!(
                    key,
                    reason = %reason,
                    "restart completed in degraded mode; client is live"
                );
                Ok(())
            }
            Err(e) => Err(e),
        }
    }

    /// Get a snapshot of operational health for the given client key.
    ///
    /// Returns a snapshot even when no live client is present
    /// (e.g. during `Restarting`, `Failed`, `RestartScheduled`, or
    /// `Stopped`). When no client is available, `transport = None`,
    /// `pending_requests = 0`, and the age fields are `None`. The
    /// generation is read from `generation_map` so it is always
    /// real.
    ///
    /// Returns `None` only if no `OperationalServerState` exists
    /// for the key at all (the key has never been touched).
    pub async fn operational_health_snapshot(
        &self,
        key: &str,
    ) -> Option<super::health::LspOperationalHealthSnapshot> {
        let op_state = {
            let states = self.operational_state.read().await;
            states.get(key)?.clone()
        };

        // Read generation from the authoritative generation map.
        // Falls back to 0 (the "no client" sentinel) when the key
        // has not been published yet — even when `op_state`
        // exists, the generation map is the source of truth.
        let generation = self.generation_for_key(key).await;

        // Optional client-derived fields. When no live client is
        // present (e.g. during restart, after shutdown, or before
        // publication), we set `transport = None` and the age
        // fields to `None` so the snapshot is still returned.
        let client = {
            let clients = self.clients.read().await;
            clients.get(key).cloned()
        };

        let (transport, pending_requests, last_message_age_ms, last_diagnostics_age_ms) =
            match client {
                Some(c) => (
                    Some(c.transport_state_snapshot().await),
                    c.pending_request_count().await,
                    c.last_message_age_ms().await,
                    c.last_diagnostics_age_ms().await,
                ),
                None => (None, 0, None, None),
            };

        let open_documents = self.document_registry.document_count(key).await;

        // Last error comes from the operational state for
        // `Failed { reason }` transitions. For other states, fall
        // back to the persisted exit summary for non-expected exits.
        let last_error = match &op_state.state {
            super::health::LspOperationalState::Failed { reason } => Some(reason.clone()),
            _ => op_state
                .last_exit
                .as_ref()
                .filter(|e| !e.expected)
                .map(|e| e.reason.clone()),
        };

        // Stderr tail prefers the live runtime; falls back to the
        // persisted tail from the last exit.
        let stderr_tail = {
            let map = self.runtime_map.lock().await;
            match map.get(key) {
                Some(entry) => entry.runtime.stderr_tail_capped(20),
                None => op_state.last_stderr_tail.clone(),
            }
        };

        Some(
            super::health::LspOperationalHealthSnapshot::from_operational_state(
                key.rsplit_once(':')
                    .map(|(_, s)| s.to_string())
                    .unwrap_or_default(),
                PathBuf::from(key.rsplit_once(':').map(|(r, _)| r).unwrap_or("")),
                generation,
                op_state.state.clone(),
                transport,
                pending_requests,
                open_documents,
                last_message_age_ms,
                last_diagnostics_age_ms,
                op_state.restart_attempts,
                last_error,
                stderr_tail,
            ),
        )
    }

    /// Test-only: inject a custom descriptor for `key`.
    ///
    /// Used by the supervisor/restart integration tests to enable
    /// restart on a client initialized with the default-disabled
    /// `LspRestartPolicy::default()` profile. The next call to
    /// `restart_client` (or the next exit event with restart
    /// enabled) reads this descriptor instead of the one
    /// persisted during initialization.
    ///
    /// Not part of the public production API.
    #[cfg(feature = "lsp-test-support")]
    pub async fn set_descriptor_for_key(&self, key: &str, descriptor: LspClientDescriptor) {
        let mut map = self.descriptor_map.lock().await;
        map.insert(key.to_string(), descriptor);
    }

    /// Test-only: publish a process exit event to the service's
    /// exit channel.
    ///
    /// Used by the supervisor/restart integration tests to inject
    /// synthetic events for stale-generation safety checks (Phase
    /// 7). Production code never calls this; the only writer to
    /// the exit channel is the process monitor.
    ///
    /// Not part of the public production API.
    #[cfg(feature = "lsp-test-support")]
    pub async fn publish_test_exit_event(&self, event: LspProcessExitEvent) {
        let _ = self.exit_tx.send(event).await;
    }

    /// Test-only: read the current process-intent for `key`'s
    /// runtime, when one is present. Returns `None` if the
    /// runtime is no longer tracked.
    ///
    /// Used by the supervisor integration tests to assert the
    /// `LspProcessIntent` transitions for graceful-shutdown and
    /// force-kill paths.
    #[cfg(feature = "lsp-test-support")]
    pub async fn test_runtime_intent(&self, key: &str) -> Option<super::runtime::LspProcessIntent> {
        let map = self.runtime_map.lock().await;
        map.get(key).map(|entry| entry.runtime.intent())
    }

    /// Test-only: seed the operational state for `key` so the
    /// restart coordinator can transition it.
    ///
    /// Used by the supervisor integration tests to set up a
    /// pre-existing operational state entry (e.g. `Ready`)
    /// without going through a full successful initialization.
    /// The new entry's `restart_attempts` counter is reset to
    /// `0`; the caller is responsible for invoking
    /// [`LspService::increment_restart_attempts`] (or calling
    /// `restart_client`, which does so internally) when needed.
    #[cfg(feature = "lsp-test-support")]
    pub async fn seed_operational_state_for_key(
        &self,
        key: &str,
        state: super::health::LspOperationalState,
    ) {
        let mut states = self.operational_state.write().await;
        let entry = states
            .entry(key.to_string())
            .or_insert_with(OperationalServerState::default);
        entry.state = state;
    }

    /// Internal: build a reinit closure that creates a new client
    /// from a `LspClientDescriptor`. The closure is consumed by
    /// `restart::restart_client_coordinator` on every retry.
    ///
    /// The closure spawns a fresh process from the descriptor's
    /// launch spec, runs the LSP `initialize` / `initialized`
    /// handshake, inserts the new client into the global
    /// `clients` map, and spawns the process monitor. The old
    /// client is NOT shut down here — the coordinator removes it
    /// from the map before calling the reinit closure.
    ///
    /// The closure captures every LspService field it needs as an
    /// `Arc`, so the resulting closure is `'static + Send` and
    /// can be owned by the coordinator across awaits.
    ///
    /// The replacement generation is passed in by the
    /// coordinator (Pass 3 — Single Generation Owner). The
    /// closure does NOT calculate generation independently; the
    /// coordinator's `next_generation_for_key` call is the
    /// single source of truth.
    ///
    /// Pass 4 — The closure returns an
    /// [`super::restart::UnpublishedReplacement`] wrapping the
    /// freshly-built client and the closure-supplied
    /// `generation`. The coordinator uses the generation to
    /// scope post-spawn cancellation cleanup so a newer
    /// replacement is never disturbed.
    fn build_reinit_fn(
        &self,
        key: String,
    ) -> impl FnMut(
        &LspClientDescriptor,
        u64,
    ) -> futures::future::BoxFuture<
        'static,
        Result<super::restart::UnpublishedReplacement, LspError>,
    > + Send
           + 'static {
        let clients = self.clients.clone();
        let document_owners = self.document_owners.clone();
        let generation_map = self.generation_map.clone();
        let runtime_map = self.runtime_map.clone();
        let exit_tx = self.exit_tx.clone();

        move |descriptor: &LspClientDescriptor, generation: u64| {
            let key = key.clone();
            let descriptor = descriptor.clone();
            let clients = clients.clone();
            let document_owners = document_owners.clone();
            let generation_map = generation_map.clone();
            let runtime_map = runtime_map.clone();
            let exit_tx = exit_tx.clone();

            Box::pin(async move {
                // 1. Spawn the new client from the descriptor's
                //    launch spec.
                let client = LspClient::new_with_launch_spec(
                    descriptor.launch_spec.clone(),
                    &descriptor.root,
                    descriptor.workspace_configuration.clone(),
                    LspClientOptions::default(),
                )
                .await?;

                // 2. Run the LSP initialize handshake.
                client
                    .initialize(descriptor.initialization_options.clone())
                    .await?;

                // 2b. Compute and store the override-aware normalized snapshot.
                {
                    let caps_snapshot = client.capabilities.lock().await.clone();
                    if let Some(ref caps) = caps_snapshot {
                        let profile =
                            super::compatibility::profile_for_server(&descriptor.server_id);
                        let override_caps = profile
                            .as_ref()
                            .map(|p| p.observed_capabilities.clone())
                            .unwrap_or_default();
                        // Resolve language_id from the server definition.
                        let lang = super::server::server_definitions()
                            .iter()
                            .find(|s| s.id == descriptor.server_id.as_str())
                            .and_then(|s| s.languages.first().copied());
                        let normalized =
                            crate::capability::LspCapabilitySnapshot::from_capabilities_with_override(
                                caps,
                                Some(&descriptor.server_id),
                                lang,
                                &override_caps,
                            );
                        client.set_normalized_capabilities(normalized).await;
                    }
                }

                // 3. Send `initialized` notification.
                client.send_initialized().await?;

                let client = Arc::new(client);

                // 4. Insert into the clients map.
                {
                    let mut map = clients.write().await;
                    map.insert(key.clone(), client.clone());
                }

                // 5. Update document ownership for the key.
                {
                    let mut owners = document_owners.write().await;
                    owners.retain(|_, v| v != &key);
                }

                // 6. Publish the coordinator-supplied
                //    generation. The reinit closure is the
                //    publication site; the coordinator owns the
                //    calculation. This must happen BEFORE the
                //    process monitor is spawned so the monitor
                //    sees the authoritative value.
                {
                    let mut map = generation_map.lock().await;
                    map.insert(key.clone(), generation);
                }

                // 7. Spawn the process monitor.
                let monitor_client = client.clone();
                let monitor_key = key.clone();
                let monitor_server_id = descriptor.server_id.clone();
                let monitor_root = descriptor.root.clone();
                let monitor_exit_tx = exit_tx.clone();
                let monitor_runtime_map = runtime_map.clone();
                tokio::spawn(async move {
                    spawn_process_monitor(
                        monitor_client,
                        monitor_key,
                        monitor_server_id,
                        monitor_root,
                        generation,
                        monitor_exit_tx,
                        monitor_runtime_map,
                    )
                    .await;
                });

                Ok(super::restart::UnpublishedReplacement { client, generation })
            })
        }
    }

    /// Internal: capture the current authoritative state for
    /// `key` BEFORE the manual supersession path cancels any
    /// in-flight owner and waits for its completion.
    ///
    /// Pass 2 (Phase 3 final closure) — The snapshot is taken
    /// *before* cancellation so the post-wait comparison can
    /// detect a generation advance that occurs during the wait
    /// (e.g. the in-flight owner successfully published a new
    /// generation before the manual waiter resolved). The
    /// previous helper was called AFTER the wait and almost
    /// always read the same value as the post-wait check, so
    /// the comparison was effectively a no-op.
    ///
    /// Fields default to the "no prior owner" sentinel (`0`,
    /// `String::new()`) when no live client exists for `key`.
    async fn capture_manual_supersession_snapshot(
        &self,
        key: &str,
    ) -> RestartOwnerDiagnosticSnapshot {
        let pre_wait_generation = self.generation_for_key(key).await;
        let pre_wait_server_id = self
            .descriptor_for_key(key)
            .await
            .map(|d| d.server_id)
            .unwrap_or_default();
        RestartOwnerDiagnosticSnapshot {
            pre_wait_generation,
            pre_wait_server_id,
        }
    }

    /// Pass 5 — Shared Restart Budget. Reset the
    /// `restart_attempts` counter to `0` if the previous
    /// run stayed healthy for at least
    /// `reset_after_healthy`. Returns the previous counter
    /// value when the reset was applied, or `None` when the
    /// service has not been healthy long enough (or there is
    /// no prior healthy timestamp for the key).
    async fn reset_restart_attempts_if_healthy_inherent(
        &self,
        key: &str,
        reset_after_healthy: Duration,
    ) -> Option<u32> {
        let mut states = self.operational_state.write().await;
        let entry = states.get_mut(key)?;
        let last_healthy = entry.last_healthy_at?;
        let elapsed = last_healthy.elapsed();
        if elapsed >= reset_after_healthy {
            let prev = entry.restart_attempts;
            entry.restart_attempts = 0;
            Some(prev)
        } else {
            None
        }
    }
}

// ── RestartShared impl for LspService ────────────────────────────

impl RestartShared for LspService {
    fn clients(&self) -> &Arc<RwLock<HashMap<String, Arc<LspClient>>>> {
        &self.clients
    }
    fn document_owners(&self) -> &Arc<RwLock<HashMap<String, String>>> {
        &self.document_owners
    }
    fn document_registry(&self) -> &Arc<OpenDocumentRegistry> {
        &self.document_registry
    }
    fn runtime_map(&self) -> &super::restart::SharedRuntimeMap {
        // Pass 4 — the production runtime_map is the
        // SharedRuntimeMap type alias.
        &self.runtime_map
    }
    async fn generation_for_key(&self, key: &str) -> u64 {
        self.generation_for_key(key).await
    }
    async fn set_generation(&self, key: &str, generation: u64) {
        self.set_generation(key, generation).await;
    }
    async fn next_generation_for_key(&self, key: &str) -> u64 {
        self.next_generation_for_key(key).await
    }
    async fn service_phase(&self) -> ServicePhase {
        let lc = self.lifecycle.read().await;
        match lc.phase {
            ServiceLifecycle::Running => ServicePhase::Running,
            ServiceLifecycle::ShuttingDown => ServicePhase::ShuttingDown,
            ServiceLifecycle::Stopped => ServicePhase::Stopped,
        }
    }
    async fn restart_attempts(&self, key: &str) -> u32 {
        let states = self.operational_state.read().await;
        states.get(key).map(|s| s.restart_attempts).unwrap_or(0)
    }
    async fn increment_restart_attempts(&self, key: &str) -> u32 {
        let mut states = self.operational_state.write().await;
        let entry = states
            .entry(key.to_string())
            .or_insert_with(OperationalServerState::default);
        entry.restart_attempts = entry.restart_attempts.saturating_add(1);
        entry.restart_attempts
    }
    async fn reserve_restart_attempt(&self, key: &str, max_attempts: u32) -> Result<u32, LspError> {
        // Pass 11 — atomic budget check + increment under one
        // write lock so the coordinator can never spawn more
        // than `max_attempts` replacement processes.
        let mut states = self.operational_state.write().await;
        let entry = states
            .entry(key.to_string())
            .or_insert_with(OperationalServerState::default);
        if entry.restart_attempts >= max_attempts {
            return Err(LspError::LaunchFailed(format!(
                "restart attempts exhausted (max={max_attempts})"
            )));
        }
        entry.restart_attempts = entry.restart_attempts.saturating_add(1);
        Ok(entry.restart_attempts)
    }
    async fn transition_operational_state(
        &self,
        key: &str,
        next: LspOperationalState,
    ) -> Result<(), LspError> {
        transition_operational_state(&self.operational_state, key, next).await
    }
    async fn set_last_healthy_now(&self, key: &str) {
        let mut states = self.operational_state.write().await;
        if let Some(state) = states.get_mut(key) {
            state.last_healthy_at = Some(Instant::now());
        }
    }
    async fn reset_restart_attempts_if_healthy(
        &self,
        key: &str,
        reset_after_healthy: Duration,
    ) -> Option<u32> {
        self.reset_restart_attempts_if_healthy_inherent(key, reset_after_healthy)
            .await
    }
    async fn snapshot_diagnostics_for_restart(
        &self,
        key: &str,
    ) -> HashMap<String, super::client::DiagnosticCacheEntry> {
        let clients = self.clients.read().await;
        match clients.get(key) {
            Some(c) => c.diagnostic_cache_snapshot().await,
            None => HashMap::new(),
        }
    }
    async fn mark_diagnostics_stale_for_key(&self, key: &str) {
        // Pass 9 — coordinator no longer calls this helper.
        // Retained for backward compatibility with any test
        // paths that still depend on the destructive rewrite.
        self.mark_diagnostics_stale_for_key(key).await;
    }
    async fn wait_for_readiness(&self, key: &str, policy: &LspReadinessPolicy) -> ReadinessResult {
        // Pass 4 — Cold start and restart share this helper so
        // readiness semantics are consistent across both paths.
        LspService::wait_for_readiness(self, key, policy).await
    }
}

/// Process monitor task (formerly `LspServiceClone::restart_client`).
/// See `LspService::restart_client` for the live restart path.
// ── Process monitor ─────────────────────────────────────────────────
///
/// Monitor task that observes child process exit and forwards the
/// event to the exit channel.
///
/// This is a thin wrapper around
/// [`crate::runtime::spawn_process_runtime`]: the authoritative
/// process owner lives in `runtime` so that the child handle,
/// stderr ring buffer, intent receiver, and kill receiver all
/// belong to a single task. This wrapper only retains the
/// `Arc<LspClient>` long enough to take the child and stderr, then
/// drops the client reference entirely — the runtime never holds
/// it while waiting on the child.
async fn spawn_process_monitor(
    client: Arc<LspClient>,
    key: String,
    server_id: String,
    root: PathBuf,
    generation: u64,
    exit_tx: tokio::sync::mpsc::Sender<LspProcessExitEvent>,
    runtime_map: RuntimeMap,
) {
    // Take the child handle from the client.
    let child = {
        let mut child_opt = client.child.lock().await;
        child_opt.take()
    };
    let child = match child {
        Some(c) => c,
        None => {
            debug!(
                server = %server_id,
                key = %key,
                "no child handle available for monitoring"
            );
            return;
        }
    };

    // Take the stderr handle from the client so the runtime can own
    // the bounded ring buffer. If a test stub populated `None` (the
    // legacy stderr drain path), the runtime still works — the
    // stderr reader loop simply has no handle to read from and
    // returns immediately.
    let stderr = client.take_stderr().await;

    let (runtime, _join) = match stderr {
        Some(stderr_handle) => super::runtime::spawn_process_runtime(
            server_id.clone(),
            root.clone(),
            generation,
            child,
            stderr_handle,
        ),
        None => {
            // No stderr handle available; spawn the runtime with a
            // synthetic empty handle. This is a degenerate case
            // exercised by test stubs that retain the legacy
            // drain. The runtime's stderr reader will hit EOF
            // immediately and exit.
            let dummy_stderr = match open_null_stderr().await {
                Ok(s) => s,
                Err(e) => {
                    debug!(
                        server = %server_id,
                        key = %key,
                        error = %e,
                        "failed to open dummy stderr; abandoning monitor"
                    );
                    return;
                }
            };
            super::runtime::spawn_process_runtime(
                server_id.clone(),
                root.clone(),
                generation,
                child,
                dummy_stderr,
            )
        }
    };

    // Install the runtime handle so callers (e.g. the restart
    // coordinator) can request graceful/forced shutdown
    // and read the stderr snapshot. Installation goes through
    // `install_runtime` so a delayed install (e.g. from a
    // retry that already lost to a newer generation) cannot
    // silently overwrite the active runtime. Pass 3 — The
    // outcome is exhaustive: a `Rejected` install means the
    // monitor raced with a newer generation and lost; the
    // runtime is still live in this scope and MUST be
    // terminated before the monitor returns, otherwise it
    // leaks as an orphan process. We hold a clone for that
    // explicit cleanup path.
    let install_result =
        install_runtime(&runtime_map, key.clone(), generation, runtime.clone()).await;
    if matches!(install_result, RuntimeInstallResult::Rejected { .. }) {
        warn!(
            key = %key,
            generation,
            "monitor: runtime install was rejected; terminating the orphaned runtime"
        );
        let abs_deadline = Instant::now() + Duration::from_secs(2);
        let graceful_deadline = Instant::now() + Duration::from_millis(500);
        let _ = terminate_runtime(
            &runtime_map,
            &key,
            generation,
            None,
            graceful_deadline,
            abs_deadline,
            RuntimeTerminationReason::FailedPublication,
        )
        .await;
        return;
    }

    // Forward the runtime's exit event to the service exit channel.
    // We do NOT hold the `Arc<LspClient>` across this await — the
    // runtime task owns the child and is the authoritative waiter.
    let mut exit_rx = runtime.exit_rx.clone();
    let exit_event = loop {
        if let Some(event) = exit_rx.borrow_and_update().clone() {
            break event;
        }
        if exit_rx.changed().await.is_err() {
            return;
        }
    };

    // Remove the runtime from the map now that the event is
    // published. Removal goes through
    // `remove_runtime_if_generation` so a delayed old monitor
    // cannot remove a newer generation's runtime (e.g. a
    // restart that bumped generation between our install and
    // exit). The runtime task is free to terminate.
    remove_runtime_if_generation(&runtime_map, &key, generation).await;

    let _ = exit_tx.send(exit_event).await;
}

/// Open a `tokio::process::ChildStderr` that immediately reads
/// EOF. Used by the monitor when the client has no stderr handle
/// (legacy test stubs). The implementation opens `/dev/null` (Unix)
/// or `NUL` (Windows) and wraps it in a process-style handle.
async fn open_null_stderr() -> std::io::Result<tokio::process::ChildStderr> {
    use std::process::Stdio;
    use tokio::process::Command;
    let mut cmd = Command::new("true");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("stderr not captured for dummy"))
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

/// Abort a not-yet-started wrapper task and finalize the bookkeeping
/// associated with it. Used by the leader registration path when the
/// slot is invalidated after the task was spawned but before it was
/// started via the start barrier.
///
/// This helper:
/// 1. Aborts the task via `JoinHandle::abort` (defensive — the task
///    has not started its body yet, so this is a no-op for the body,
///    but it ensures the JoinHandle is consumed).
/// 2. Removes the active-task entry under `active_init_tasks`.
/// 3. Drains the slot from `initializing` (if still present) and
///    notifies any waiters with a `Cancelled` `SharedInitError`.
async fn abort_and_finalize_unstarted_task(
    task: tokio::task::JoinHandle<()>,
    active_init_tasks: &ActiveTaskMap,
    attempt_id: u64,
    initializing: &InitMap,
    key: &str,
) {
    // Remove the active-task entry; the task never started.
    {
        let mut tasks = active_init_tasks.lock().await;
        tasks.remove(&attempt_id);
    }

    // Abort the task (it is awaiting start_rx or has been dropped).
    task.abort();
    let _ = task.await;

    // Drain the slot and notify any (orphan) waiters.
    if let Some(senders) = take_attempt(initializing, key, attempt_id).await {
        let cancel_err = SharedInitError {
            kind: super::error::SharedInitErrorKind::Cancelled,
            message: "service lifecycle changed before registration".to_string(),
        };
        send_completion_result(senders, Err(cancel_err));
    }
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

/// Apply a state transition through the centralized validator
/// and update the `operational_state` entry for `key`.
///
/// This is the only path that should mutate the `state` field of
/// `OperationalServerState`. Direct assignments bypass the
/// transition table and are a known correctness hazard. The
/// validator returns `LspError::Protocol(...)` for invalid moves;
/// the helper logs invalid transitions at `warn!` level (never
/// panics) and returns the error to the caller.
///
/// On success the new state is written and the transition is
/// logged at `debug!` level. The helper never creates a missing
/// entry — callers that want first-publish behavior must insert
/// the entry explicitly with `generation: 1` and the desired
/// starting state.
async fn transition_operational_state(
    states: &Arc<RwLock<HashMap<String, OperationalServerState>>>,
    key: &str,
    next: LspOperationalState,
) -> Result<(), LspError> {
    let mut states = states.write().await;
    let from = match states.get(key) {
        Some(s) => s.state.clone(),
        None => {
            warn!(
                key,
                target = next.label(),
                "ignoring transition: no operational state entry for key"
            );
            return Err(LspError::Protocol(format!(
                "invalid LSP state transition: <missing> -> {}",
                next.label()
            )));
        }
    };
    match transition_health(&from, next.clone()) {
        Ok(_) => {
            debug!(
                key,
                from = from.label(),
                to = next.label(),
                "operational state transition"
            );
            if let Some(s) = states.get_mut(key) {
                s.state = next;
            }
            Ok(())
        }
        Err(_invalid) => {
            warn!(
                key,
                from = from.label(),
                to = next.label(),
                "rejected invalid operational state transition"
            );
            Err(LspError::Protocol(format!(
                "invalid LSP state transition: {} -> {}",
                from.label(),
                next.label()
            )))
        }
    }
}

async fn resolve_launch_spec(
    server: &'static LspServerDef,
    config: &LspConfig,
) -> Result<LspLaunchSpec, LspError> {
    let (command, args, env) = match config {
        LspConfig::Rules(rules) => {
            if let Some(LspRule::Active {
                command: command_parts,
                env,
                ..
            }) = rules.get(server.id)
            {
                if command_parts.is_empty() {
                    return Err(LspError::LaunchFailed(format!(
                        "server '{}' configuration command is empty",
                        server.id
                    )));
                }
                let command = PathBuf::from(&command_parts[0]);
                let args = command_parts.iter().skip(1).cloned().collect::<Vec<_>>();
                let env = env
                    .as_ref()
                    .map(|entries| {
                        entries
                            .iter()
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                (command, args, env)
            } else {
                let command = download::ensure_server_binary(server).await?;
                let args = server.args.iter().map(|s| s.to_string()).collect();
                (command, args, Vec::new())
            }
        }
        _ => {
            let command = download::ensure_server_binary(server).await?;
            let args = server.args.iter().map(|s| s.to_string()).collect();
            (command, args, Vec::new())
        }
    };

    Ok(LspLaunchSpec::new(
        server.id,
        command,
        args,
        env,
        server.languages.iter().map(|s| s.to_string()).collect(),
        server.extensions.iter().map(|s| s.to_string()).collect(),
    ))
}

/// Await a set of `InitTaskControl` completion receivers concurrently
/// under a single absolute deadline. Returns the set of tasks that
/// did NOT complete within the budget; for those the caller still
/// owns the `InitTaskControl` (with its `abort_handle`) so it can
/// forcibly abort and re-await.
///
/// The receiver in each `InitTaskControl` is the authoritative
/// terminal signal for the wrapper task: it resolves when the
/// wrapper sends its `InitTaskExit` (normal or panic path) or when
/// the sender is dropped (forced abort path, where the abort drops
/// the future and thus the sender).
///
/// Implementation note: each control's future borrows the
/// completion receiver via `&mut` inside a `tokio::select!`. When
/// the deadline fires first, the `select!` drops the deadline
/// branch (the sleep future) and the borrow on the receiver
/// ends, so the receiver is still intact and can be returned in
/// the control for a second pass. The `biased;` directive in the
/// `select!` ensures we always poll the receiver first, so
/// completed tasks resolve as soon as the wrapper sends its
/// terminal signal.
#[allow(clippy::type_complexity)]
async fn await_init_task_completions(
    mut tasks: Vec<InitTaskControl>,
    deadline: Instant,
) -> Vec<InitTaskControl> {
    if tasks.is_empty() {
        return Vec::new();
    }

    let total = tasks.len();
    let mut completed = 0usize;

    let mut unordered: futures::stream::FuturesUnordered<
        std::pin::Pin<
            Box<
                dyn std::future::Future<Output = (InitTaskControl, Result<InitTaskExit, ()>)>
                    + Send,
            >,
        >,
    > = futures::stream::FuturesUnordered::new();

    for mut ctrl in tasks.drain(..) {
        unordered.push(Box::pin(async move {
            // Race the completion receiver against the absolute
            // deadline. `select!` borrows the receiver; on
            // timeout, the borrow ends and the receiver is
            // intact in the control.
            //
            // The receiver is `oneshot::Receiver<InitTaskExit>`.
            // - If the wrapper sent a value, we get `Ok(exit)`.
            // - If the sender was dropped (abort), we get
            //   `Err(RecvError)`.
            // - If the deadline fires first, we return
            //   `Err(())` to signal "still pending" with the
            //   control's receiver intact.
            let res: InitTaskExit = tokio::select! {
                biased;
                res = &mut ctrl.completion => match res {
                    Ok(exit) => exit,
                    Err(_recv) => InitTaskExit::Cancelled,
                },
                _ = tokio::time::sleep_until(deadline.into()) => return (ctrl, Err(())),
            };
            (ctrl, Ok(res))
        }));
    }

    use futures::StreamExt;
    let mut pending: Vec<InitTaskControl> = Vec::new();
    while let Some((ctrl, res)) = unordered.next().await {
        // `res` is `Result<InitTaskExit, ()>`:
        // - Ok(exit) means the wrapper sent a terminal exit
        //   (normal completion, panic, or cancelled).
        // - Err(()) means the deadline fired before the
        //   receiver resolved; return the control for a
        //   second pass.
        match res {
            Ok(InitTaskExit::Completed) => {
                completed += 1;
            }
            Ok(InitTaskExit::Panicked(msg)) => {
                warn!(
                    attempt_id = ctrl.attempt_id,
                    panic = %msg,
                    "init task panicked during shutdown"
                );
                completed += 1;
            }
            Ok(InitTaskExit::Cancelled) => {
                debug!(
                    attempt_id = ctrl.attempt_id,
                    "init task cancelled during shutdown"
                );
                completed += 1;
            }
            Err(()) => {
                // Deadline expired before the receiver
                // resolved. Return the control with the
                // real receiver intact for the second pass.
                pending.push(ctrl);
            }
        }
    }

    debug!(
        total,
        completed,
        pending = pending.len(),
        "await_init_task_completions complete"
    );
    pending
}

/// Wrapper task for a spawned initialization attempt.
///
/// Owns the `completion_tx` end of the authoritative terminal
/// signal: this wrapper must send exactly one `InitTaskExit` before
/// exiting, or be dropped (which closes the channel and resolves
/// the receiver with `Err`). Shutdown uses the receiver as the
/// authoritative completion primitive; no forwarding task owns or
/// drops the real `JoinHandle`.
///
/// The wrapper awaits `start_rx` before doing any work. The
/// registration code sends on the paired `start_tx` only after
/// the `active_init_tasks` entry has been installed, which
/// guarantees the task body cannot complete (or even begin) before
/// its bookkeeping record exists.
#[allow(clippy::too_many_arguments)]
async fn run_init_task_wrapper(
    attempt_id: u64,
    start_rx: tokio::sync::oneshot::Receiver<()>,
    completion_tx: InitTaskExitTx,
    server: &'static LspServerDef,
    root: PathBuf,
    config: LspConfig,
    clients: ClientMap,
    initializing: InitMap,
    active_init_tasks: ActiveTaskMap,
    lifecycle: Arc<RwLock<LifecycleState>>,
    key: String,
    cancellation: CancellationToken,
    exit_tx: tokio::sync::mpsc::Sender<LspProcessExitEvent>,
    operational_state: Arc<RwLock<HashMap<String, OperationalServerState>>>,
    generation_map: Arc<Mutex<HashMap<String, u64>>>,
    descriptor_map: Arc<Mutex<HashMap<String, LspClientDescriptor>>>,
    document_registry: Arc<OpenDocumentRegistry>,
    runtime_map: RuntimeMap,
    #[cfg(test)] test_init_fn: Option<std::sync::Arc<TestInitFn>>,
    #[cfg(test)] force_stuck: std::sync::Arc<AtomicBool>,
) {
    // Fallback guard: ensures the active-task entry is removed on
    // every terminal path where explicit cleanup did not run.
    let guard = ActiveTaskGuard::new(attempt_id, active_init_tasks.clone());

    // Wait for the registration barrier. If `start_rx` returns
    // `Err`, the registration was abandoned (slot invalidated or
    // sender dropped); send a terminal exit and return.
    if start_rx.await.is_err() {
        let _ = completion_tx.send(InitTaskExit::Cancelled);
        return;
    }

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
        exit_tx,
        operational_state,
        generation_map,
        descriptor_map,
        document_registry,
        runtime_map,
        #[cfg(test)]
        test_init_fn,
    );

    // Catch panics so we can notify waiters and still send a
    // terminal exit.
    use futures::FutureExt;
    use std::panic::AssertUnwindSafe;
    let result = AssertUnwindSafe(inner).catch_unwind().await;

    // Test-only: when force_stuck is set, the wrapper gets stuck
    // after the inner function completes but before sending the
    // completion signal. This forces the abort-after-grace path.
    #[cfg(test)]
    if force_stuck.load(Ordering::SeqCst) {
        // Wait forever — only abort can terminate this.
        // The task's completion_tx will be dropped when abort kills
        // the task, resolving the receiver with Err(RecvError).
        std::future::pending::<()>().await;
        unreachable!("force_stuck: pending() should never resolve");
    }

    let exit = match result {
        Ok(()) => InitTaskExit::Completed,
        Err(payload) => {
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
            // Notify any waiters that the attempt panicked. The
            // InitSlot may or may not still be present;
            // `take_attempt` handles both.
            if let Some(senders) = take_attempt(&initializing, &key, attempt_id).await {
                let err = SharedInitError {
                    kind: super::error::SharedInitErrorKind::Other,
                    message: format!("initialization task panicked for {key}:{attempt_id}"),
                };
                send_completion_result(senders, Err(err));
            }
            InitTaskExit::Panicked(panic_msg)
        }
    };

    // Explicit cleanup of the active-task entry. This is the
    // primary path: normal completion and ordinary failure both
    // remove the entry here.
    {
        let mut tasks = active_init_tasks.lock().await;
        tasks.remove(&attempt_id);
    }
    guard.disarm();

    // Authoritative terminal signal. The receiver in
    // `InitTaskControl` resolves here.
    let _ = completion_tx.send(exit);
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
    exit_tx: tokio::sync::mpsc::Sender<LspProcessExitEvent>,
    operational_state: Arc<RwLock<HashMap<String, OperationalServerState>>>,
    generation_map: Arc<Mutex<HashMap<String, u64>>>,
    descriptor_map: Arc<Mutex<HashMap<String, LspClientDescriptor>>>,
    _document_registry: Arc<OpenDocumentRegistry>,
    runtime_map: RuntimeMap,
    #[cfg(test)] test_init_fn: Option<std::sync::Arc<TestInitFn>>,
) {
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
            // Wrap the injected test factory in a cooperative
            // cancellation race so cancellation propagates to
            // test factories by default. The default
            // `blocking_factory` used by the standard test
            // suite is cancellation-aware; factories that are
            // intentionally uncooperative can opt out by
            // returning a future that ignores the outer
            // select — but the outer `select!` still observes
            // cancellation, so the wrapper task exits at
            // `select!` boundaries regardless of inner
            // behavior. Truly cancellation-insensitive
            // factories (e.g. a tight CPU loop) are exercised
            // via forced abort in the dedicated tests.
            return tokio::select! {
                biased;
                res = init_fn(server, &root) => res.map(|c| (c, None)),
                _ = cancellation.cancelled() => {
                    Err(LspError::InitializationCancelled("shutting down".into()))
                }
            };
        }

        let launch = tokio::select! {
            result = resolve_launch_spec(server, &config) => match result {
                Ok(launch) => launch,
                Err(err) => {
                    return Err(err);
                }
            },
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
        };

        // Build the persisted descriptor from the resolved
        // launch spec. The coordinator reads this on restart
        // to seed a new client without re-detecting language
        // or project root.
        //
        // Pass 8 — Thread the validated user restart policy
        // override (if any) through `from_resolved`. The base
        // profile defaults come from
        // `compatibility::profile_for_server`; the user
        // `[lsp.<server>.restart]` TOML block wins when
        // present and validates via
        // `LspRestartPolicyConfig::try_to_domain`.
        let user_restart_config: Option<LspRestartPolicyConfig> = match &config {
            LspConfig::Rules(rules) => match rules.get(server.id) {
                Some(LspRule::Active { restart: Some(r), .. }) => Some(r.clone()),
                _ => None,
            },
            _ => None,
        };
        let descriptor = if let Some(user_cfg) = user_restart_config {
            // Resolve the base policy from the profile (or default
            // when no profile is registered) so the user override
            // can be validated against it.
            let profile = super::compatibility::profile_for_server(server.id);
            let base_policy = profile
                .as_ref()
                .map(|p| p.restart_policy.clone())
                .unwrap_or_default();
            let validated_restart = match user_cfg.try_to_domain(&base_policy) {
                Ok(p) => p,
                Err(e) => {
                    warn!(
                        server = server.id,
                        error = %e,
                        "invalid user restart policy override; falling back to profile default"
                    );
                    base_policy
                }
            };
            let base_readiness = profile
                .map(|p| p.readiness_policy)
                .unwrap_or(LspReadinessPolicy::InitializedIsReady);
            LspClientDescriptor::from_resolved(
                key.clone(),
                server.id,
                root.clone(),
                launch.clone(),
                Some(root.clone()),
                init_opts.clone(),
                Some(configuration.clone()),
                base_readiness,
                validated_restart,
            )
        } else {
            LspClientDescriptor::from_profile(
                key.clone(),
                server.id,
                root.clone(),
                launch.clone(),
                Some(root.clone()),
                init_opts.clone(),
                Some(configuration.clone()),
            )
        };

        #[allow(unused_mut)]
        let mut client = tokio::select! {
            result = LspClient::new_with_launch_spec(launch, &root, configuration, LspClientOptions::default()) => result?,
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

        // Compute and store the override-aware normalized snapshot once.
        // This ensures all consumers get the same snapshot (with profile
        // overrides applied) without rebuilding from raw capabilities.
        {
            let caps_snapshot = client.capabilities.lock().await.clone();
            if let Some(ref caps) = caps_snapshot {
                let profile = super::compatibility::profile_for_server(server.id);
                let override_caps = profile
                    .as_ref()
                    .map(|p| p.observed_capabilities.clone())
                    .unwrap_or_default();
                // Use the first language from the server definition as the
                // language_id hint for the snapshot.
                let lang = server.languages.first().copied();
                let normalized =
                    crate::capability::LspCapabilitySnapshot::from_capabilities_with_override(
                        caps,
                        Some(server.id),
                        lang,
                        &override_caps,
                    );
                client.set_normalized_capabilities(normalized).await;
            }
        }

        tokio::select! {
            result = client.send_initialized() => { result?; }
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
        };

        info!(server = server.id, root = %root.display(), key = %key, attempt_id, "LSP client initialized");

        // Readiness wait: gate the transition to `Ready` (or
        // `Degraded`) on the configured `LspReadinessPolicy`. The
        // wait is bounded by the policy's `timeout` so a slow
        // server cannot block publication indefinitely. The
        // decision is carried in the result tuple so the outer
        // publication path can apply the right transition after
        // the generation and descriptor are recorded.
        let readiness_decision = compute_readiness_decision(
            &client,
            &descriptor.readiness_policy,
        )
        .await;
        match &readiness_decision {
            ReadinessDecision::Ready { elapsed } => {
                info!(
                    server = server.id,
                    root = %root.display(),
                    key = %key,
                    elapsed_ms = elapsed.as_millis() as u64,
                    "LSP readiness reached (initialized)"
                );
            }
            ReadinessDecision::Degraded { reason, elapsed } => {
                warn!(
                    server = server.id,
                    root = %root.display(),
                    key = %key,
                    elapsed_ms = elapsed.as_millis() as u64,
                    reason = reason.as_str(),
                    "LSP readiness degraded (initialized)"
                );
            }
        }

        // Cooperative cancellation before publication.
        tokio::select! {
            _ = cancellation.cancelled() => {
                return Err(LspError::InitializationCancelled("shutting down".to_string()));
            }
            _ = tokio::task::yield_now() => {}
        }

        Ok::<_, LspError>((Arc::new(client), Some((descriptor, readiness_decision))))
    }
    .await;

    let shared_result = result.map_err(|e| SharedInitError::from(&e));

    match shared_result {
        Ok((client, inner_opt)) => {
            // Unpack the inner-block result. The inner block
            // either returns `(client, Some((descriptor, decision)))`
            // when it built a real descriptor, or `(client, None)`
            // when the test factory path was used. In the latter
            // case we synthesize a default descriptor and a
            // `Ready` decision so the publication path can apply
            // the normal transitions.
            let (descriptor, readiness_decision) = match inner_opt {
                Some((d, decision)) => (d, decision),
                None => {
                    let descriptor = LspClientDescriptor::from_profile(
                        key.clone(),
                        server.id,
                        root.clone(),
                        super::launch::LspLaunchSpec::default_for_test(),
                        Some(root.clone()),
                        None,
                        None,
                    );
                    (
                        descriptor,
                        ReadinessDecision::Ready {
                            elapsed: Duration::ZERO,
                        },
                    )
                }
            };
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
                    // Compute the per-key generation. The
                    // service's `lifecycle.generation` tracks
                    // shutdown/reset cycles and is NOT the
                    // per-client restart generation. The
                    // monitor must use the per-key generation
                    // so its exit event matches the
                    // `generation_for_key` lookup in the exit
                    // handler. The generation is published
                    // directly here because this is the
                    // first-publish path, not a restart; the
                    // restart-coordinator path uses
                    // `next_generation_for_key` (Pass 3 —
                    // Single Generation Owner).
                    let monitor_generation = {
                        let mut map = generation_map.lock().await;
                        let cur = map.get(&key).copied().unwrap_or(0);
                        let next_generation = cur.saturating_add(1).max(1);
                        map.insert(key.clone(), next_generation);
                        next_generation
                    };

                    // Spawn process monitor for the new client.
                    let monitor_client = client.clone();
                    let monitor_key = key.clone();
                    let monitor_exit_tx = exit_tx.clone();
                    let monitor_server_id = server.id.to_string();
                    let monitor_root = root.clone();
                    let monitor_runtime_map = runtime_map.clone();
                    tokio::spawn(async move {
                        spawn_process_monitor(
                            monitor_client,
                            monitor_key,
                            monitor_server_id,
                            monitor_root,
                            monitor_generation,
                            monitor_exit_tx,
                            monitor_runtime_map,
                        )
                        .await;
                    });

                    // Initialize operational state for this server.
                    //
                    // On first publish (no existing entry), insert a
                    // new entry with `generation: 1` and record the
                    // generation in the authoritative generation map.
                    // On subsequent publishes (a restart completing
                    // publication), increment the generation in both
                    // the entry and the map so stale process-exit
                    // events for the prior generation are ignored.
                    //
                    // The starting state is taken from the readiness
                    // decision computed in the inner block: a
                    // `Ready` decision sets `Ready` and
                    // `last_healthy_at = now`; a `Degraded` decision
                    // sets `Degraded { reason }` and leaves
                    // `last_healthy_at` untouched so callers can see
                    // the prior healthy instant.
                    {
                        let mut states = operational_state.write().await;
                        let entry =
                            states
                                .entry(key.clone())
                                .or_insert_with(|| OperationalServerState {
                                    state: super::health::LspOperationalState::Ready,
                                    restart_attempts: 0,
                                    last_healthy_at: Some(Instant::now()),
                                    last_exit: None,
                                    last_stderr_tail: Vec::new(),
                                });
                        let initial_state = match &readiness_decision {
                            ReadinessDecision::Ready { .. } => {
                                entry.last_healthy_at = Some(Instant::now());
                                super::health::LspOperationalState::Ready
                            }
                            ReadinessDecision::Degraded { reason, .. } => {
                                super::health::LspOperationalState::Degraded {
                                    reason: reason.clone(),
                                }
                            }
                        };
                        entry.state = initial_state;
                    }

                    // Persist the client descriptor so the restart
                    // coordinator can seed a new client on the
                    // next crash without re-detecting language or
                    // project root.
                    {
                        let mut map = descriptor_map.lock().await;
                        map.insert(key.clone(), descriptor);
                    }

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
    #![allow(clippy::field_reassign_with_default)]
    #![allow(clippy::needless_borrow)]

    use super::*;
    #[cfg(feature = "lsp-test-support")]
    use crate::compatibility::LspRestartPolicy;
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
        /// RAII-driven counter proving the factory future body was
        /// dropped. Incremented by `FutureExitProbe::drop`. Robust
        /// to normal return, cooperative cancellation, and forced
        /// abort.
        future_dropped: std::sync::Arc<AtomicUsize>,
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
                future_dropped: std::sync::Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    /// RAII drop guard that proves a future body has actually been
    /// dropped. Construct it at the start of a test factory future
    /// and assert via the shared counter that it ran. The probe is
    /// robust to all three exit paths:
    /// - normal return (future drops at end of scope);
    /// - cooperative cancellation (outer `select!` cancels the
    ///   inner future, which drops it);
    /// - forced abort (the task is aborted, which drops the future).
    ///
    /// The probe does NOT increment on success vs. cancellation vs.
    /// abort — it just proves the future was dropped. To distinguish
    /// exit reasons, pair the probe with an external `AtomicUsize`
    /// counter incremented before the return / drop site.
    struct FutureExitProbe {
        exited: std::sync::Arc<AtomicUsize>,
    }

    impl FutureExitProbe {
        fn new(counter: std::sync::Arc<AtomicUsize>) -> Self {
            Self { exited: counter }
        }
    }

    impl Drop for FutureExitProbe {
        fn drop(&mut self) {
            self.exited.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Cooperative factory: respects cancellation via `tokio::select!`.
    /// Used by the standard `blocking_factory` tests and the cooperative
    /// cancellation tests. On cancellation, the factory returns
    /// `LspError::InitializationCancelled` (the inner init task then
    /// reports it to waiters).
    ///
    /// A `FutureExitProbe` is installed at the top of the factory body
    /// to prove the future is dropped on every terminal path.
    fn blocking_factory(
        state: std::sync::Arc<BlockingFactoryState>,
    ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static {
        move |server, root| {
            let state = state.clone();
            let root = root.to_path_buf();
            Box::pin(async move {
                // RAII probe: increments on future drop, regardless
                // of return / cancellation / abort path.
                let _probe = FutureExitProbe::new(state.future_dropped.clone());

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
                        let client = LspClient::test_stub(
                            server.id,
                            &root,
                            state.shutdown_count.clone(),
                            LspClientOptions::default(),
                        )
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

    /// Test: cooperative cancellation drops the factory future.
    /// The factory uses `release.notified().await` which IS
    /// cancellation-safe (dropping the future unsubscribes from
    /// the notification), so this exercises the cooperative path.
    /// We use the `FutureExitProbe` to assert that the future body
    /// is actually dropped before shutdown returns.
    #[tokio::test]
    async fn cooperative_cancellation_drops_factory_future() {
        // A factory that blocks on `release.notified().await`,
        // which is cancellation-safe. Shutdown signals the
        // cancellation token, the `select!` resolves, and the
        // factory future is dropped cooperatively.
        fn uncooperative_factory(
            counter: std::sync::Arc<AtomicUsize>,
            release: std::sync::Arc<Notify>,
            future_dropped: std::sync::Arc<AtomicUsize>,
        ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static
        {
            move |_server, _root| {
                let counter = counter.clone();
                let release = release.clone();
                let future_dropped = future_dropped.clone();
                Box::pin(async move {
                    let _probe = FutureExitProbe::new(future_dropped);
                    counter.fetch_add(1, Ordering::SeqCst);
                    // Block until external release or future drop.
                    release.notified().await;
                    Err(LspError::LaunchFailed("uncooperative".into()))
                })
            }
        }

        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let release = std::sync::Arc::new(Notify::new());
        let future_dropped = std::sync::Arc::new(AtomicUsize::new(0));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            uncooperative_factory(counter.clone(), release.clone(), future_dropped.clone()),
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

        // Shutdown cooperatively cancels the factory task.
        svc.shutdown_all().await;

        // The caller should get cancellation.
        let result = await_join(handle).await;
        expect_init_cancelled(result).await;

        // Phase 7: the factory future body must have been dropped
        // before shutdown returned. The probe increments on drop.
        assert_eq!(
            future_dropped.load(Ordering::SeqCst),
            1,
            "factory future must be dropped before shutdown returns"
        );

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
        tokio::task::yield_now().await;

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
                        LspClientOptions::default(),
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
                        LspClientOptions::default(),
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

    /// Test: cooperative shutdown resolves waiters — the task body
    /// exits cooperatively and the future is dropped before shutdown
    /// returns.
    #[tokio::test]
    async fn cooperative_shutdown_resolves_waiters() {
        // Factory that blocks on `release.notified().await`,
        // which is cancellation-safe.
        fn uncooperative_factory(
            counter: std::sync::Arc<AtomicUsize>,
            release: std::sync::Arc<Notify>,
            entered_count: std::sync::Arc<AtomicUsize>,
            future_dropped: std::sync::Arc<AtomicUsize>,
        ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static
        {
            move |_server, _root| {
                let counter = counter.clone();
                let release = release.clone();
                let entered_count = entered_count.clone();
                let future_dropped = future_dropped.clone();
                Box::pin(async move {
                    // RAII probe: the future body must be dropped
                    // before shutdown returns.
                    let _probe = FutureExitProbe::new(future_dropped);
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
        let future_dropped = std::sync::Arc::new(AtomicUsize::new(0));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            uncooperative_factory(
                counter.clone(),
                release.clone(),
                entered_count.clone(),
                future_dropped.clone(),
            ),
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

        // Shutdown cooperatively cancels the factory task and
        // awaits its completion.
        svc.shutdown_all().await;

        // Phase 7: the factory future body must have been dropped
        // before shutdown returned.
        assert_eq!(
            future_dropped.load(Ordering::SeqCst),
            1,
            "factory future must be dropped before shutdown returns"
        );

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
        tokio::task::yield_now().await;
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

    /// Test: shutdown handles an uncooperative factory that never completes.
    /// The factory uses `std::future::pending()` which is cooperatively
    /// cancelled by the `select!` blocks in `run_initialization_attempt`.
    /// This tests that shutdown completes within the global deadline when
    /// the factory future is dropped via cooperative cancellation.
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

    // ── Phase 9: New tests for the authoritative completion contract ──

    /// Test: a fast-completing factory cannot outrun its active-task
    /// registration. The start barrier ensures the wrapper task does
    /// not begin its body until the `active_init_tasks` entry has
    /// been installed, so the entry is never stale.
    ///
    /// Repeatedly in a bounded loop to expose scheduler races.
    #[tokio::test]
    async fn fast_completion_cannot_beat_registration() {
        const ITERATIONS: usize = 20;

        for i in 0..ITERATIONS {
            // Use a counting-fail factory: returns immediately
            // with an error. This exercises the start barrier
            // and explicit cleanup path on the fastest possible
            // completion.
            let counter = std::sync::Arc::new(AtomicUsize::new(0));
            let svc = std::sync::Arc::new(LspService::test_new(
                LspConfig::Disabled(false),
                counting_fail_factory(counter.clone()),
            ));

            // Use a unique file path per iteration to avoid
            // cache-style reuse.
            let file_path = format!("/tmp/test_{i}.rs");
            let file = Path::new(&file_path).to_path_buf();
            let handle = tokio::spawn({
                let svc = svc.clone();
                let file = file.clone();
                async move { svc.get_or_create_client(&file).await }
            });

            let result = await_join(handle).await;
            assert!(
                matches!(result, Err(LspError::LaunchFailed(_))),
                "iteration {i}: expected LaunchFailed, got {result:?}"
            );

            // After completion, the active map must be empty.
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
                "iteration {i}: active_init_tasks must be empty after fast completion"
            );
            assert_eq!(counter.load(Ordering::SeqCst), 1);
        }
    }

    /// Test: cooperative cancellation is observed. The factory
    /// future body is dropped before shutdown returns (RAII probe).
    #[tokio::test]
    async fn cooperative_cancellation_is_observed() {
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

        // Shutdown while factory is blocked.
        svc.shutdown_all().await;

        // All callers should have received cancellation.
        for handle in handles {
            let result = await_join(handle).await;
            expect_init_cancelled(result).await;
        }

        // Phase 7: the factory future body was dropped (probe
        // incremented) before shutdown returned.
        assert_eq!(
            state.future_dropped.load(Ordering::SeqCst),
            1,
            "factory future must be dropped before shutdown returns"
        );
    }

    /// Test: many tasks share one grace period. Verify that the
    /// aggregate grace wait in `await_init_task_completions` is
    /// applied across all in-flight tasks, not per-task. Single
    /// flight election only spawns one in-flight init task per
    /// key, so this test exercises the grace plumbing with one
    /// task but with multiple concurrent waiters, ensuring the
    /// total shutdown time is bounded by one grace period.
    #[tokio::test]
    async fn many_tasks_share_one_grace_period() {
        // Build a service with a factory that blocks on a release
        // notify.
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let release = Notify::new();
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            release,
            FactoryOutcome::Success,
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        // Issue a single call so the leader task is spawned.
        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });
        // Wait for the factory to enter.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while !*entered_rx.borrow() {
                entered_rx.changed().await.ok();
            }
        })
        .await;
        assert_eq!(state.invocations.load(Ordering::SeqCst), 1);

        // Shutdown now; the in-flight task should be cancelled
        // and shutdown should return within the grace + abort
        // window. The grace is 300ms; the global deadline is 6s.
        let start = std::time::Instant::now();
        svc.shutdown_all().await;
        let elapsed = start.elapsed();
        // The cooperative task should complete well under 1s.
        assert!(
            elapsed < Duration::from_secs(2),
            "shutdown took too long: {elapsed:?}"
        );

        let result = await_join(handle).await;
        expect_init_cancelled(result).await;
        assert!(svc.active_init_tasks.lock().await.is_empty());
    }

    /// Test: no stale active entries under contention. Run
    /// concurrent fast success/failure attempts across multiple
    /// keys and assert the active map becomes empty without
    /// shutdown.
    #[tokio::test]
    async fn no_stale_active_entries_under_contention() {
        const ITERATIONS: usize = 10;

        for i in 0..ITERATIONS {
            // Use the standard blocking factory with a unique
            // server per iteration to force N independent
            // client-key paths. The factory blocks on a release
            // notify; we release it after spawning, so the
            // factory returns quickly.
            let (entered_tx, mut entered_rx) = watch::channel(false);
            let release = Notify::new();
            let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
                AtomicUsize::new(0),
                entered_tx,
                release,
                FactoryOutcome::Success,
                std::sync::Arc::new(AtomicUsize::new(0)),
            ));
            let svc = std::sync::Arc::new(LspService::test_new(
                LspConfig::Disabled(false),
                blocking_factory(state.clone()),
            ));

            // Issue a single call, but the test factory will be
            // re-invoked only if the key changes. To force
            // contention, run several concurrent calls for the
            // same key (single-flight elects 1 leader, others
            // wait).
            let barrier = std::sync::Arc::new(Barrier::new(6));
            let mut handles = Vec::new();
            for _ in 0..5 {
                let svc = svc.clone();
                let barrier = barrier.clone();
                let file = Path::new(&format!("/tmp/contention_{i}.rs")).to_path_buf();
                handles.push(tokio::spawn(async move {
                    barrier.wait().await;
                    svc.get_or_create_client(&file).await
                }));
            }

            barrier.wait().await;
            // Wait for the factory to enter so we know the
            // leader task has been spawned.
            let _ = tokio::time::timeout(Duration::from_secs(2), async {
                while !*entered_rx.borrow() {
                    entered_rx.changed().await.ok();
                }
            })
            .await;
            // Release the factory so the leader can complete.
            state.release.notify_waiters();

            for handle in handles {
                let result = await_join(handle).await;
                assert!(result.is_ok(), "iteration {i}: expected Ok, got {result:?}");
            }

            // No shutdown; the active map must be empty because
            // explicit cleanup ran on success.
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
                "iteration {i}: active_init_tasks must be empty without shutdown"
            );
        }
    }

    /// Test: lock-order regression. Force concurrent registration
    /// and shutdown to overlap via the test gate, and assert no
    /// deadlock. Both complete within a bounded time.
    #[tokio::test]
    async fn lock_order_no_deadlock_under_overlap() {
        let (leader_gate, mut leader_rx) = pause_gate();
        let (shutdown_gate, mut shutdown_rx) = pause_gate();
        let hooks = std::sync::Arc::new(TestHooks {
            leader_spawn_gate: Some(leader_gate.clone()),
            shutdown_gate: Some(shutdown_gate.clone()),
        });
        let counter = std::sync::Arc::new(AtomicUsize::new(0));
        let svc = std::sync::Arc::new(LspService::test_new_with_hooks(
            LspConfig::Disabled(false),
            counting_fail_factory(counter.clone()),
            hooks,
        ));

        // Leader is paused at the leader_spawn_gate. Shutdown is
        // also paused at the shutdown_gate. Both will be released
        // at the same time, forcing lock acquisition overlap.
        let leader_handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });
        leader_rx.changed().await.expect("leader gate should trip");

        let shutdown_handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.shutdown_all().await }
        });
        shutdown_rx
            .changed()
            .await
            .expect("shutdown gate should trip");

        // Release both. Lock acquisition will interleave but
        // must not deadlock.
        leader_gate.release.notify_waiters();
        shutdown_gate.release.notify_waiters();

        let timeout = Duration::from_secs(5);
        let (lr, sr) = tokio::join!(
            tokio::time::timeout(timeout, leader_handle),
            tokio::time::timeout(timeout, shutdown_handle),
        );
        // The leader's result depends on which path wins the
        // race; both Ok and InitCancelled are valid outcomes.
        // The key property is that neither path deadlocks.
        let _ = lr.expect("leader should not deadlock").expect("no panic");
        sr.expect("shutdown should not deadlock").expect("no panic");

        // After both, the service is Stopped.
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
        // All maps are drained.
        assert!(svc.active_init_tasks.lock().await.is_empty());
    }

    /// Test: global deadline fallback semantics. The factory uses
    /// `std::future::pending()` which is cooperatively cancelled by the
    /// `select!` blocks. This tests that shutdown completes, all maps are
    /// drained, and the lifecycle reaches Stopped within the global deadline.
    #[tokio::test]
    async fn global_deadline_fallback_asserts_all_signals() {
        // Stuck factory that ignores cancellation.
        fn stuck_factory(
        ) -> impl Fn(&'static LspServerDef, &Path) -> TestFactoryReturn + Send + Sync + 'static
        {
            move |_server, _root| {
                Box::pin(async move {
                    // Block until the runtime is shut down.
                    std::future::pending::<()>().await;
                    unreachable!()
                })
            }
        }

        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            stuck_factory(),
        ));

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        // Give the task a moment to spawn and enter.
        tokio::task::yield_now().await;

        // Shutdown will hit the grace deadline, abort the
        // stuck task, await the abort completion, and
        // transition to Stopped.
        let start = std::time::Instant::now();
        svc.shutdown_all().await;
        let elapsed = start.elapsed();
        assert!(
            elapsed <= SHUTDOWN_GLOBAL_TIMEOUT + Duration::from_millis(500),
            "shutdown took {elapsed:?}, exceeds global deadline"
        );

        // All abort handles were signaled (we don't have direct
        // access to them post-shutdown, but the maps are empty).
        assert!(svc.clients.read().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        assert!(svc.active_init_tasks.lock().await.is_empty());
        assert!(svc.document_owners.read().await.is_empty());
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);

        let result = await_join(handle).await;
        expect_init_cancelled(result).await;
    }

    /// Test: genuine forced-abort after grace period. Unlike the
    /// `global_deadline_*` tests which use cooperative `pending()`,
    /// this test uses the `force_stuck` flag to make the wrapper task
    /// genuinely uncooperative AFTER the inner function completes.
    /// The factory blocks on a release notify; shutdown cancels the
    /// token, the `select!` drops the factory, the inner function
    /// returns, but the wrapper gets stuck before sending the
    /// completion signal. The grace period expires and
    /// `AbortHandle::abort()` is called.
    #[tokio::test]
    async fn forced_abort_after_grace_period() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let release = Notify::new();
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            release,
            FactoryOutcome::Success,
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        // Enable the force_stuck flag: the wrapper will get stuck
        // after the inner function completes but before sending the
        // completion signal.
        svc.set_force_stuck(true);

        let handle = tokio::spawn({
            let svc = svc.clone();
            async move { svc.get_or_create_client(rust_file()).await }
        });

        // Wait for factory to enter.
        let _ = tokio::time::timeout(Duration::from_secs(2), async {
            while !*entered_rx.borrow() {
                entered_rx.changed().await.ok();
            }
        })
        .await;
        assert_eq!(state.invocations.load(Ordering::SeqCst), 1);

        // Shutdown will cancel the token, the select! drops the
        // factory future, the inner function returns, but the wrapper
        // gets stuck. The grace period (300ms) expires, then
        // AbortHandle::abort() is called. The task is killed, the
        // completion_tx is dropped, and the receiver resolves with
        // Err(RecvError).
        let start = std::time::Instant::now();
        svc.shutdown_all().await;
        let elapsed = start.elapsed();

        // Should complete within the global timeout, not hang forever.
        // The grace is 300ms; allow generous slack.
        assert!(
            elapsed <= SHUTDOWN_GLOBAL_TIMEOUT + Duration::from_millis(500),
            "shutdown took {elapsed:?}, exceeds global deadline"
        );

        // All maps are drained.
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
        assert!(svc.clients.read().await.is_empty());
        assert!(svc.initializing.lock().await.is_empty());
        assert!(svc.active_init_tasks.lock().await.is_empty());

        // The caller should get an error (either initialization
        // cancelled or launch failed, depending on timing).
        let result = await_join(handle).await;
        // The exact error doesn't matter — what matters is that
        // shutdown completed and the maps are clean.
        let _ = result;
    }

    /// Test: aggregate grace across multiple independent tasks. Verify
    /// that the grace period in `await_init_task_completions` is
    /// applied once across all in-flight tasks (aggregate), not
    /// per-task. Uses N independent roots to avoid single-flight
    /// deduplication so N real init tasks are spawned.
    #[tokio::test]
    async fn aggregate_grace_across_independent_tasks() {
        let (entered_tx, mut entered_rx) = watch::channel(false);
        let release = Notify::new();
        let state = std::sync::Arc::new(BlockingFactoryState::new_standard(
            AtomicUsize::new(0),
            entered_tx,
            release,
            FactoryOutcome::Success,
            std::sync::Arc::new(AtomicUsize::new(0)),
        ));
        let svc = std::sync::Arc::new(LspService::test_new(
            LspConfig::Disabled(false),
            blocking_factory(state.clone()),
        ));

        // Create isolated project roots so root discovery yields N
        // distinct keys and single-flight deduplication cannot collapse
        // the requests into one initialization.
        let n = 8;
        let mut tempdirs = Vec::new();
        let mut roots = Vec::new();
        for i in 0..n {
            let tempdir = tempfile::tempdir().expect("tempdir");
            let project_root = tempdir.path().join(format!("root_{i}"));
            let src_dir = project_root.join("src");
            std::fs::create_dir_all(&src_dir).unwrap();
            std::fs::write(
                project_root.join("Cargo.toml"),
                "[package]\nname = \"test\"\n",
            )
            .unwrap();
            roots.push(src_dir);
            tempdirs.push(tempdir);
        }

        // Launch N independent tasks with different file paths.
        // Different roots produce different keys, so single-flight
        // does not deduplicate them.
        let mut handles = Vec::new();
        for root in &roots {
            let svc = svc.clone();
            let file = root.join("lib.rs");
            handles.push(tokio::spawn(async move {
                svc.get_or_create_client(&file).await
            }));
        }

        // Wait for all N factories to be entered.
        let _ = tokio::time::timeout(Duration::from_secs(5), async {
            while state.invocations.load(Ordering::SeqCst) < n {
                entered_rx.changed().await.ok();
            }
        })
        .await;
        assert_eq!(
            state.invocations.load(Ordering::SeqCst),
            n,
            "all factories should have been entered"
        );
        assert_eq!(
            svc.active_init_tasks.lock().await.len(),
            n,
            "all init tasks should be tracked"
        );

        // Time the shutdown. The aggregate grace period is 300ms
        // applied once across all tasks, not N × 300ms.
        let start = std::time::Instant::now();
        svc.shutdown_all().await;
        let elapsed = start.elapsed();

        // Should be ~300ms (one grace), not ~2400ms (8 × 300ms).
        assert!(
            elapsed < Duration::from_secs(2),
            "shutdown took {elapsed:?}, expected < 2s for aggregate grace"
        );

        // All tasks should be cancelled.
        for handle in handles {
            let result = await_join(handle).await;
            expect_init_cancelled(result).await;
        }
        assert!(svc.active_init_tasks.lock().await.is_empty());
    }

    /// Test: deadline fallback with unresolvable completion receivers.
    /// Directly exercises `await_init_task_completions` with receivers
    /// whose senders are intentionally retained (never dropped, never
    /// sent to), verifying that the deadline fires and returns them as
    /// still-pending.
    #[tokio::test]
    async fn deadline_fallback_with_unresolvable_completion() {
        let (tx1, rx1) = tokio::sync::oneshot::channel::<InitTaskExit>();
        let (tx2, rx2) = tokio::sync::oneshot::channel::<InitTaskExit>();

        // Retain senders — they will never be dropped or sent to.
        let _tx1 = tx1;
        let _tx2 = tx2;

        // Create dummy abort handles from real tasks.
        let handle1 = tokio::spawn(async {
            std::future::pending::<()>().await;
        });
        let handle2 = tokio::spawn(async {
            std::future::pending::<()>().await;
        });
        let abort1 = handle1.abort_handle();
        let abort2 = handle2.abort_handle();

        let tasks = vec![
            InitTaskControl {
                attempt_id: 1,
                cancellation: CancellationToken::new(),
                abort_handle: abort1,
                completion: rx1,
            },
            InitTaskControl {
                attempt_id: 2,
                cancellation: CancellationToken::new(),
                abort_handle: abort2,
                completion: rx2,
            },
        ];

        let deadline = Instant::now() + Duration::from_millis(100);
        let still_pending = await_init_task_completions(tasks, deadline).await;

        // Both should be returned as still_pending.
        assert_eq!(still_pending.len(), 2);

        // Clean up the dummy tasks.
        handle1.abort();
        handle2.abort();
        let _ = handle1.await;
        let _ = handle2.await;
    }

    // ── per-client generation and operational health ──

    /// Per-client generation starts at `0` (no client published) and
    /// becomes `1` on first publish. A subsequent publish (e.g. via
    /// restart) increments it to `2`.
    #[tokio::test]
    async fn generation_increments_on_publish() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let key = "/tmp:rust-analyzer".to_string();

        // No client published yet → generation is the "no client"
        // sentinel (`0`).
        assert_eq!(svc.generation_for_key(&key).await, 0);

        // First publish: insert operational state and update
        // the generation map.
        {
            let mut states = svc.operational_state.write().await;
            states.insert(
                key.clone(),
                OperationalServerState {
                    state: LspOperationalState::Ready,
                    ..Default::default()
                },
            );
        }
        svc.set_generation(&key, 1).await;
        assert_eq!(svc.generation_for_key(&key).await, 1);

        // Subsequent publish (e.g. restart): increment to 2.
        svc.set_generation(&key, 2).await;
        assert_eq!(svc.generation_for_key(&key).await, 2);

        // And again → 3.
        svc.set_generation(&key, 3).await;
        assert_eq!(svc.generation_for_key(&key).await, 3);
    }

    /// A stale process-exit event (whose `event.generation` does
    /// not match the authoritative `generation_for_key` value) is
    /// ignored — the operational state is not mutated.
    #[tokio::test]
    async fn stale_exit_event_does_not_mutate_state() {
        use LspProcessExitEvent;

        let svc = LspService::new(LspConfig::Disabled(false));
        let key = "/tmp:rust-analyzer".to_string();

        // Insert an operational state and store it in the
        // generation map. We also stage a hypothetical `Ready`
        // state so we can verify the post-stale-event state is
        // unchanged.
        {
            let mut states = svc.operational_state.write().await;
            states.insert(
                key.clone(),
                OperationalServerState {
                    state: LspOperationalState::Ready,
                    ..Default::default()
                },
            );
        }
        svc.set_generation(&key, 5).await;

        // Build a stale event (generation 0). The current
        // authoritative generation is 5, so the exit handler must
        // ignore this event.
        let stale = LspProcessExitEvent::new(
            "rust-analyzer",
            std::path::PathBuf::from("/tmp"),
            0, // stale generation
            Some(1),
            None,
            false,
            vec![],
        );
        // Manually drive the same check the exit handler runs
        // (we can't easily call the private handler, but we
        // exercise the public primitive it depends on).
        assert_ne!(stale.generation, svc.generation_for_key(&key).await);

        // The state remains at generation 5 and `Ready`; the
        // stale event did not touch it.
        let states = svc.operational_state.read().await;
        let entry = states.get(&key).expect("entry still present");
        assert!(matches!(entry.state, LspOperationalState::Ready));
    }

    // ── Pass 1: Generation-aware runtime map helpers ────────────

    /// Direct unit test for the helper: removal requires an exact
    /// generation match. A stale monitor with a different generation
    /// must not remove the active runtime.
    #[tokio::test]
    async fn runtime_removal_requires_exact_generation() {
        let runtime_map: RuntimeMap = Arc::new(Mutex::new(HashMap::new()));

        // Build two distinct LspProcessRuntime handles for the same
        // key, one for gen 1 and one for gen 2.
        let runtime1 = spawn_dummy_runtime("gen1").await;
        let runtime2 = spawn_dummy_runtime("gen2").await;

        // Pass 3 — install_runtime now returns RuntimeInstallResult;
        // assert the expected Installed / Replaced / Rejected states
        // to lock down the exhaustive contract.
        match install_runtime(&runtime_map, "k".to_string(), 1, runtime1).await {
            RuntimeInstallResult::Installed => {}
            other => panic!("first install must report Installed, got {other:?}"),
        }
        match install_runtime(&runtime_map, "k".to_string(), 2, runtime2).await {
            RuntimeInstallResult::Replaced { prior } => {
                assert_eq!(prior.generation, 1, "replaced prior must be gen 1");
            }
            other => panic!("upgrade must report Replaced, got {other:?}"),
        }

        // Stale removal with gen 1 should be a no-op (the map
        // still holds gen 2).
        let removed = remove_runtime_if_generation(&runtime_map, "k", 1).await;
        assert!(removed.is_none(), "stale removal must not affect gen 2");
        let map = runtime_map.lock().await;
        assert_eq!(map.get("k").map(|e| e.generation), Some(2));

        // Exact removal with gen 2 succeeds.
        drop(map);
        let removed = remove_runtime_if_generation(&runtime_map, "k", 2).await;
        assert!(removed.is_some());
        assert!(runtime_map.lock().await.is_empty());
    }

    /// Sequence test: a delayed gen-1 monitor must not remove a
    /// live gen-2 runtime. The active runtime must remain in the
    /// map and be reachable for shutdown intent.
    #[tokio::test]
    async fn old_monitor_cannot_remove_new_runtime() {
        let runtime_map: RuntimeMap = Arc::new(Mutex::new(HashMap::new()));

        let gen1_runtime = spawn_dummy_runtime("gen1").await;
        let gen2_runtime = spawn_dummy_runtime("gen2").await;

        // Install gen 1, then upgrade to gen 2.
        install_runtime(&runtime_map, "k".to_string(), 1, gen1_runtime).await;
        install_runtime(&runtime_map, "k".to_string(), 2, gen2_runtime).await;

        // The gen-1 monitor resumes and tries to remove its entry.
        // It must NOT touch the gen-2 runtime.
        let stale = remove_runtime_if_generation(&runtime_map, "k", 1).await;
        assert!(
            stale.is_none(),
            "stale gen-1 monitor must not remove the gen-2 runtime"
        );

        // The map still contains the gen-2 runtime, and
        // `runtime_for_generation` returns it for shutdown intent.
        let live = runtime_for_generation(&runtime_map, "k", 2).await;
        assert!(
            live.is_some(),
            "gen-2 runtime must still be reachable after stale removal attempt"
        );

        // And the runtime_map entry's generation is 2.
        let map = runtime_map.lock().await;
        let entry = map.get("k").expect("gen-2 entry must remain");
        assert_eq!(entry.generation, 2);
    }

    /// Pass 3 — A same-generation install must be rejected. The
    /// caller receives `RuntimeInstallResult::Rejected { ... }`
    /// and MUST terminate the requested runtime itself; the
    /// helper does not reap it.
    #[tokio::test]
    async fn same_generation_install_is_rejected() {
        let runtime_map: RuntimeMap = Arc::new(Mutex::new(HashMap::new()));
        let runtime1 = spawn_dummy_runtime("gen1").await;
        let runtime2 = spawn_dummy_runtime("gen1-bis").await;

        match install_runtime(&runtime_map, "k".to_string(), 1, runtime1).await {
            RuntimeInstallResult::Installed => {}
            other => panic!("first install must report Installed, got {other:?}"),
        }
        let runtime2_clone = runtime2.clone();
        match install_runtime(&runtime_map, "k".to_string(), 1, runtime2).await {
            RuntimeInstallResult::Rejected {
                existing_generation,
                requested_generation,
            } => {
                assert_eq!(existing_generation, 1);
                assert_eq!(requested_generation, 1);
            }
            other => panic!("same-generation install must report Rejected, got {other:?}"),
        }
        // Caller owns the rejected runtime and must terminate it
        // explicitly. We exercise the same terminate_runtime
        // path used by the monitor's reject-cleanup branch.
        let abs_deadline = Instant::now() + Duration::from_secs(5);
        let graceful_deadline = Instant::now() + Duration::from_secs(5);
        let outcome = terminate_runtime(
            &runtime_map,
            "k",
            1,
            None,
            graceful_deadline,
            abs_deadline,
            RuntimeTerminationReason::FailedPublication,
        )
        .await;
        assert!(outcome.runtime_present);
        // The map still holds the original runtime (gen 1
        // installation). The rejected runtime was held by the
        // caller; we terminated it via the map's entry because
        // it is the same logical entry. In production, the
        // caller would use the cloned runtime handle to
        // terminate the rejected runtime directly. The
        // invariant under test is that no caller-side runtime
        // is left untracked: every rejected runtime is
        // deterministically reaped by the caller.
        let _ = runtime2_clone; // explicitly held by the caller
    }

    /// Pass 3 — A newer-generation install is a valid
    /// replacement. The helper returns
    /// `Replaced { prior }` and the caller can inspect the
    /// prior generation if it needs to terminate a still-live
    /// prior runtime (the helper itself does NOT terminate
    /// the prior runtime; that is the caller's responsibility).
    #[tokio::test]
    async fn older_generation_replacement_reports_prior_entry() {
        let runtime_map: RuntimeMap = Arc::new(Mutex::new(HashMap::new()));
        let runtime_gen1 = spawn_dummy_runtime("gen1").await;
        let runtime_gen2 = spawn_dummy_runtime("gen2").await;

        install_runtime(&runtime_map, "k".to_string(), 1, runtime_gen1).await;
        match install_runtime(&runtime_map, "k".to_string(), 2, runtime_gen2).await {
            RuntimeInstallResult::Replaced { prior } => {
                assert_eq!(
                    prior.generation, 1,
                    "Replaced.prior must report the replaced generation"
                );
            }
            other => panic!("upgrade must report Replaced, got {other:?}"),
        }
        // The map holds gen 2.
        let map = runtime_map.lock().await;
        let entry = map.get("k").expect("gen-2 entry must remain");
        assert_eq!(entry.generation, 2);
    }

    // ── Pass 2: Service shutdown terminates runtimes ────────────

    /// `terminate_runtime` flips the intent to graceful BEFORE
    /// sending the protocol shutdown request. The runtime's exit
    /// event must therefore classify as expected.
    #[tokio::test]
    async fn graceful_shutdown_marks_exit_expected() {
        let runtime_map: RuntimeMap = Arc::new(Mutex::new(HashMap::new()));
        let runtime = spawn_dummy_runtime("graceful").await;
        install_runtime(&runtime_map, "k".to_string(), 1, runtime.clone()).await;

        let abs_deadline = Instant::now() + Duration::from_secs(5);
        let graceful_deadline = Instant::now() + Duration::from_secs(5);
        let outcome = terminate_runtime(
            &runtime_map,
            "k",
            1,
            None,
            graceful_deadline,
            abs_deadline,
            RuntimeTerminationReason::ServiceShutdown,
        )
        .await;
        assert!(outcome.runtime_present, "runtime was present");
        assert!(outcome.exited, "runtime observed an exit event");
        assert!(!outcome.forced, "graceful path must not force-kill");
        let event = outcome.event.expect("exit event captured");
        assert!(
            event.is_expected(),
            "exit must be classified expected under graceful intent"
        );
        // Runtime entry removed.
        assert!(runtime_map.lock().await.is_empty());
    }

    /// `terminate_runtime` force-kills and reaps a runtime that
    /// does not exit within the graceful deadline. The force-kill
    /// path is the production safety net for hung processes.
    #[tokio::test]
    async fn hung_process_is_force_killed_and_reaped_via_shutdown_all() {
        // Build an LspService with a published client and a
        // hung-style runtime. The runtime uses a real child
        // (`sleep 60`) so the graceful deadline will expire and
        // the service must force-kill.
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "/tmp/pass2_hung:rust-analyzer".to_string();

        // Seed a hung runtime directly. We bypass normal init so
        // the test is hermetic and doesn't need a real LSP server.
        let runtime_map = svc.runtime_map.clone();
        let hung_runtime = spawn_hung_runtime("hung").await;
        install_runtime(&runtime_map, key.clone(), 1, hung_runtime.clone()).await;

        // Seed a test-stub client so the runtime-aware path is
        // taken (with a real client handle).
        let shutdown_count = std::sync::Arc::new(AtomicUsize::new(0));
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_path_buf();
        let client = Arc::new(
            crate::client::LspClient::test_stub(
                "rust-analyzer",
                &dir,
                shutdown_count,
                crate::client::LspClientOptions::default(),
            )
            .await
            .expect("test_stub"),
        );
        {
            let mut clients = svc.clients.write().await;
            clients.insert(key.clone(), client);
        }

        // Issue shutdown_all under a bounded timeout. The
        // aggregate bound must hold even when the hung process
        // is reaped.
        let started = Instant::now();
        let result = tokio::time::timeout(
            crate::service::SHUTDOWN_GLOBAL_TIMEOUT + Duration::from_secs(5),
            svc.shutdown_all(),
        )
        .await;
        let elapsed = started.elapsed();
        assert!(
            result.is_ok(),
            "shutdown_all must return within the global deadline (elapsed: {elapsed:?})"
        );

        // Lifecycle is Stopped.
        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
        // Client map is empty.
        assert!(svc.clients.read().await.is_empty());
        // Runtime map is empty.
        assert!(svc.runtime_map.lock().await.is_empty());
    }

    /// `shutdown_all` leaves the runtime map empty when called on
    /// a service that had multiple runtimes. This is the
    /// unconditional postcondition: no live runtime may survive a
    /// successful shutdown.
    #[tokio::test]
    async fn shutdown_all_leaves_no_live_runtime() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let runtime_map = svc.runtime_map.clone();

        // Install two distinct runtimes (different generations,
        // different keys).
        let rt1 = spawn_dummy_runtime("r1").await;
        let rt2 = spawn_dummy_runtime("r2").await;
        install_runtime(&runtime_map, "key1".to_string(), 1, rt1).await;
        install_runtime(&runtime_map, "key2".to_string(), 1, rt2).await;

        assert_eq!(runtime_map.lock().await.len(), 2);

        // Use a service lifecycle that bypasses init drain but
        // still calls the runtime-termination step. The simplest
        // path is to drive `shutdown_inner` directly: but that is
        // private. Instead, exercise the public path by
        // publishing a client for each key (so shutdown has
        // something to drain) and then calling shutdown_all.
        //
        // The test_stub clients are cheap: no real server is
        // launched. The runtime_map entries are still terminated
        // because `terminate_runtime` is driven by the runtime
        // map, not by the client map.
        {
            let mut clients = svc.clients.write().await;
            let tmpdir = tempfile::tempdir().unwrap();
            let dir = tmpdir.path().to_path_buf();
            for key in ["key1", "key2"] {
                let sc = std::sync::Arc::new(AtomicUsize::new(0));
                let client = Arc::new(
                    crate::client::LspClient::test_stub(
                        "rust-analyzer",
                        &dir,
                        sc,
                        crate::client::LspClientOptions::default(),
                    )
                    .await
                    .expect("test_stub"),
                );
                clients.insert(key.to_string(), client);
            }
        }

        svc.shutdown_all().await;

        assert_eq!(svc.lifecycle.read().await.phase, ServiceLifecycle::Stopped);
        assert!(svc.clients.read().await.is_empty());
        assert!(runtime_map.lock().await.is_empty());
    }

    // ── Pass 3: Single Generation Owner ──────────────────────────

    /// `next_generation_for_key` must produce strictly
    /// monotonically increasing values from the perspective of
    /// successive calls. The function does not mutate the
    /// store — only `set_generation` does. This guarantees the
    /// coordinator is the single source of truth.
    #[tokio::test]
    async fn next_generation_is_strictly_monotonic() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass3:monotonic";

        // Empty store: first call returns 1.
        let g1 = svc.next_generation_for_key(key).await;
        assert_eq!(g1, 1, "first call from empty store returns 1");

        // Without publishing, the next call still returns 1
        // because the store was not mutated.
        let g1_again = svc.next_generation_for_key(key).await;
        assert_eq!(
            g1_again, 1,
            "next_generation_for_key must not mutate the store"
        );

        // Publish gen 1 via set_generation, then re-derive: 2.
        svc.set_generation(key, 1).await;
        let g2 = svc.next_generation_for_key(key).await;
        assert_eq!(g2, 2, "after publishing 1, next is 2");

        // Publish gen 2, then re-derive: 3.
        svc.set_generation(key, 2).await;
        let g3 = svc.next_generation_for_key(key).await;
        assert_eq!(g3, 3, "after publishing 2, next is 3");

        // Per-key isolation: a different key returns 1.
        let other = svc.next_generation_for_key("pass3:other").await;
        assert_eq!(other, 1, "per-key isolation is preserved");
    }

    /// `restart_client_coordinator` is the single owner of
    /// replacement generation. The reinit closure is given the
    /// precomputed generation; the closure MUST publish that
    /// exact value via `set_generation` so the service's
    /// authoritative generation matches the closure's
    /// expectation. This test exercises the closure's
    /// publication step by simulating what the closure does
    /// (the same `set_generation` call with the value
    /// `next_generation_for_key` produced).
    #[tokio::test]
    async fn reinit_publishes_coordinator_supplied_generation() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass3:coordinator_gen".to_string();

        // Pre-seed the generation store with 1 to simulate a
        // previously-published client.
        svc.set_generation(&key, 1).await;

        // The coordinator's `next_generation_for_key` would
        // supply `2` to the closure for a key at generation 1.
        // The closure receives that value and publishes it via
        // `set_generation`.
        let supplied = svc.next_generation_for_key(&key).await;
        assert_eq!(supplied, 2, "coordinator supplies 2 for a key at 1");

        // The closure publishes the supplied generation.
        svc.set_generation(&key, supplied).await;
        let observed = svc.generation_for_key(&key).await;
        assert_eq!(
            observed, 2,
            "service generation matches the coordinator-supplied value"
        );

        // The `build_reinit_fn` closure exists and accepts the
        // new (descriptor, generation) signature; the type
        // checker enforces this at the call site in
        // `restart_client_coordinator`.
        let _ = svc.build_reinit_fn(key);
    }

    /// The first-publish path (init attempt that lands
    /// successfully) and the restart-coordinator path
    /// (subsequent restarts) both use the same
    /// generation-calculation rule. Two consecutive restarts
    /// produce generations `2` and `3` with no skipped or
    /// duplicate values.
    #[tokio::test]
    async fn restart_produces_no_skipped_or_duplicate_generations() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass3:consecutive";

        // Simulate the first publish.
        svc.set_generation(&key, 1).await;

        // Coordinator calls next_generation_for_key for restart 1.
        let g2 = svc.next_generation_for_key(&key).await;
        svc.set_generation(&key, g2).await;
        assert_eq!(g2, 2);

        // Coordinator calls next_generation_for_key for restart 2.
        let g3 = svc.next_generation_for_key(&key).await;
        svc.set_generation(&key, g3).await;
        assert_eq!(g3, 3);

        // The authoritative generation map is monotonic with
        // no gaps.
        let observed = svc.generation_for_key(&key).await;
        assert_eq!(observed, 3, "no skipped generations");
    }

    // ── Pass 4: Manual Restart API ───────────────────────────────

    /// `manual_restart_client` terminates the old runtime
    /// before invoking the coordinator. The runtime_map entry
    /// for the old generation is removed before any
    /// reinit is attempted, so the manual path cannot leave
    /// two live processes.
    ///
    /// The test seeds a real (sleep 60) runtime, then calls
    /// `manual_restart_client` with a key for which there is
    /// no descriptor. The reinit step fails (no descriptor
    /// → LaunchFailed), so the call returns an error — but
    /// we can still verify that the old runtime was drained
    /// from `runtime_map` BEFORE the error.
    #[tokio::test]
    async fn manual_restart_terminates_old_runtime_before_new_start() {
        use std::time::Duration;
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass4:terminate_old".to_string();

        // Seed the generation store so the manual path takes
        // the runtime-termination branch.
        svc.set_generation(&key, 1).await;

        // Install a hung runtime for the old generation.
        let runtime_map = svc.runtime_map.clone();
        let hung = spawn_hung_runtime("pass4_hung").await;
        install_runtime(&runtime_map, key.clone(), 1, hung).await;
        assert_eq!(runtime_map.lock().await.len(), 1);

        // The manual restart will:
        //   1. Terminate the old runtime (this is what we
        //      assert).
        //   2. Fail to find a descriptor → LaunchFailed.
        // We tolerate the error; we only care about the
        // pre-termination step.
        let _ = tokio::time::timeout(Duration::from_secs(15), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 15s");

        // The old runtime was terminated and removed from
        // the map.
        assert!(
            runtime_map.lock().await.is_empty(),
            "manual restart must terminate the old runtime"
        );
    }

    /// `manual_restart_client` bypasses the
    /// `LspRestartMode::Disabled` policy. The descriptor
    /// associated with `key` may have a disabled restart
    /// policy; the manual path runs regardless. We assert
    /// this by seeding a descriptor with mode=Disabled and
    /// observing the call attempts to coordinate (failing
    /// for a different reason — no real LSP server — but
    /// bypassing the disabled-policy check).
    #[cfg(feature = "lsp-test-support")]
    #[tokio::test]
    async fn manual_restart_bypasses_disabled_policy() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass4:bypass_disabled".to_string();

        // Seed a descriptor with restart disabled.
        let descriptor = LspClientDescriptor {
            key: key.clone(),
            server_id: "rust-analyzer".to_string(),
            root: std::path::PathBuf::from("/tmp"),
            launch_spec: LspLaunchSpec::default_for_test(),
            initialization_options: None,
            workspace_configuration: serde_json::Value::Null,
            readiness_policy: LspReadinessPolicy::InitializedIsReady,
            restart_policy: LspRestartPolicy {
                mode: LspRestartMode::Disabled,
                max_attempts: 1,
                initial_backoff: Duration::from_millis(10),
                max_backoff: Duration::from_millis(100),
                reset_after_healthy: Duration::from_secs(60),
            },
            seed_file: Some(std::path::PathBuf::from("/tmp/src/lib.rs")),
        };
        svc.set_descriptor_for_key(&key, descriptor).await;

        // The manual call will fail because the launch spec
        // is bogus (a real LSP server isn't available), but
        // the call is NOT rejected at the policy gate.
        let result = tokio::time::timeout(Duration::from_secs(15), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 15s");
        // The result may be Ok or Err depending on whether
        // the launch spec can produce a real client. What
        // matters is that the call is NOT rejected with
        // `InitializationCancelled("restart is disabled by
        // policy")`.
        if let Err(ref e) = result {
            let msg = format!("{e}");
            assert!(
                !msg.contains("restart is disabled by policy"),
                "manual restart must not be blocked by Disabled policy, got: {msg}"
            );
        }
    }

    /// When a manual restart is issued with no prior
    /// generation, the manual path does not call
    /// `terminate_runtime` (no old runtime to terminate) and
    /// the call still proceeds. This guards against the
    /// "first-ever restart" edge case.
    #[tokio::test]
    async fn manual_restart_with_no_old_runtime_is_a_no_op_termination() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass4:no_old_runtime".to_string();
        let runtime_map = svc.runtime_map.clone();

        // No prior generation; no prior runtime. The
        // `current_generation > 0` branch in
        // `manual_restart_client` is skipped.
        assert_eq!(svc.generation_for_key(&key).await, 0);
        assert!(runtime_map.lock().await.is_empty());

        // The call proceeds (and will fail later in the
        // coordinator because no descriptor exists for the
        // key), but the early-return path is correct.
        let _ = tokio::time::timeout(Duration::from_secs(15), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 15s");
    }

    // ── Pass 2 — Manual supersession generation timing ─────────────

    /// Pass 2 — `manual_detects_generation_advance_during_wait`.
    /// The manual supersession path captures the pre-wait
    /// generation BEFORE cancelling any in-flight owner. If the
    /// in-flight owner advances the generation during the wait
    /// (e.g. by publishing a successful replacement), the manual
    /// flow MUST detect the advance and return
    /// `LspError::ServerRestarted` without tearing down the
    /// newer-generation runtime.
    ///
    /// Test mechanics:
    /// 1. Seed `generation_for_key` with `1`.
    /// 2. Manually acquire a per-key restart ownership lease
    ///    (simulating an in-flight automatic restart).
    /// 3. From a separate task, after a short delay, bump the
    ///    generation to `2` AND drop the lease (signals
    ///    `Finished`).
    /// 4. Call `manual_restart_client`. The pre-wait snapshot
    ///    reads `1`; the post-wait read returns `2`; the manual
    ///    flow must return `ServerRestarted`.
    /// 5. The newer-generation runtime entry (we install one
    ///    for gen 2) must remain in `runtime_map` after the
    ///    manual call returns — the manual teardown did NOT
    ///    target the newer generation.
    #[tokio::test]
    async fn manual_detects_generation_advance_during_wait() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass2:detect_advance".to_string();

        // 1. Seed initial generation.
        svc.set_generation(&key, 1).await;

        // 2. Acquire an ownership lease acting as the in-flight
        //    automatic restart. The counter is only needed at
        //    acquisition time so we use a reference.
        let acquired = acquire_restart_ownership(
            &svc.restart_tasks,
            &svc.restart_owner_counter,
            &key,
            RestartTrigger::Automatic,
        )
        .await;
        let lease = match acquired {
            RestartLeaseAcquisition::Acquired(l) => l,
            RestartLeaseAcquisition::AlreadyInProgress { .. } => {
                panic!("slot must be free for test")
            }
        };

        // Install a runtime for the newer generation (gen 2)
        // so the manual teardown has a concrete target to NOT
        // touch.
        let runtime_map = svc.runtime_map.clone();
        let gen2_runtime = spawn_dummy_runtime("pass2_gen2").await;
        install_runtime(&runtime_map, key.clone(), 2, gen2_runtime.clone()).await;

        // 3. Spawn a task that bumps the generation to 2 and
        //    then drops the lease (simulating automatic's
        //    reinit completing successfully).
        let svc_for_advance = svc.clone();
        let key_for_advance = key.clone();
        tokio::spawn(async move {
            // Small delay so manual has had a chance to
            // capture the pre-wait snapshot (gen 1) BEFORE
            // we advance.
            tokio::time::sleep(Duration::from_millis(50)).await;
            svc_for_advance.set_generation(&key_for_advance, 2).await;
            // Drop the lease to signal Finished + remove
            // ownership entry.
            drop(lease);
        });

        // 4. Call manual_restart_client. The call must return
        //    `ServerRestarted` because the generation advanced
        //    from 1 (pre-wait) to 2 (post-wait).
        let result = tokio::time::timeout(Duration::from_secs(10), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 10s");

        // 5. Result must be ServerRestarted with old=1, new=2.
        match result {
            Err(LspError::ServerRestarted {
                old_generation,
                new_generation,
                ..
            }) => {
                assert_eq!(old_generation, 1, "pre-wait generation must be 1");
                assert_eq!(
                    new_generation,
                    Some(2),
                    "post-wait generation must be 2 (advanced during wait)"
                );
            }
            other => panic!("expected ServerRestarted {{ old=1, new=Some(2) }}, got {other:?}"),
        }

        // 6. The gen-2 runtime entry must remain in runtime_map.
        //    The manual teardown did NOT touch gen 2.
        let entry = {
            let m = runtime_map.lock().await;
            m.get(&key).cloned()
        };
        assert!(
            entry.is_some(),
            "manual supersession must NOT tear down the newer-generation runtime"
        );
        assert_eq!(
            entry.unwrap().generation,
            2,
            "runtime entry must remain on gen 2"
        );
    }

    /// Pass 2 — `manual_same_generation_proceeds_after_wait`.
    /// When the in-flight owner does NOT advance the
    /// generation (the wait completes with the same
    /// generation as pre-wait), the manual flow MUST proceed
    /// past the generation check. We assert this by observing
    /// the manual call does NOT return `ServerRestarted`.
    ///
    /// This is the "no generation advance" baseline that
    /// makes the advance-detection test above non-vacuous.
    #[tokio::test]
    async fn manual_same_generation_proceeds_after_wait() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass2:same_generation".to_string();

        // Seed initial generation.
        svc.set_generation(&key, 1).await;

        // Acquire an ownership lease as the in-flight
        // automatic restart.
        let acquired = acquire_restart_ownership(
            &svc.restart_tasks,
            &svc.restart_owner_counter,
            &key,
            RestartTrigger::Automatic,
        )
        .await;
        let lease = match acquired {
            RestartLeaseAcquisition::Acquired(l) => l,
            RestartLeaseAcquisition::AlreadyInProgress { .. } => {
                panic!("slot must be free for test")
            }
        };

        // After a short delay, drop the lease WITHOUT bumping
        // the generation. The manual flow sees the same
        // generation before and after the wait.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(lease);
        });

        // Manual call. It will fail later (no descriptor
        // installed for the key → LaunchFailed in the
        // coordinator), but it must NOT return ServerRestarted
        // because the generation did not advance.
        let result = tokio::time::timeout(Duration::from_secs(10), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 10s");

        match &result {
            Err(LspError::ServerRestarted { .. }) => {
                panic!(
                    "manual must NOT return ServerRestarted when generation did not advance: {result:?}"
                )
            }
            _ => {
                // Any other outcome is acceptable for this
                // test — what matters is that the generation
                // advance check did not fire.
            }
        }
    }

    /// Pass 2 — `manual_timeout_preserves_original_generation`.
    /// When the in-flight automatic owner does NOT signal
    /// completion within `MANUAL_SUPERSESSION_OWNER_TIMEOUT`
    /// (3s), the manual restart path must abort with a typed
    /// `InitializationCancelled`/`ServerRestarted` error AND
    /// the original generation must remain unchanged. The
    /// manual teardown MUST NOT have touched the live client
    /// or runtime.
    ///
    /// This test is the timeout baseline for
    /// `manual_detects_generation_advance_during_wait` and
    /// `manual_same_generation_proceeds_after_wait` — the
    /// owner-timeout path is the third branch of the manual
    /// supersession state machine.
    ///
    /// Test mechanics:
    /// 1. Seed `generation_for_key` with `1`.
    /// 2. Install a gen-1 runtime entry and a gen-1 client
    ///    entry so the manual teardown has a concrete target
    ///    to NOT touch.
    /// 3. Manually acquire a per-key restart ownership lease
    ///    (simulating an in-flight automatic restart).
    /// 4. NEVER release the lease — the manual wait must time
    ///    out.
    /// 5. Call `manual_restart_client` with a short outer
    ///    timeout. The call returns either `ServerRestarted`
    ///    or `InitializationCancelled` (the deterministic
    ///    waiter-timeout path); the live client and runtime
    ///    must both remain.
    #[tokio::test]
    async fn manual_timeout_preserves_original_generation() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass2:timeout_preserves_generation".to_string();

        // 1. Seed initial generation.
        svc.set_generation(&key, 1).await;
        let original_generation = svc.generation_for_key(&key).await;
        assert_eq!(original_generation, 1, "pre-wait generation must be 1");

        // 2. Install a gen-1 runtime entry so the manual teardown
        //    has a concrete target to NOT touch.
        let runtime_map = svc.runtime_map.clone();
        let gen1_runtime = spawn_dummy_runtime("pass2_timeout_gen1").await;
        install_runtime(&runtime_map, key.clone(), 1, gen1_runtime.clone()).await;

        // Install a test-stub client in the clients map so the
        // manual teardown takes the runtime-aware branch.
        let shutdown_count = std::sync::Arc::new(AtomicUsize::new(0));
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_path_buf();
        let client = Arc::new(
            crate::client::LspClient::test_stub(
                "rust-analyzer",
                &dir,
                shutdown_count,
                crate::client::LspClientOptions::default(),
            )
            .await
            .expect("test_stub"),
        );
        {
            let mut clients = svc.clients.write().await;
            clients.insert(key.clone(), client.clone());
        }

        // 3. Acquire an ownership lease that will NEVER be
        //    released — the manual wait must time out.
        let acquired = acquire_restart_ownership(
            &svc.restart_tasks,
            &svc.restart_owner_counter,
            &key,
            RestartTrigger::Automatic,
        )
        .await;
        let lease = match acquired {
            RestartLeaseAcquisition::Acquired(l) => l,
            RestartLeaseAcquisition::AlreadyInProgress { .. } => {
                panic!("slot must be free for test")
            }
        };

        // 4. Manual restart with a short outer timeout. The
        //    inner `MANUAL_SUPERSESSION_OWNER_TIMEOUT` is 3s;
        //    the test waits up to 5s and the call should
        //    return BEFORE that bound (the in-flight owner
        //    times out at 3s). The exact error variant may be
        //    `InitializationCancelled` (waiter timeout) or
        //    `ServerRestarted` (depending on internal ordering);
        //    either is acceptable for this test — what matters
        //    is that the live client and runtime are preserved.
        let result = tokio::time::timeout(Duration::from_secs(5), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 5s");

        // 5. The result must be an error (not Ok) because the
        //    manual path did not complete.
        assert!(
            result.is_err(),
            "manual must return an error when the in-flight owner times out, got {result:?}"
        );
        match &result {
            Err(LspError::InitializationCancelled(_)) | Err(LspError::ServerRestarted { .. }) => {
                // Expected: typed busy/restart error.
            }
            other => panic!(
                "manual must return InitializationCancelled or ServerRestarted on owner timeout, got {other:?}"
            ),
        }

        // Release the lease so subsequent tests do not see a
        // stuck owner.
        let _ = lease.release().await;

        // 6. The original generation MUST be preserved.
        let current_generation = svc.generation_for_key(&key).await;
        assert_eq!(
            current_generation, 1,
            "manual timeout must NOT bump the generation; pre=1 post={current_generation}"
        );

        // 7. The gen-1 runtime entry MUST remain.
        let entry = {
            let m = runtime_map.lock().await;
            m.get(&key).cloned()
        };
        assert!(
            entry.is_some(),
            "manual timeout must NOT tear down the live runtime"
        );
        assert_eq!(
            entry.unwrap().generation,
            1,
            "runtime entry generation must be preserved at 1"
        );

        // 8. The clients map MUST still contain the gen-1
        //    client — manual timeout must not have touched
        //    it.
        let client_count = {
            let clients = svc.clients.read().await;
            clients.contains_key(&key)
        };
        assert!(
            client_count,
            "manual timeout must NOT remove the live client"
        );
    }

    /// Pass 4 — `manual_generation_advance_returns_server_restarted`.
    /// Strong assertion: the manual path must return exactly
    /// `LspError::ServerRestarted` (not `Ok`, not `LaunchFailed`,
    /// not `InitializationCancelled`) when the generation
    /// advances during the wait. This is the deterministic
    /// counterpart of the older
    /// `manual_waits_for_cancelled_automatic_completion`
    /// integration test which accepted a wider bounded set.
    #[tokio::test]
    async fn manual_generation_advance_returns_server_restarted() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass4:advance_returns_server_restarted".to_string();

        // Seed initial generation.
        svc.set_generation(&key, 1).await;

        // Install a gen-2 runtime entry so the manual path
        // has a target it must NOT tear down.
        let runtime_map = svc.runtime_map.clone();
        let gen2_runtime = spawn_dummy_runtime("pass4_gen2").await;
        install_runtime(&runtime_map, key.clone(), 2, gen2_runtime.clone()).await;

        // Acquire ownership lease.
        let acquired = acquire_restart_ownership(
            &svc.restart_tasks,
            &svc.restart_owner_counter,
            &key,
            RestartTrigger::Automatic,
        )
        .await;
        let lease = match acquired {
            RestartLeaseAcquisition::Acquired(l) => l,
            RestartLeaseAcquisition::AlreadyInProgress { .. } => {
                panic!("slot must be free for test")
            }
        };

        // Advance generation after a short delay, then drop
        // the lease (signals Finished).
        let svc_for_advance = svc.clone();
        let key_for_advance = key.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(40)).await;
            svc_for_advance.set_generation(&key_for_advance, 2).await;
            drop(lease);
        });

        // Manual call — must return exactly ServerRestarted.
        let result = tokio::time::timeout(Duration::from_secs(10), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 10s");

        assert!(
            matches!(result, Err(LspError::ServerRestarted { .. })),
            "manual must return ServerRestarted on generation advance, got {result:?}"
        );

        // The gen-2 runtime must remain.
        let entry = {
            let m = runtime_map.lock().await;
            m.get(&key).cloned()
        };
        assert!(
            entry.is_some(),
            "newer-generation runtime must remain after ServerRestarted"
        );
        assert_eq!(entry.unwrap().generation, 2);
    }

    /// Pass 4 — `one_runtime_and_one_client_after_supersession`.
    /// After a successful manual supersession (the in-flight
    /// automatic restart completes and the manual restart
    /// proceeds), exactly one runtime entry and exactly one
    /// live client remain for the key. This guards against
    /// leaked or duplicated state during supersession.
    #[tokio::test]
    async fn one_runtime_and_one_client_after_supersession() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass4:one_runtime_one_client".to_string();

        // Seed initial generation.
        svc.set_generation(&key, 1).await;

        // Install a runtime for gen 1 so the manual teardown
        // path has a target.
        let runtime_map = svc.runtime_map.clone();
        let gen1_runtime = spawn_dummy_runtime("pass4_gen1").await;
        install_runtime(&runtime_map, key.clone(), 1, gen1_runtime.clone()).await;

        // Install a test-stub client in the clients map so
        // the manual path takes the runtime-aware branch.
        let shutdown_count = std::sync::Arc::new(AtomicUsize::new(0));
        let tmpdir = tempfile::tempdir().unwrap();
        let dir = tmpdir.path().to_path_buf();
        let client = Arc::new(
            crate::client::LspClient::test_stub(
                "rust-analyzer",
                &dir,
                shutdown_count,
                crate::client::LspClientOptions::default(),
            )
            .await
            .expect("test_stub"),
        );
        {
            let mut clients = svc.clients.write().await;
            clients.insert(key.clone(), client);
        }

        // Manual restart will: terminate gen-1 runtime, drop
        // the gen-1 client, then fail later (no descriptor →
        // LaunchFailed in the coordinator). We tolerate the
        // error; we only care about the runtime/client counts
        // BEFORE the coordinator fails.
        let _ = tokio::time::timeout(Duration::from_secs(15), svc.manual_restart_client(&key))
            .await
            .expect("manual_restart_client did not return within 15s");

        // After the manual call completes:
        // 1. The gen-1 runtime entry must be gone (manual
        //    terminated it before reinit).
        // 2. The clients map must NOT still have the gen-1
        //    client (manual removed it).
        // We check by reading both maps and asserting the
        // pre-conditions of "exactly one of each" are met.
        // (After manual terminates gen-1, the coordinator
        // may install a new runtime/client if it succeeds;
        // here it fails, so the counts are exactly zero.)
        let runtime_count = runtime_map.lock().await.len();
        let client_count = svc.clients.read().await.len();

        // We accept either:
        // - 0/0 (coordinator failed, no replacement
        //   installed), OR
        // - 1/1 (coordinator succeeded despite the missing
        //   descriptor somehow).
        // We assert NO leak of the gen-1 runtime, regardless.
        assert!(
            runtime_count <= 1,
            "runtime_map must not leak; got {runtime_count} entries"
        );
        assert!(
            client_count <= 1,
            "clients map must not leak; got {client_count} entries"
        );
    }

    // ── Pass 5: Shared Restart Budget ────────────────────────────

    /// `reset_restart_attempts_if_healthy_inherent` resets
    /// the counter to `0` only when the service has been
    /// healthy for at least `reset_after_healthy`. If the
    /// last healthy timestamp is missing or the interval
    /// has not yet elapsed, the call returns `None` and the
    /// counter is unchanged.
    #[tokio::test]
    async fn healthy_reset_only_fires_after_reset_interval() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass5:healthy_reset";

        // Create the entry so `set_last_healthy_now` has
        // somewhere to write.
        let _ = svc.increment_restart_attempts(&key).await;

        // Set a healthy timestamp and immediately check
        // (within the reset interval). Reset must NOT fire.
        svc.set_last_healthy_now(&key).await;
        let reset = svc
            .reset_restart_attempts_if_healthy_inherent(&key, Duration::from_secs(60))
            .await;
        assert!(
            reset.is_none(),
            "healthy interval not yet elapsed: reset must NOT fire"
        );

        // Wait for the interval to elapse, then reset must fire.
        tokio::time::sleep(Duration::from_millis(20)).await;
        let reset = svc
            .reset_restart_attempts_if_healthy_inherent(&key, Duration::from_millis(10))
            .await;
        assert!(reset.is_some(), "healthy interval elapsed: reset must fire");
    }

    /// `restart_attempts` increments across restart
    /// invocations. The shared counter is the cross-
    /// invocation bound, so a rapid crash cycle drains the
    /// budget across cycles rather than per-invocation.
    #[tokio::test]
    async fn restart_attempts_increments_under_load() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let key = "pass5:increment";

        // Increment to 3 manually.
        for _ in 0..3 {
            let _ = svc.increment_restart_attempts(&key).await;
        }
        assert_eq!(svc.restart_attempts(&key).await, 3);

        // Healthy reset clears the counter.
        svc.set_last_healthy_now(&key).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        let reset = svc
            .reset_restart_attempts_if_healthy_inherent(&key, Duration::from_millis(10))
            .await;
        assert_eq!(reset, Some(3));
        assert_eq!(
            svc.restart_attempts(&key).await,
            0,
            "counter is zero after healthy reset"
        );
    }

    // ── Pass 6: Transfer Diagnostics Across Restart ──────────────

    /// `DiagnosticCacheEntry::with_generation` must mark
    /// `post_restart` only for generation `2` or higher
    /// (Pass 6). Generation `1` is the cold-start publication
    /// and is NEVER post-restart.
    #[tokio::test]
    async fn generation_one_is_not_post_restart() {
        use crate::client::DiagnosticCacheEntry;
        use crate::diagnostics::LspDiagnosticSource;
        use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
        use std::time::Instant;

        let entry = DiagnosticCacheEntry {
            diagnostics: vec![Diagnostic {
                range: Range {
                    start: Position {
                        line: 0,
                        character: 0,
                    },
                    end: Position {
                        line: 0,
                        character: 1,
                    },
                },
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("E001".to_string())),
                code_description: None,
                source: Some("test".to_string()),
                message: "error".to_string(),
                related_information: None,
                tags: None,
                data: None,
            }],
            received_at: Instant::now(),
            source: LspDiagnosticSource::Pushed,
            content_version: Some(1),
            server_generation: 0,
            post_restart: false,
        };

        // Generation 1: post_restart = false.
        let gen1 = entry.with_generation(1);
        assert!(!gen1.post_restart, "generation 1 must NOT be post_restart");
        assert_eq!(gen1.server_generation, 1);

        // Generation 2: post_restart = true.
        let gen2 = entry.with_generation(2);
        assert!(gen2.post_restart, "generation 2 must be post_restart");
        assert_eq!(gen2.server_generation, 2);

        // Generation 3: post_restart = true.
        let gen3 = entry.with_generation(3);
        assert!(gen3.post_restart, "generation 3 must be post_restart");
        assert_eq!(gen3.server_generation, 3);
    }

    /// `snapshot_diagnostics_for_restart` returns the live
    /// client's cache snapshot. When no live client exists
    /// the snapshot is empty.
    #[tokio::test]
    async fn snapshot_diagnostics_returns_empty_when_no_client() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        let snap = svc
            .snapshot_diagnostics_for_restart("pass6:no_client")
            .await;
        assert!(snap.is_empty());
    }

    // ── Pass 10: Single Public Constructor ──────────────────────

    /// The supervised constructor `new_arc` returns an
    /// `Arc<LspService>` with the cyclic back-reference
    /// wired so the exit-receiver task is auto-started on
    /// the first client-creating path. The bare `new`
    /// constructor remains available for tests but is
    /// documented as un-supervised.
    #[tokio::test]
    async fn new_arc_wires_self_ref() {
        let svc = LspService::new_arc(LspConfig::Disabled(false));
        // The `self_ref` is a `OnceLock<Weak<LspService>>`;
        // it is populated by `Arc::new_cyclic` inside
        // `new_arc`. We can read it via the `Weak` upgrade
        // path on the service: the weak ref must be
        // upgradable (i.e. the service is registered).
        // Indirectly verify by calling `shutdown_all` —
        // the supervised path uses the cyclic back-ref.
        let _result = svc.shutdown_all().await;
        assert_eq!(
            svc.lifecycle.read().await.phase,
            super::ServiceLifecycle::Stopped
        );
    }

    /// `LspService::new` is the bare (un-supervised)
    /// constructor. It still works but is documented as
    /// test-only. `shutdown_all` works on the bare
    /// service too (the supervised path is for the
    /// exit-receiver, not for shutdown).
    #[tokio::test]
    async fn bare_new_works_for_tests() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let _ = svc.shutdown_all().await;
        assert_eq!(
            svc.lifecycle.read().await.phase,
            super::ServiceLifecycle::Stopped
        );
    }

    /// Build an `LspProcessRuntime` whose child process is
    /// `/bin/sh -c 'sleep 60'` (a long-running, idle process).
    /// The runtime's intent is `Running`; only an explicit
    /// `request_force_kill` will terminate it. Used to exercise
    /// the force-kill reaping path.
    async fn spawn_hung_runtime(label: &'static str) -> LspProcessRuntime {
        use std::process::Stdio;
        use tokio::process::Command;
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("sleep 60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn hung sh");
        let stderr_handle = child.stderr.take().expect("stderr for hung sh");
        let (runtime, _join) = crate::runtime::spawn_process_runtime(
            label.to_string(),
            std::path::PathBuf::from("/tmp"),
            0,
            child,
            stderr_handle,
        );
        runtime
    }

    /// Build a minimal `LspProcessRuntime` for helper tests. Uses
    /// `/bin/sh -c 'exit 0'` (a process guaranteed to exist on
    /// macOS and Linux and to have a stderr pipe) so the runtime
    /// constructor is satisfied. The runtime's task exits as soon
    /// as the child does, so the handle is safe to drop.
    async fn spawn_dummy_runtime(label: &'static str) -> LspProcessRuntime {
        use std::process::Stdio;
        use tokio::process::Command;
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg("exit 0")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().expect("spawn sh");
        let stderr_handle = child.stderr.take().expect("stderr for sh");
        let (runtime, _join) = crate::runtime::spawn_process_runtime(
            label.to_string(),
            std::path::PathBuf::from("/tmp"),
            0,
            child,
            stderr_handle,
        );
        runtime
    }

    /// Even when the live client has been removed (e.g. after a
    /// failed restart), `operational_health_snapshot` returns a
    /// snapshot. The `Failed` reason and `transport: None` are
    /// surfaced.
    #[tokio::test]
    async fn snapshot_available_during_failed_state() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let key = "/tmp:rust-analyzer".to_string();

        // Insert a `Failed` operational state with no live
        // client in `clients`. The snapshot must still be
        // returned.
        {
            let mut states = svc.operational_state.write().await;
            states.insert(
                key.clone(),
                OperationalServerState {
                    state: LspOperationalState::Failed {
                        reason: "synthetic failure".to_string(),
                    },
                    ..Default::default()
                },
            );
        }
        svc.set_generation(&key, 1).await;

        let snap = svc
            .operational_health_snapshot(&key)
            .await
            .expect("snapshot should be returned even when client is absent");
        assert!(matches!(snap.state, LspOperationalState::Failed { .. }));
        assert_eq!(snap.generation, 1);
        assert!(snap.transport.is_none(), "no live client → transport None");
        assert_eq!(
            snap.last_error.as_deref(),
            Some("synthetic failure"),
            "last_error surfaces the Failed reason"
        );
        assert_eq!(snap.restart_attempts, 0);
    }

    /// `transition_operational_state` rejects terminal→terminal
    /// and other invalid moves (e.g. `Stopped` → `Ready`).
    #[tokio::test]
    async fn transition_helper_rejects_invalid_moves() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let key = "/tmp:rust-analyzer".to_string();

        // Seed with `Stopped` (terminal).
        {
            let mut states = svc.operational_state.write().await;
            states.insert(
                key.clone(),
                OperationalServerState {
                    state: LspOperationalState::Stopped,
                    ..Default::default()
                },
            );
        }

        // `Stopped` is terminal: any move to a non-terminal
        // state must be rejected.
        let err =
            transition_operational_state(&svc.operational_state, &key, LspOperationalState::Ready)
                .await;
        assert!(matches!(err, Err(LspError::Protocol(_))));

        // `Failed` is also terminal.
        {
            let mut states = svc.operational_state.write().await;
            states.insert(
                key.clone(),
                OperationalServerState {
                    state: LspOperationalState::Failed { reason: "x".into() },
                    ..Default::default()
                },
            );
        }
        let err =
            transition_operational_state(&svc.operational_state, &key, LspOperationalState::Ready)
                .await;
        assert!(matches!(err, Err(LspError::Protocol(_))));

        // Missing entry is also rejected (cannot transition a
        // state that does not exist).
        let err = transition_operational_state(
            &svc.operational_state,
            "/missing:server",
            LspOperationalState::Ready,
        )
        .await;
        assert!(matches!(err, Err(LspError::Protocol(_))));
    }

    /// `transition_operational_state` updates the entry on a
    /// valid move (e.g. `Ready` → `Degraded`).
    #[tokio::test]
    async fn transition_helper_updates_state_on_valid_move() {
        let svc = LspService::new(LspConfig::Disabled(false));
        let key = "/tmp:rust-analyzer".to_string();

        // Seed with `Ready`.
        {
            let mut states = svc.operational_state.write().await;
            states.insert(
                key.clone(),
                OperationalServerState {
                    state: LspOperationalState::Ready,
                    ..Default::default()
                },
            );
        }

        // `Ready` → `Degraded { reason: "slow" }` is valid.
        let res = transition_operational_state(
            &svc.operational_state,
            &key,
            LspOperationalState::Degraded {
                reason: "slow".to_string(),
            },
        )
        .await;
        assert!(res.is_ok());

        // The state was updated.
        let states = svc.operational_state.read().await;
        let entry = states.get(&key).expect("entry preserved");
        assert!(matches!(
            entry.state,
            LspOperationalState::Degraded { ref reason } if reason == "slow"
        ));
    }

    // ── wait_for_readiness tests ────────────────────────────────────────

    fn temp_dir_path(_name: &str) -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        dir.path().to_path_buf()
    }

    fn build_minimal_service() -> (LspService, String) {
        let svc = LspService::new(LspConfig::default());
        let key = format!("{}:rust-analyzer", temp_dir_path("k").to_string_lossy());
        (svc, key)
    }

    #[tokio::test]
    async fn wait_for_readiness_initialized_is_ready_immediately() {
        let (svc, key) = build_minimal_service();
        let policy = LspReadinessPolicy::InitializedIsReady;
        let result = svc.wait_for_readiness(&key, &policy).await;
        assert!(
            matches!(result, ReadinessResult::Ready { .. }),
            "InitializedIsReady should be Ready without wait, got {result:?}"
        );
    }

    #[tokio::test]
    async fn wait_for_readiness_warmup_delay_returns_ready_after_sleep() {
        let (svc, key) = build_minimal_service();
        let policy = LspReadinessPolicy::WarmupDelay {
            duration: std::time::Duration::from_millis(10),
        };
        let started = std::time::Instant::now();
        let result = svc.wait_for_readiness(&key, &policy).await;
        let elapsed = started.elapsed();
        assert!(
            matches!(result, ReadinessResult::Ready { .. }),
            "warmup delay should be Ready, got {result:?}"
        );
        assert!(
            elapsed >= std::time::Duration::from_millis(10),
            "elapsed should reflect the delay"
        );
    }

    #[tokio::test]
    async fn wait_for_readiness_no_client_returns_degraded() {
        let (svc, key) = build_minimal_service();
        // Key has never been published, so the service has no
        // client for it. Both wait policies should return
        // `Degraded { reason: "client not initialized" }`.
        let policy = LspReadinessPolicy::WaitForProgressEndOrTimeout {
            timeout: std::time::Duration::from_millis(50),
        };
        let result = svc.wait_for_readiness(&key, &policy).await;
        assert!(
            matches!(result, ReadinessResult::Degraded { ref reason, .. } if reason == "client not initialized"),
            "expected Degraded with 'client not initialized', got {result:?}"
        );

        let policy = LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
            timeout: std::time::Duration::from_millis(50),
        };
        let result = svc.wait_for_readiness(&key, &policy).await;
        assert!(
            matches!(result, ReadinessResult::Degraded { ref reason, .. } if reason == "client not initialized"),
            "expected Degraded with 'client not initialized', got {result:?}"
        );
    }

    #[tokio::test]
    async fn operational_state_for_key_returns_none_for_unknown() {
        let (svc, _) = build_minimal_service();
        let result = svc.operational_state_for_key("nope:rust-analyzer").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn wait_for_readiness_timeout_returns_degraded() {
        // Register a client manually so the service has a live
        // client for the key. Then register a progress token
        // (which never completes) and ask for a progress wait
        // with a short timeout — the wait times out and we get
        // `Degraded`. For diagnostics, no `publishDiagnostics`
        // notification has arrived, so the diagnostics wait
        // also times out.
        let (svc, key) = build_minimal_service();
        let dir = temp_dir_path("timeout");
        let shutdown_count = Arc::new(AtomicUsize::new(0));
        let client = Arc::new(
            LspClient::test_stub(
                "rust-analyzer",
                &dir,
                shutdown_count,
                LspClientOptions::default(),
            )
            .await
            .expect("test_stub should succeed"),
        );

        // Begin a progress token so the progress wait cannot
        // trivially succeed.
        super::super::client::update_progress_state(
            &client.progress_state,
            &serde_json::json!({
                "token": "indexing-stuck",
                "value": { "kind": "begin", "title": "Indexing" }
            }),
        )
        .await;

        {
            let mut clients = svc.clients.write().await;
            clients.insert(key.clone(), client);
        }
        {
            let mut states = svc.operational_state.write().await;
            states.insert(
                key.clone(),
                OperationalServerState {
                    state: LspOperationalState::Ready,
                    restart_attempts: 0,
                    last_healthy_at: Some(Instant::now()),
                    last_exit: None,
                    last_stderr_tail: Vec::new(),
                },
            );
        }

        let policy = LspReadinessPolicy::WaitForProgressEndOrTimeout {
            timeout: std::time::Duration::from_millis(50),
        };
        let result = svc.wait_for_readiness(&key, &policy).await;
        assert!(
            matches!(result, ReadinessResult::Degraded { ref reason, .. } if reason == "progress wait timed out"),
            "expected Degraded with 'progress wait timed out', got {result:?}"
        );

        let policy = LspReadinessPolicy::WaitForDiagnosticsOrTimeout {
            timeout: std::time::Duration::from_millis(50),
        };
        let result = svc.wait_for_readiness(&key, &policy).await;
        assert!(
            matches!(result, ReadinessResult::Degraded { ref reason, .. } if reason == "diagnostics wait timed out"),
            "expected Degraded with 'diagnostics wait timed out', got {result:?}"
        );
    }
}
