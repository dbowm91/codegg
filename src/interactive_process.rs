//! Scheduler-owned PTY engine for local interactive processes (M001).
//!
//! This module owns the daemon/execution-node interactive-process service.
//! It is the single production owner of PTY-backed child lifecycle:
//! admission-checked spawn under an immutable [`ExecutionContext`], bounded
//! sequence-numbered scrollback, input/resize/termination, process-group
//! cleanup, and daemon-shutdown cancellation.
//!
//! ```text
//! Arc<ExecutionContext> + SpawnSpec
//!   -> workspace/cwd policy (ExecutionContext::resolve_relative_cwd)
//!   -> AdmissionController::try_admit_arc (no spawn before permit)
//!   -> openpty + tokio::process::Command with slave stdio + setsid/ctty
//!   -> LiveSession { AsyncFd master, ring, permit guard, waiter/reader }
//!   -> write / resize / read_from(seq) / terminate (TERM -> KILL) / shutdown
//! ```
//!
//! # Ownership and non-goals
//!
//! - PTY lifecycle is distinct from the human `Session` (conversation) and
//!   from scheduler `Job`/`Run` identities. Handles are ephemeral UUIDs.
//! - No model-facing tool is registered here. The deferred `terminal` tool
//!   keeps its existing one-shot managed-process behavior until M003.
//! - No durable job record is created: interactive PTYs are ephemeral and
//!   do not survive daemon restart. Restart reports prior handles gone.
//! - This is not a second general process supervisor: finite argv work
//!   stays with `ManagedProcessService`. The only direct-spawn site here
//!   is the PTY slave-stdio child, admitted by the scheduler permit
//!   contract below.
//!
//! # Scheduler admission contract
//!
//! Every spawn acquires a [`ResourcePermitGuard`] via
//! [`AdmissionController::try_admit_arc`] with the dimensions returned by
//! [`permit_dimensions_for_interactive`] (the `ManagedProcess` resource
//! class: one process slot) *before* any PTY or child is created. Spawn
//! failure releases the permit and no handle becomes live. Contention
//! queues/refuses per existing admission policy rather than spawning
//! anyway. The guard is held inside the live session and dropped on
//! natural exit, terminate, or shutdown, releasing capacity.
//!
//! # Platform support
//!
//! Supported: Unix hosts where `libc::openpty` exists (Linux, macOS).
//! Unsupported: non-Unix targets, where [`InteractiveProcessService::spawn`]
//! returns [`InteractiveError::PlatformUnsupported`] without spawning.
//! The implementation uses only `libc` (already a direct dependency) plus
//! `tokio`; no multiplexer or session framework is imported, per the
//! milestone handoff notes.
//!
//! # Bounds
//!
//! - Scrollback: [`DEFAULT_SCROLLBACK_BYTES`] retained per session
//!   (configurable within [`MIN_SCROLLBACK_BYTES`]..=[`MAX_SCROLLBACK_BYTES`]).
//!   Older bytes are dropped; [`SequenceRing`] tracks the gap so M002 can
//!   answer resync deterministically.
//! - Input: each [`InteractiveProcessService::write_input`] call is capped at
//!   [`MAX_INPUT_WRITE_BYTES`].
//! - PTY size: 1..=[`MAX_PTY_DIMENSION`] columns/rows.
//! - Environment overrides: at most [`MAX_ENV_OVERRIDES`] entries, each name
//!   and value bounded; denied command-bearing variables can never be
//!   reintroduced (the canonical sanitized policy owns the deny list).
//! - Termination: SIGTERM to the child process group, escalating to SIGKILL
//!   after [`TERMINATE_GRACE`]. Shutdown uses [`SHUTDOWN_GRACE`] per session.

use std::collections::VecDeque;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codegg_core::jobs::{JobKind, ResourceRequest};
use codegg_core::workspace::{ExecutionContext, WorkspaceId};
use dashmap::DashMap;
use thiserror::Error;
#[cfg(unix)]
use tokio::io::unix::AsyncFd;
use tokio::sync::{Mutex, Notify, RwLock};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use std::os::fd::{AsRawFd, OwnedFd};

use crate::scheduler::admission::{AdmissionController, AdmissionDecision};
use crate::scheduler::permit::{PermitDimensions, ResourcePermitGuard};

/// Default per-session scrollback retention.
pub const DEFAULT_SCROLLBACK_BYTES: usize = 256 * 1024;
/// Minimum configurable scrollback retention.
pub const MIN_SCROLLBACK_BYTES: usize = 4 * 1024;
/// Maximum configurable scrollback retention.
pub const MAX_SCROLLBACK_BYTES: usize = 4 * 1024 * 1024;
/// Maximum bytes accepted by a single [`InteractiveProcessService::write_input`] call.
pub const MAX_INPUT_WRITE_BYTES: usize = 32 * 1024;
/// Maximum columns or rows for [`PtySize`].
pub const MAX_PTY_DIMENSION: u16 = 1000;
/// Maximum environment override entries per spawn.
pub const MAX_ENV_OVERRIDES: usize = 64;
/// Maximum bytes per environment override name or value.
pub const MAX_ENV_ENTRY_BYTES: usize = 32 * 1024;
/// Loader-injection variables the PTY service never sets, even when
/// requested via overrides.
///
/// The canonical sanitized policy already owns the Git command-bearing deny
/// list. Interactive children additionally execute arbitrary user binaries
/// under a PTY, so dynamic-loader injection (`LD_PRELOAD`, `DYLD_*`) is
/// denied at this boundary too. This matches the deferred `terminal` tool's
/// dangerous-variable set and is documented here rather than widening the
/// Git-scoped canonical list, whose threat model is fixed-argv Git
/// execution.
pub const PTY_HARD_DENIED_ENV_VARS: &[&str] = &[
    "LD_PRELOAD",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
    "DYLD_FRAMEWORK_PATH",
];
/// Grace between SIGTERM and SIGKILL escalation on terminate.
pub const TERMINATE_GRACE: Duration = Duration::from_secs(2);
/// Per-session grace used by [`InteractiveProcessService::shutdown`].
pub const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);
/// Cap for a single scrollback read.
pub const MAX_READ_BYTES: usize = 256 * 1024;

/// Terminal dimensions in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

impl PtySize {
    /// Validate character-cell dimensions.
    pub fn new(cols: u16, rows: u16) -> Result<Self, InteractiveError> {
        if cols == 0 || rows == 0 {
            return Err(InteractiveError::InvalidSize {
                cols,
                rows,
                reason: "columns and rows must be non-zero",
            });
        }
        if cols > MAX_PTY_DIMENSION || rows > MAX_PTY_DIMENSION {
            return Err(InteractiveError::InvalidSize {
                cols,
                rows,
                reason: "columns and rows exceed the supported maximum",
            });
        }
        Ok(Self { cols, rows })
    }
}

impl Default for PtySize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

/// Opaque ephemeral handle for one interactive process.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InteractiveHandle(String);

impl InteractiveHandle {
    fn fresh() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InteractiveHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle state of one interactive process.
///
/// ```text
/// Starting -> Running -> Terminating -> Exited
///              |                         ^
///              +-------------------------+  (natural exit)
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Starting,
    Running,
    Terminating,
    Exited,
}

impl SessionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionState::Starting => "starting",
            SessionState::Running => "running",
            SessionState::Terminating => "terminating",
            SessionState::Exited => "exited",
        }
    }

    fn is_live(&self) -> bool {
        matches!(self, SessionState::Starting | SessionState::Running)
    }
}

/// How the child stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
}

/// Bounded tail-only scrollback with a global output sequence.
///
/// `next_seq` counts every byte ever produced. `retained` holds the newest
/// `cap` bytes. The gap `next_seq - retained.len()` is the oldest retained
/// sequence, so readers can detect truncation deterministically.
#[derive(Debug)]
pub struct SequenceRing {
    cap: usize,
    retained: VecDeque<u8>,
    total_bytes: u64,
}

impl SequenceRing {
    /// Build a ring retaining the newest `cap` bytes. Only positivity and
    /// the global maximum are enforced here; spawn-time policy additionally
    /// enforces [`MIN_SCROLLBACK_BYTES`] so production sessions cannot
    /// configure degenerate retention.
    pub fn new(cap: usize) -> Result<Self, InteractiveError> {
        if cap == 0 || cap > MAX_SCROLLBACK_BYTES {
            return Err(InteractiveError::InvalidScrollback { requested: cap });
        }
        Ok(Self {
            cap,
            retained: VecDeque::with_capacity(cap.min(8192)),
            total_bytes: 0,
        })
    }

    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn push(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        // A single write larger than the cap replaces retention entirely.
        if bytes.len() >= self.cap {
            self.retained.clear();
            self.retained
                .extend(bytes[bytes.len() - self.cap..].iter().copied());
            return;
        }
        self.retained.extend(bytes.iter().copied());
        let excess = self.retained.len().saturating_sub(self.cap);
        self.retained.drain(..excess);
    }

    /// Next sequence number (total bytes produced so far).
    pub fn next_seq(&self) -> u64 {
        self.total_bytes
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    pub fn retained_bytes(&self) -> usize {
        self.retained.len()
    }

    pub fn omitted_bytes(&self) -> u64 {
        self.total_bytes.saturating_sub(self.retained.len() as u64)
    }

    pub fn is_truncated(&self) -> bool {
        self.omitted_bytes() > 0
    }

    /// Oldest retained sequence number.
    pub fn base_seq(&self) -> u64 {
        self.omitted_bytes()
    }

    /// Read up to `max_bytes` starting at `seq`.
    ///
    /// When `seq` predates retention, the oldest retained bytes are
    /// returned with `gap: true` so the caller (M002 resync) can report a
    /// typed truncation instead of silently shifting history.
    pub fn read_from(&self, seq: u64, max_bytes: usize) -> RingRead {
        let max_bytes = max_bytes.min(MAX_READ_BYTES);
        let next = self.total_bytes;
        if seq >= next {
            return RingRead {
                bytes: Vec::new(),
                next_seq: next,
                gap: false,
            };
        }
        let base = self.base_seq();
        if seq < base {
            let bytes: Vec<u8> = self.retained.iter().copied().take(max_bytes).collect();
            return RingRead {
                bytes,
                next_seq: next,
                gap: true,
            };
        }
        let offset = (seq - base) as usize;
        let bytes: Vec<u8> = self
            .retained
            .iter()
            .skip(offset)
            .copied()
            .take(max_bytes)
            .collect();
        RingRead {
            bytes,
            next_seq: next,
            gap: false,
        }
    }
}

/// Result of [`SequenceRing::read_from`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RingRead {
    pub bytes: Vec<u8>,
    pub next_seq: u64,
    /// True when `from_seq` predated retention (caller must resync).
    pub gap: bool,
}

/// Spawn parameters for one interactive process.
#[derive(Debug)]
pub struct SpawnSpec {
    /// Executable plus arguments. Must be non-empty with no interior NULs.
    /// Unlike the one-shot `terminal` tool, no `sh -c` join is performed:
    /// argv is executed directly so there is no shell-quoting boundary.
    pub argv: Vec<OsString>,
    /// Working directory relative to the workspace root (`None` = root).
    /// Absolute paths are rejected; use a relative path under the workspace.
    pub cwd: Option<PathBuf>,
    /// Extra environment variables. The canonical sanitized policy owns the
    /// deny list; denied names are silently dropped, never reintroduced.
    /// The PTY service additionally denies loader-injection variables
    /// ([`PTY_HARD_DENIED_ENV_VARS`]).
    pub env_overrides: Vec<(OsString, OsString)>,
    /// Initial PTY dimensions.
    pub size: PtySize,
    /// Scrollback retention override (`None` = default).
    pub scrollback_bytes: Option<usize>,
}

impl SpawnSpec {
    pub fn new(argv: Vec<OsString>) -> Self {
        Self {
            argv,
            cwd: None,
            env_overrides: Vec::new(),
            size: PtySize::default(),
            scrollback_bytes: None,
        }
    }

    pub fn with_cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn with_size(mut self, size: PtySize) -> Self {
        self.size = size;
        self
    }

    pub fn with_env(mut self, name: OsString, value: OsString) -> Self {
        self.env_overrides.push((name, value));
        self
    }
}

/// Point-in-time snapshot of one session (no secret material).
#[derive(Debug, Clone)]
pub struct SessionSnapshot {
    pub handle: InteractiveHandle,
    pub workspace_id: WorkspaceId,
    /// Canonical workspace root the child was spawned under.
    pub workspace_root: PathBuf,
    /// Executable name (no arguments; arguments may carry secrets).
    pub command: String,
    pub state: SessionState,
    pub size: PtySize,
    pub child_pid: Option<u32>,
    pub next_seq: u64,
    pub retained_bytes: usize,
    pub total_bytes: u64,
    pub truncated: bool,
    pub exit: Option<ExitInfo>,
    /// Time since spawn.
    pub age: Duration,
}

/// Output read plus resumption cursor.
#[derive(Debug, Clone)]
pub struct OutputRead {
    pub bytes: Vec<u8>,
    pub next_seq: u64,
    /// True when the requested sequence predated retention.
    pub gap: bool,
}

#[derive(Debug, Error)]
pub enum InteractiveError {
    #[error("interactive argv must not be empty")]
    EmptyArgv,
    #[error("interactive argv entry contains an interior NUL: {0}")]
    InvalidArgument(String),
    #[error("interactive argv exceeds the per-entry limit: {0}")]
    ArgumentTooLarge(String),
    #[error("invalid PTY size {cols}x{rows}: {reason}")]
    InvalidSize {
        cols: u16,
        rows: u16,
        reason: &'static str,
    },
    #[error("invalid scrollback request ({requested} bytes)")]
    InvalidScrollback { requested: usize },
    #[error("environment overrides exceed the per-spawn bound")]
    TooManyEnvOverrides,
    #[error("environment entry exceeds the per-entry bound")]
    EnvEntryTooLarge,
    #[error("absolute cwd is not accepted for interactive spawn; use a workspace-relative path")]
    AbsoluteCwdRejected,
    #[error("workspace policy rejected interactive spawn: {0}")]
    WorkspacePolicy(String),
    #[error("scheduler admission refused interactive spawn: {0}")]
    AdmissionBlocked(String),
    #[error("scheduler admission cannot ever satisfy interactive spawn: {0}")]
    AdmissionImpossible(String),
    #[error("interactive PTY is not supported on this platform ({platform})")]
    PlatformUnsupported { platform: &'static str },
    #[error("interactive process spawn failed: {0}")]
    SpawnFailed(String),
    #[error("unknown interactive handle")]
    UnknownHandle,
    #[error("interactive process is no longer running")]
    NotRunning,
    #[error("interactive input write exceeds the per-write bound ({len} > {bound})")]
    InputTooLarge { len: usize, bound: usize },
    #[error("interactive output read exceeds the per-read bound")]
    ReadTooLarge,
    #[error("interactive input failed: {0}")]
    WriteFailed(String),
    #[error("interactive resize failed: {0}")]
    ResizeFailed(String),
    #[error("interactive service is shutting down")]
    ShuttingDown,
    #[error("interactive termination timed out after escalation")]
    TerminateTimeout,
    #[error("interactive PTY I/O error: {0}")]
    Io(String),
}

/// Scheduler resource dimensions for one interactive PTY.
///
/// This is the documented permit contract: an interactive process reserves
/// the `ManagedProcess` resource class (one process slot, minimal
/// CPU/memory/IO weights, no exclusivity key). Interactive sessions never
/// claim `workspace-mutation` exclusivity; workspace file policy is enforced
/// separately by the execution context.
pub fn permit_dimensions_for_interactive() -> PermitDimensions {
    let request = ResourceRequest::for_kind(JobKind::ManagedProcess);
    PermitDimensions {
        cpu_weight: request.cpu_weight,
        memory_mb_hint: request.memory_mb_hint,
        process_slots: request.process_slots,
        io_weight: request.io_weight,
        network_slots: request.network_slots,
        exclusivity_keys: Vec::new(),
    }
}

/// Platform display name for closure matrices.
pub fn platform_name() -> &'static str {
    std::env::consts::OS
}

/// Whether this host can back PTY sessions.
pub fn is_supported() -> bool {
    cfg!(unix)
}

struct LiveSession {
    handle: InteractiveHandle,
    workspace_id: WorkspaceId,
    workspace_root: PathBuf,
    executable_summary: String,
    size: Mutex<PtySize>,
    state: RwLock<SessionState>,
    exit: RwLock<Option<ExitInfo>>,
    ring: Mutex<SequenceRing>,
    #[cfg(unix)]
    pty: Arc<AsyncFd<std::os::fd::OwnedFd>>,
    child: Mutex<Option<tokio::process::Child>>,
    child_pid: Option<u32>,
    permit: Mutex<Option<ResourcePermitGuard>>,
    notify_exit: Notify,
    cancel: CancellationToken,
    created_at: Instant,
}

struct ServiceInner {
    sessions: DashMap<String, Arc<LiveSession>>,
    admission: Arc<AdmissionController>,
    shutdown: CancellationToken,
    shutting_down: AtomicBool,
}

/// Daemon-owned scheduler-admitted interactive-process service.
///
/// Constructed once per execution node with the daemon's
/// [`AdmissionController`]. All spawns require an [`ExecutionContext`]
/// (immutable workspace/execution target); there is deliberately no
/// constructor that infers a working directory from process-global state.
pub struct InteractiveProcessService {
    inner: Arc<ServiceInner>,
}

impl InteractiveProcessService {
    pub fn new(admission: Arc<AdmissionController>) -> Self {
        Self {
            inner: Arc::new(ServiceInner {
                sessions: DashMap::new(),
                admission,
                shutdown: CancellationToken::new(),
                shutting_down: AtomicBool::new(false),
            }),
        }
    }

    /// Number of tracked handles (live plus retained-exited).
    pub fn session_count(&self) -> usize {
        self.inner.sessions.len()
    }

    pub fn is_shutting_down(&self) -> bool {
        self.inner.shutting_down.load(Ordering::SeqCst)
    }

    /// Spawn a PTY-backed child admitted by the scheduler.
    ///
    /// Order of checks (stable, relied on by tests):
    /// shutdown -> argv/size/env validation -> workspace/cwd policy ->
    /// platform support -> scheduler admission -> PTY/child creation.
    /// Anything before PTY/child creation leaves no handle live and holds
    /// no permit.
    pub async fn spawn(
        &self,
        ctx: &Arc<ExecutionContext>,
        spec: SpawnSpec,
    ) -> Result<InteractiveHandle, InteractiveError> {
        if self.is_shutting_down() || self.inner.shutdown.is_cancelled() {
            return Err(InteractiveError::ShuttingDown);
        }
        validate_argv(&spec.argv)?;
        validate_env_overrides(&spec.env_overrides)?;
        let scrollback = spec.scrollback_bytes.unwrap_or(DEFAULT_SCROLLBACK_BYTES);
        if !(MIN_SCROLLBACK_BYTES..=MAX_SCROLLBACK_BYTES).contains(&scrollback) {
            return Err(InteractiveError::InvalidScrollback {
                requested: scrollback,
            });
        }
        // Validate the ring bound before admission so invalid requests never
        // consume scheduler capacity.
        let _ = SequenceRing::new(scrollback)?;
        if let Some(cwd) = spec.cwd.as_ref() {
            if cwd.is_absolute() {
                return Err(InteractiveError::AbsoluteCwdRejected);
            }
        }
        let cwd = ctx
            .resolve_relative_cwd(spec.cwd.as_deref())
            .await
            .map_err(|error| InteractiveError::WorkspacePolicy(error.to_string()))?;
        if !is_supported() {
            return Err(InteractiveError::PlatformUnsupported {
                platform: platform_name(),
            });
        }

        let dimensions = permit_dimensions_for_interactive();
        let permit = match self.inner.admission.try_admit_arc(&dimensions) {
            AdmissionDecision::Admitted(guard) => guard,
            AdmissionDecision::TemporarilyBlocked(reason) => {
                return Err(InteractiveError::AdmissionBlocked(format!("{reason:?}")));
            }
            AdmissionDecision::Impossible(reason) => {
                return Err(InteractiveError::AdmissionImpossible(format!("{reason:?}")));
            }
        };

        #[cfg(not(unix))]
        {
            drop(permit);
            return Err(InteractiveError::PlatformUnsupported {
                platform: platform_name(),
            });
        }

        #[cfg(unix)]
        {
            let handle = InteractiveHandle::fresh();
            let executable_summary = spec
                .argv
                .first()
                .map(|entry| entry.to_string_lossy().into_owned())
                .unwrap_or_default();
            let spawned = match spawn_unix_child(&spec, &cwd, ctx, handle.as_str(), spec.size) {
                Ok(spawned) => spawned,
                Err(error) => {
                    drop(permit);
                    return Err(InteractiveError::SpawnFailed(error.to_string()));
                }
            };

            let ring = SequenceRing::new(scrollback).map_err(|_| {
                InteractiveError::SpawnFailed("scrollback validation raced spawn".to_string())
            })?;
            let session = Arc::new(LiveSession {
                handle: handle.clone(),
                workspace_id: ctx.workspace_id.clone(),
                workspace_root: ctx.workspace_root.clone(),
                executable_summary,
                size: Mutex::new(spec.size),
                state: RwLock::new(SessionState::Running),
                exit: RwLock::new(None),
                ring: Mutex::new(ring),
                pty: spawned.pty,
                child: Mutex::new(Some(spawned.child)),
                child_pid: spawned.pid,
                permit: Mutex::new(Some(permit)),
                notify_exit: Notify::new(),
                cancel: CancellationToken::new(),
                created_at: Instant::now(),
            });
            self.inner
                .sessions
                .insert(handle.as_str().to_string(), session.clone());
            spawn_reader_task(session.clone());
            spawn_waiter_task(session.clone());
            Ok(handle)
        }
    }

    async fn lookup(
        &self,
        handle: &InteractiveHandle,
    ) -> Result<Arc<LiveSession>, InteractiveError> {
        self.inner
            .sessions
            .get(handle.as_str())
            .map(|entry| entry.clone())
            .ok_or(InteractiveError::UnknownHandle)
    }

    /// Write bounded input bytes to the PTY master.
    pub async fn write_input(
        &self,
        handle: &InteractiveHandle,
        bytes: &[u8],
    ) -> Result<(), InteractiveError> {
        if bytes.len() > MAX_INPUT_WRITE_BYTES {
            return Err(InteractiveError::InputTooLarge {
                len: bytes.len(),
                bound: MAX_INPUT_WRITE_BYTES,
            });
        }
        let session = self.lookup(handle).await?;
        if !session.state.read().await.is_live() {
            return Err(InteractiveError::NotRunning);
        }
        #[cfg(unix)]
        {
            write_pty_bytes(&session.pty, bytes)
                .await
                .map_err(|error| InteractiveError::WriteFailed(error.to_string()))
        }
        #[cfg(not(unix))]
        {
            Err(InteractiveError::PlatformUnsupported {
                platform: platform_name(),
            })
        }
    }

    /// Forward a terminal resize (SIGWINCH + size record).
    pub async fn resize(
        &self,
        handle: &InteractiveHandle,
        size: PtySize,
    ) -> Result<(), InteractiveError> {
        let session = self.lookup(handle).await?;
        if !session.state.read().await.is_live() {
            return Err(InteractiveError::NotRunning);
        }
        #[cfg(unix)]
        {
            apply_winsize(&session.pty, size, session.child_pid)
                .map_err(|error| InteractiveError::ResizeFailed(error.to_string()))?;
        }
        #[cfg(not(unix))]
        {
            let _ = size;
            return Err(InteractiveError::PlatformUnsupported {
                platform: platform_name(),
            });
        }
        *session.size.lock().await = size;
        Ok(())
    }

    /// Read bounded scrollback from a sequence cursor.
    pub async fn read_output(
        &self,
        handle: &InteractiveHandle,
        from_seq: u64,
        max_bytes: usize,
    ) -> Result<OutputRead, InteractiveError> {
        if max_bytes == 0 || max_bytes > MAX_READ_BYTES {
            return Err(InteractiveError::ReadTooLarge);
        }
        let session = self.lookup(handle).await?;
        let ring = session.ring.lock().await;
        let read = ring.read_from(from_seq, max_bytes);
        Ok(OutputRead {
            bytes: read.bytes,
            next_seq: read.next_seq,
            gap: read.gap,
        })
    }

    /// Snapshot session metadata (no secret material, no output bytes).
    pub async fn snapshot(
        &self,
        handle: &InteractiveHandle,
    ) -> Result<SessionSnapshot, InteractiveError> {
        let session = self.lookup(handle).await?;
        let state = *session.state.read().await;
        let exit = *session.exit.read().await;
        let size = *session.size.lock().await;
        let ring = session.ring.lock().await;
        let child_pid = match state {
            SessionState::Exited => None,
            _ => session.child_pid,
        };
        Ok(SessionSnapshot {
            handle: session.handle.clone(),
            workspace_id: session.workspace_id.clone(),
            workspace_root: session.workspace_root.clone(),
            command: session.executable_summary.clone(),
            state,
            size,
            child_pid,
            next_seq: ring.next_seq(),
            retained_bytes: ring.retained_bytes(),
            total_bytes: ring.total_bytes(),
            truncated: ring.is_truncated(),
            exit,
            age: session.created_at.elapsed(),
        })
    }

    /// Gracefully terminate a session (SIGTERM, escalate to SIGKILL).
    ///
    /// The child process group is always signalled so descendants die with
    /// the leader. The scheduler permit is released and scrollback is
    /// retained for post-mortem reads until [`Self::remove`].
    pub async fn terminate(
        &self,
        handle: &InteractiveHandle,
    ) -> Result<Option<ExitInfo>, InteractiveError> {
        self.terminate_with_grace(handle, TERMINATE_GRACE).await
    }

    pub async fn terminate_with_grace(
        &self,
        handle: &InteractiveHandle,
        grace: Duration,
    ) -> Result<Option<ExitInfo>, InteractiveError> {
        #[cfg(not(unix))]
        {
            let _ = self.lookup(handle).await?;
            let _ = grace;
            return Err(InteractiveError::PlatformUnsupported {
                platform: platform_name(),
            });
        }
        #[cfg(unix)]
        {
            let session = self.lookup(handle).await?;
            {
                let state = session.state.read().await;
                if matches!(*state, SessionState::Exited) {
                    let exit = *session.exit.read().await;
                    return Ok(exit);
                }
                if !matches!(*state, SessionState::Terminating) {
                    drop(state);
                    *session.state.write().await = SessionState::Terminating;
                }
            }
            signal_process_group(session.child_pid, libc::SIGTERM);
            let exited = tokio::time::timeout(grace, session.notify_exit.notified()).await;
            if exited.is_err() {
                signal_process_group(session.child_pid, libc::SIGKILL);
            }
            // Bounded join for the reaper: escalation guarantees exit on a
            // supported host; never wait unboundedly for a wedged child.
            let _ = tokio::time::timeout(SHUTDOWN_GRACE, session.notify_exit.notified()).await;
            finish_session(&session).await;
            let exit = *session.exit.read().await;
            Ok(exit)
        }
    }

    /// Drop a terminal handle, freeing scrollback memory.
    ///
    /// Live sessions are terminated first (bounded escalation); already
    /// exited sessions are removed immediately. Unknown handles error.
    pub async fn remove(&self, handle: &InteractiveHandle) -> Result<(), InteractiveError> {
        let session = self.lookup(handle).await?;
        if session.state.read().await.is_live() {
            let _ = self.terminate(handle).await;
        }
        session.cancel.cancel();
        self.inner.sessions.remove(handle.as_str());
        Ok(())
    }

    /// Cancel all sessions then join their cleanup (bounded per session).
    pub async fn shutdown(&self) {
        if self
            .inner
            .shutting_down
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return;
        }
        self.inner.shutdown.cancel();
        let handles: Vec<String> = self
            .inner
            .sessions
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        for key in handles {
            if let Some(entry) = self.inner.sessions.get(&key) {
                let session = entry.clone();
                drop(entry);
                session.cancel.cancel();
                #[cfg(unix)]
                signal_process_group(session.child_pid, libc::SIGTERM);
                let _ = tokio::time::timeout(TERMINATE_GRACE, session.notify_exit.notified()).await;
                #[cfg(unix)]
                if !matches!(*session.state.read().await, SessionState::Exited) {
                    signal_process_group(session.child_pid, libc::SIGKILL);
                }
                let _ = tokio::time::timeout(SHUTDOWN_GRACE, session.notify_exit.notified()).await;
                finish_session(&session).await;
            }
        }
    }
}

impl Drop for InteractiveProcessService {
    fn drop(&mut self) {
        self.inner.shutdown.cancel();
        // Best-effort group cleanup for any session still tracked. The async
        // reaper cannot be joined from Drop; SIGKILL is immediate and the
        // OS reaps the orphaned group.
        #[cfg(unix)]
        for entry in self.inner.sessions.iter() {
            signal_process_group(entry.child_pid, libc::SIGKILL);
        }
    }
}

fn validate_argv(argv: &[OsString]) -> Result<(), InteractiveError> {
    let Some(first) = argv.first() else {
        return Err(InteractiveError::EmptyArgv);
    };
    if first.is_empty() {
        return Err(InteractiveError::EmptyArgv);
    }
    for (index, entry) in argv.iter().enumerate() {
        if entry.to_string_lossy().contains('\0') {
            return Err(InteractiveError::InvalidArgument(format!(
                "argument {index}"
            )));
        }
        if entry.len() > MAX_ENV_ENTRY_BYTES {
            return Err(InteractiveError::ArgumentTooLarge(format!(
                "argument {index}"
            )));
        }
    }
    Ok(())
}

fn validate_env_overrides(overrides: &[(OsString, OsString)]) -> Result<(), InteractiveError> {
    if overrides.len() > MAX_ENV_OVERRIDES {
        return Err(InteractiveError::TooManyEnvOverrides);
    }
    for (name, value) in overrides {
        if name.len() > MAX_ENV_ENTRY_BYTES || value.len() > MAX_ENV_ENTRY_BYTES {
            return Err(InteractiveError::EnvEntryTooLarge);
        }
        if name.to_string_lossy().contains('\0') || value.to_string_lossy().contains('\0') {
            return Err(InteractiveError::EnvEntryTooLarge);
        }
    }
    Ok(())
}

async fn finish_session(session: &Arc<LiveSession>) {
    session.cancel.cancel();
    // Releasing the permit is the scheduler-resource release proof: capacity
    // returns to the admission controller exactly once per session.
    session.permit.lock().await.take();
    *session.state.write().await = SessionState::Exited;
    session.notify_exit.notify_waiters();
}

#[cfg(unix)]
fn spawn_reader_task(session: Arc<LiveSession>) {
    tokio::spawn(async move {
        let mut buffer = vec![0_u8; 8192];
        loop {
            if session.cancel.is_cancelled() {
                break;
            }
            let notified = session.pty.readable();
            tokio::pin!(notified);
            tokio::select! {
                result = &mut notified => {
                    let read = {
                        let mut guard = match result {
                            Ok(guard) => guard,
                            Err(error) => {
                                tracing::debug!("interactive PTY readability failed: {error}");
                                break;
                            }
                        };
                        match guard.try_io(|afd| {
                            // Delegated to the owned master fd; no shared
                            // mutable aliasing beyond this closure.
                            #[allow(unsafe_code)]
                            unsafe {
                                let slice = &mut buffer[..];
                                let read = libc::read(
                                    afd.as_raw_fd(),
                                    slice.as_mut_ptr().cast(),
                                    slice.len(),
                                );
                                if read < 0 {
                                    Err(io::Error::last_os_error())
                                } else {
                                    Ok(read as usize)
                                }
                            }
                        }) {
                            Ok(Ok(read)) => read,
                            Ok(Err(error))
                                if error.raw_os_error() == Some(libc::EIO) =>
                            {
                                // Slave closed (child exited) surfaces as EIO
                                // on Linux/macOS master reads: terminal output
                                // is complete, not a failure.
                                break;
                            }
                            Ok(Err(error)) => {
                                tracing::debug!("interactive PTY read failed: {error}");
                                break;
                            }
                            // Spurious readiness; the guard already cleared
                            // the tokio-side flag, so poll again.
                            Err(_) => continue,
                        }
                    };
                    if read == 0 {
                        break;
                    }
                    session.ring.lock().await.push(&buffer[..read]);
                }
                _ = session.cancel.cancelled() => break,
            }
            if matches!(*session.state.read().await, SessionState::Exited) {
                break;
            }
        }
    });
}

fn spawn_waiter_task(session: Arc<LiveSession>) {
    tokio::spawn(async move {
        let status = {
            let mut guard = session.child.lock().await;
            let Some(child) = guard.as_mut() else {
                return;
            };
            child.wait().await
        };
        let exit = match status {
            Ok(status) => {
                #[cfg(unix)]
                {
                    use std::os::unix::process::ExitStatusExt;
                    ExitInfo {
                        code: status.code(),
                        signal: status.signal(),
                    }
                }
                #[cfg(not(unix))]
                {
                    ExitInfo {
                        code: status.code(),
                        signal: None,
                    }
                }
            }
            Err(error) => {
                tracing::debug!("interactive child wait failed: {error}");
                ExitInfo {
                    code: None,
                    signal: None,
                }
            }
        };
        *session.exit.write().await = Some(exit);
        finish_session(&session).await;
    });
}

#[cfg(unix)]
async fn write_pty_bytes(pty: &Arc<AsyncFd<std::os::fd::OwnedFd>>, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    let mut written = 0;
    while written < bytes.len() {
        let mut guard = pty.writable().await?;
        match guard.try_io(|afd| {
            #[allow(unsafe_code)]
            unsafe {
                let slice = &bytes[written..];
                let result = libc::write(afd.as_raw_fd(), slice.as_ptr().cast(), slice.len());
                if result < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(result as usize)
                }
            }
        }) {
            Ok(Ok(0)) => {
                return Err(io::Error::other("interactive PTY write returned zero"));
            }
            Ok(Ok(count)) => written += count,
            Ok(Err(error)) => return Err(error),
            // Spurious readiness; the guard already cleared the tokio-side
            // flag, so poll again.
            Err(_) => continue,
        }
    }
    Ok(())
}

#[cfg(unix)]
struct SpawnedChild {
    child: tokio::process::Child,
    pty: Arc<AsyncFd<std::os::fd::OwnedFd>>,
    pid: Option<u32>,
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn open_pty_pair(size: PtySize) -> io::Result<(OwnedFd, OwnedFd)> {
    use std::os::fd::FromRawFd;

    let mut master: libc::c_int = -1;
    let mut slave: libc::c_int = -1;
    let mut winsize: libc::winsize = unsafe { std::mem::zeroed() };
    // winsize fields are platform-typed integers; character cells always fit.
    winsize.ws_col = size.cols;
    winsize.ws_row = size.rows;
    let result = unsafe {
        #[cfg(target_vendor = "apple")]
        {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut winsize,
            )
        }
        #[cfg(not(target_vendor = "apple"))]
        {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                &winsize,
            )
        }
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    let master_fd = unsafe { OwnedFd::from_raw_fd(master) };
    let slave_fd = unsafe { OwnedFd::from_raw_fd(slave) };
    set_nonblocking(&master_fd)?;
    Ok((master_fd, slave_fd))
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn set_nonblocking(fd: &OwnedFd) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 {
        return Err(io::Error::last_os_error());
    }
    let result = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn spawn_unix_child(
    spec: &SpawnSpec,
    cwd: &std::path::Path,
    ctx: &Arc<ExecutionContext>,
    handle: &str,
    size: PtySize,
) -> io::Result<SpawnedChild> {
    use std::os::unix::ffi::OsStrExt;

    let (master_fd, slave_fd) = open_pty_pair(size)?;
    let mut executable = spec.argv.iter();
    let program = executable.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "interactive argv must not be empty",
        )
    })?;

    // Canonical sanitized environment, reused rather than duplicated. Denied
    // command-bearing variables stay denied; caller overrides cannot
    // reintroduce them. Values are never logged.
    let mut policy = crate::managed_process::EnvironmentPolicy::sanitized();
    for (name, value) in &spec.env_overrides {
        // Skip NUL-bearing names defensively; validated above, but the child
        // boundary must fail closed even if validation evolves.
        if name.as_bytes().contains(&0) || value.as_bytes().contains(&0) {
            continue;
        }
        // Loader-injection hardening (see PTY_HARD_DENIED_ENV_VARS): never
        // set these, even when explicitly requested.
        if PTY_HARD_DENIED_ENV_VARS
            .iter()
            .any(|denied| name.as_bytes() == denied.as_bytes())
        {
            continue;
        }
        policy = policy.with_var(name, value);
    }

    let mut command = tokio::process::Command::new(program);
    command
        .args(executable)
        .current_dir(cwd)
        .env_clear()
        .kill_on_drop(true);
    policy.apply(&mut command);
    // Interactive terminals need a real TERM (managed non-interactive work
    // pins TERM=dumb). Workspace/handle provenance aids audit; no secrets.
    command
        .env("TERM", "xterm-256color")
        .env("CODEGG_INTERACTIVE_PROCESS", "1")
        .env("CODEGG_INTERACTIVE_HANDLE", handle)
        .env("CODEGG_WORKSPACE_ID", ctx.workspace_id.as_str());

    let stdin_slave = slave_fd.try_clone().map_err(io::Error::other)?;
    let stdout_slave = slave_fd.try_clone().map_err(io::Error::other)?;
    command
        .stdin(std::process::Stdio::from(stdin_slave))
        .stdout(std::process::Stdio::from(stdout_slave))
        .stderr(std::process::Stdio::from(slave_fd));

    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            // Attach the slave as the controlling terminal of the new
            // session so job control, SIGWINCH, and line discipline behave
            // like a real terminal.
            let result = libc::ioctl(0, libc::TIOCSCTTY as libc::c_ulong, 0);
            if result == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let pid = child.id();
    let async_fd = AsyncFd::new(master_fd).map_err(io::Error::other)?;
    // Log only shapes, never environment content.
    tracing::info!(
        workspace_id = %ctx.workspace_id,
        child_pid = ?pid,
        cols = size.cols,
        rows = size.rows,
        "interactive PTY spawned"
    );
    Ok(SpawnedChild {
        child,
        pty: Arc::new(async_fd),
        pid,
    })
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn apply_winsize(
    pty: &Arc<AsyncFd<OwnedFd>>,
    size: PtySize,
    child_pid: Option<u32>,
) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    let mut winsize: libc::winsize = unsafe { std::mem::zeroed() };
    winsize.ws_col = size.cols;
    winsize.ws_row = size.rows;
    let result = unsafe {
        libc::ioctl(
            pty.as_raw_fd(),
            libc::TIOCSWINSZ as libc::c_ulong,
            &raw const winsize,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    // Forward SIGWINCH so full-screen programs reflow promptly.
    if let Some(pid) = child_pid {
        unsafe {
            // Best effort: a vanished group is not a resize failure.
            libc::kill(-(pid as libc::pid_t), libc::SIGWINCH);
        }
    }
    Ok(())
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn signal_process_group(child_pid: Option<u32>, signal: libc::c_int) {
    let Some(pid) = child_pid else {
        return;
    };
    // The child is a session leader (setsid in pre_exec), so negative-PID
    // signaling reaches the whole interactive process group, including
    // backgrounded descendants like `sleep 60 &`.
    unsafe {
        libc::kill(-(pid as libc::pid_t), signal);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn pty_size_validation_rejects_nonpositive_and_oversized() {
        assert!(PtySize::new(80, 24).is_ok());
        assert!(matches!(
            PtySize::new(0, 24),
            Err(InteractiveError::InvalidSize { .. })
        ));
        assert!(matches!(
            PtySize::new(80, 0),
            Err(InteractiveError::InvalidSize { .. })
        ));
        assert!(matches!(
            PtySize::new(MAX_PTY_DIMENSION + 1, 24),
            Err(InteractiveError::InvalidSize { .. })
        ));
    }

    #[test]
    fn sequence_ring_is_ordered_and_bounded() {
        let mut ring = SequenceRing::new(16).expect("ring");
        ring.push(b"hello ");
        ring.push(b"world!");
        assert_eq!(ring.next_seq(), 12);
        assert_eq!(ring.retained_bytes(), 12);
        assert!(!ring.is_truncated());

        let read = ring.read_from(0, 64);
        assert_eq!(read.bytes, b"hello world!");
        assert_eq!(read.next_seq, 12);
        assert!(!read.gap);

        let read = ring.read_from(6, 64);
        assert_eq!(read.bytes, b"world!");
        assert!(!read.gap);

        // Overflow drops the oldest bytes and reports the gap.
        ring.push(b"0123456789ABCDEF");
        assert_eq!(ring.next_seq(), 28);
        assert_eq!(ring.retained_bytes(), 16);
        assert!(ring.is_truncated());
        let read = ring.read_from(0, 64);
        assert!(read.gap);
        assert_eq!(read.bytes.len(), 16);
        assert_eq!(read.next_seq, 28);

        // A single write larger than the cap keeps only its tail.
        ring.push(&[b'x'; MAX_SCROLLBACK_BYTES]);
        assert_eq!(
            ring.retained_bytes(),
            ring.capacity().min(MAX_SCROLLBACK_BYTES)
        );
    }

    #[test]
    fn ring_read_is_capped_and_cursor_stable() {
        let mut ring = SequenceRing::new(MIN_SCROLLBACK_BYTES).expect("ring");
        ring.push(b"abcdef");
        let read = ring.read_from(2, 2);
        assert_eq!(read.bytes, b"cd");
        assert_eq!(read.next_seq, 6);
        let read = ring.read_from(6, 16);
        assert!(read.bytes.is_empty());
        assert_eq!(read.next_seq, 6);
    }

    #[test]
    fn scrollback_bounds_are_enforced_before_admission() {
        assert!(SequenceRing::new(0).is_err());
        assert!(SequenceRing::new(MAX_SCROLLBACK_BYTES + 1).is_err());
        assert!(SequenceRing::new(MIN_SCROLLBACK_BYTES - 1).is_ok());
        assert!(SequenceRing::new(DEFAULT_SCROLLBACK_BYTES).is_ok());
    }

    #[test]
    fn argv_validation_rejects_empty_and_nul() {
        assert!(matches!(
            validate_argv(&[]),
            Err(InteractiveError::EmptyArgv)
        ));
        assert!(matches!(
            validate_argv(&[OsString::from("bad\0arg")]),
            Err(InteractiveError::InvalidArgument(_))
        ));
        assert!(validate_argv(&[OsString::from("sh"), OsString::from("-c")]).is_ok());
    }

    #[test]
    fn permit_contract_uses_one_process_slot_without_exclusivity() {
        let dims = permit_dimensions_for_interactive();
        assert_eq!(dims.process_slots, 1);
        assert!(dims.exclusivity_keys.is_empty());
        assert!(dims.cpu_weight > 0);
    }

    async fn test_context(root: &std::path::Path) -> Arc<ExecutionContext> {
        use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
        use codegg_core::workspace_services::{
            ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
        };

        let store = Arc::new(InMemoryWorkspaceStore::new());
        let registry = WorkspaceRegistry::load(store).await.expect("registry");
        let record = registry.get_or_register(root).await.expect("register");
        let _services = WorkspaceServiceRegistry::new(
            registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        ExecutionContext::new(
            record,
            Some("session-test".to_string()),
            CancellationToken::new(),
        )
    }

    fn test_admission(process_slots: u32) -> Arc<AdmissionController> {
        let mut config = crate::scheduler::config::ResolvedSchedulerConfig::default();
        config.resources.max_process_slots = process_slots;
        Arc::new(AdmissionController::new(config))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn empty_argv_fails_before_admission_or_spawn() {
        let dir = tempfile::tempdir().expect("workspace");
        let ctx = test_context(dir.path()).await;
        let service = InteractiveProcessService::new(test_admission(4));
        let error = service
            .spawn(&ctx, SpawnSpec::new(Vec::new()))
            .await
            .expect_err("empty argv must fail");
        assert!(matches!(error, InteractiveError::EmptyArgv));
        assert_eq!(service.session_count(), 0);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn absolute_cwd_is_rejected_without_a_permit_or_handle() {
        let dir = tempfile::tempdir().expect("workspace");
        let ctx = test_context(dir.path()).await;
        let admission = test_admission(4);
        let used_before = admission.used_process_slots();
        let service = InteractiveProcessService::new(admission.clone());
        let error = service
            .spawn(
                &ctx,
                SpawnSpec::new(vec![OsString::from("sh")]).with_cwd(PathBuf::from("/tmp")),
            )
            .await
            .expect_err("absolute cwd must fail");
        assert!(matches!(error, InteractiveError::AbsoluteCwdRejected));
        assert_eq!(service.session_count(), 0);
        assert_eq!(admission.used_process_slots(), used_before);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn workspace_escape_is_rejected_without_a_permit_or_handle() {
        let dir = tempfile::tempdir().expect("workspace");
        let ctx = test_context(dir.path()).await;
        let admission = test_admission(4);
        let used_before = admission.used_process_slots();
        let service = InteractiveProcessService::new(admission.clone());
        let error = service
            .spawn(
                &ctx,
                SpawnSpec::new(vec![OsString::from("sh")]).with_cwd(PathBuf::from("../escape")),
            )
            .await
            .expect_err("escape must fail");
        assert!(matches!(error, InteractiveError::WorkspacePolicy(_)));
        assert_eq!(service.session_count(), 0);
        assert_eq!(admission.used_process_slots(), used_before);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn oversized_input_is_rejected_before_pty_write() {
        let dir = tempfile::tempdir().expect("workspace");
        let ctx = test_context(dir.path()).await;
        let service = InteractiveProcessService::new(test_admission(4));
        let _ = ctx;
        let error = service
            .write_input(
                &InteractiveHandle("missing".to_string()),
                &vec![b'x'; MAX_INPUT_WRITE_BYTES + 1],
            )
            .await
            .expect_err("oversized input must fail");
        assert!(matches!(error, InteractiveError::InputTooLarge { .. }));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn unknown_handles_error_without_side_effects() {
        let dir = tempfile::tempdir().expect("workspace");
        let _ctx = test_context(dir.path()).await;
        let service = InteractiveProcessService::new(test_admission(4));
        let handle = InteractiveHandle("no-such-handle".to_string());
        assert!(matches!(
            service.read_output(&handle, 0, 16).await,
            Err(InteractiveError::UnknownHandle)
        ));
        assert!(matches!(
            service.snapshot(&handle).await,
            Err(InteractiveError::UnknownHandle)
        ));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn scheduler_contention_spawns_nothing_without_a_permit() {
        let dir = tempfile::tempdir().expect("workspace");
        let ctx = test_context(dir.path()).await;
        // One process slot: the first session exhausts admission.
        let service = InteractiveProcessService::new(test_admission(1));
        let first = service
            .spawn(&ctx, SpawnSpec::new(vec![OsString::from("cat")]))
            .await
            .expect("first spawn admitted");
        assert_eq!(service.session_count(), 1);
        let error = service
            .spawn(&ctx, SpawnSpec::new(vec![OsString::from("cat")]))
            .await
            .expect_err("second spawn must block");
        assert!(
            matches!(error, InteractiveError::AdmissionBlocked(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(service.session_count(), 1);
        service.shutdown().await;
        let _ = service.remove(&first).await;
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_failure_releases_the_permit() {
        let dir = tempfile::tempdir().expect("workspace");
        let ctx = test_context(dir.path()).await;
        let admission = test_admission(4);
        let service = InteractiveProcessService::new(admission.clone());
        let error = service
            .spawn(
                &ctx,
                SpawnSpec::new(vec![OsString::from("/definitely/not/a/real/binary-xyz")]),
            )
            .await
            .expect_err("missing binary must fail");
        assert!(
            matches!(error, InteractiveError::SpawnFailed(_)),
            "unexpected error: {error:?}"
        );
        assert_eq!(service.session_count(), 0);
        assert_eq!(admission.used_process_slots(), 0);
    }
}
