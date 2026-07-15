//! Single-daemon instance lifecycle and user-scoped path resolution.
//!
//! Phase 1 of the single-daemon roadmap establishes the production invariant
//! that exactly one user-scoped Codegg daemon owns execution at a time.
//!
//! This module owns:
//!
//! - [`DaemonPaths`] — centralized resolution of the per-user lock, metadata,
//!   socket, and log paths. Production daemons share one user-scoped lock;
//!   tests can override the lock root to use a temporary directory.
//! - [`DaemonInstanceMetadata`] — atomic, informational snapshot of the
//!   running daemon written once the socket is bound. Diagnostic aid only.
//! - [`DaemonInstanceGuard`] — RAII guard that holds the advisory exclusive
//!   lock for the daemon's lifetime and removes the metadata file on drop.
//! - [`CoreRuntimeMode`] — explicit classification of how a process is wired
//!   to the core (daemon-client vs standalone in-process vs stdio).
//! - [`connect_or_start_daemon`] — connect-or-start helper used by frontends
//!   that default to daemon-client mode.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::protocol::core::PROTOCOL_VERSION;

use super::CoreClient;

/// What this binary invocation is doing with respect to the core runtime.
///
/// The default for ordinary TUI startup is [`DaemonClient`](Self::DaemonClient),
/// which connects to (or starts) the user-scoped singleton daemon. Standalone
/// modes exist for tests, embedding, and explicit compatibility shims but
/// must be opted into explicitly so the singleton invariant cannot be silently
/// defeated by ad-hoc invocation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreRuntimeMode {
    /// Connect to (or auto-start) the user-scoped singleton daemon.
    #[default]
    DaemonClient,
    /// Run the core in the current process without touching the daemon
    /// singleton. Visible non-production mode; global scheduling is not
    /// available in this mode.
    StandaloneInproc,
    /// Communicate with a `core-stdio` subprocess. Compatibility/testing.
    StandaloneStdio,
}

impl CoreRuntimeMode {
    /// Returns true when this mode is the production daemon-client path.
    pub fn is_daemon_client(self) -> bool {
        matches!(self, Self::DaemonClient)
    }

    /// Returns true when this mode constructs or owns an in-process core.
    pub fn is_inproc(self) -> bool {
        matches!(self, Self::StandaloneInproc)
    }

    /// Returns true when this mode spawns a `core-stdio` subprocess.
    pub fn is_stdio(self) -> bool {
        matches!(self, Self::StandaloneStdio)
    }
}

/// Centralized resolution of all user-scoped daemon paths.
///
/// Production daemons and clients share the same paths on a given machine.
/// Tests can substitute a private root via [`DaemonPaths::with_root`] so
/// parallel test runs cannot collide.
#[derive(Clone, Debug)]
pub struct DaemonPaths {
    /// Root directory holding all daemon artifacts (lock, metadata, socket).
    pub root: PathBuf,
    /// Path of the advisory exclusive lock file.
    pub lock_path: PathBuf,
    /// Path of the metadata record (atomic-write).
    pub metadata_path: PathBuf,
    /// Path of the Unix domain socket the daemon binds.
    pub socket_path: PathBuf,
    /// Path of the daemon's debug log file (best-effort).
    pub log_path: PathBuf,
}

impl DaemonPaths {
    /// Resolve production paths for the current OS user.
    ///
    /// - macOS: `$HOME/Library/Application Support/codegg`
    /// - Linux: `${XDG_RUNTIME_DIR:-/tmp}/codegg` (and falls back to
    ///   `$HOME/.local/share/codegg` when neither is writable)
    /// - Other Unix: `/tmp/codegg`
    ///
    /// `CODEGG_DAEMON_HOME` overrides the root directory.
    pub fn resolve() -> Self {
        let override_root = std::env::var("CODEGG_DAEMON_HOME").ok().map(PathBuf::from);
        let root = override_root.unwrap_or_else(default_user_runtime_root);
        Self::with_root(root)
    }

    /// Construct paths rooted at `root`. Used by production and by tests.
    pub fn with_root(root: PathBuf) -> Self {
        let lock_path = root.join("daemon.lock");
        let metadata_path = root.join("daemon.json");
        let socket_path = root.join("core.sock");
        let log_path = root.join("daemon.log");
        Self {
            root,
            lock_path,
            metadata_path,
            socket_path,
            log_path,
        }
    }

    /// Return a copy of these paths with a different socket path. Used by
    /// the explicit `--endpoint` / `CODEGG_CORE_ENDPOINT` path so the
    /// caller can choose a non-default socket while still reusing the
    /// user-scoped lock and metadata locations.
    pub fn with_socket(&self, socket_path: PathBuf) -> Self {
        let mut out = self.clone();
        out.socket_path = socket_path;
        out
    }

    /// Ensure the root directory exists, creating it with user-only
    /// permissions where the platform permits it.
    pub fn ensure_root(&self) -> Result<(), AppError> {
        if !self.root.exists() {
            std::fs::create_dir_all(&self.root).map_err(|e| {
                AppError::Other(anyhow::anyhow!(
                    "failed to create daemon home {}: {}",
                    self.root.display(),
                    e
                ))
            })?;
            set_user_only_permissions(&self.root);
        }
        Ok(())
    }

    /// Socket endpoint suitable for `SocketCoreClient::connect`.
    pub fn endpoint_uri(&self) -> String {
        format!("unix://{}", self.socket_path.display())
    }

    /// Socket path as a plain filesystem path.
    pub fn socket_path_str(&self) -> String {
        self.socket_path.to_string_lossy().into_owned()
    }
}

/// Persisted metadata for the running daemon. Diagnostic aid only.
///
/// The advisory lock (see [`DaemonInstanceGuard`]) is authoritative; this
/// record exists so that operators and the `daemon status` command can
/// identify which process owns the lock, what socket it bound, and which
/// protocol generation it speaks. It is written atomically and may briefly
/// lag behind the live process; absence of the lock guarantees this record
/// is stale.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DaemonInstanceMetadata {
    pub daemon_id: String,
    pub generation: String,
    pub pid: u32,
    pub socket_path: PathBuf,
    pub protocol_version: u32,
    pub started_at: DateTime<Utc>,
    pub binary_version: String,
}

impl DaemonInstanceMetadata {
    /// Serialize to JSON.
    pub fn to_json(&self) -> Result<String, AppError> {
        serde_json::to_string_pretty(self).map_err(AppError::Json)
    }

    /// Parse JSON.
    pub fn from_json(raw: &str) -> Result<Self, AppError> {
        serde_json::from_str(raw).map_err(AppError::Json)
    }
}

/// RAII guard that holds the singleton lock for the daemon's lifetime.
///
/// The lock is advisory and exclusive (`flock(LOCK_EX | LOCK_NB)`); the
/// process holding this guard is the only process allowed to bind the
/// production socket and to be considered live. When the guard is dropped,
/// the metadata file is removed and the lock is released.
///
/// Note: the OS releases the underlying flock automatically when the
/// process exits, even if `drop` is not run (panic, `std::process::exit`,
/// signal). The `Drop` impl is best-effort cleanup of the metadata file.
pub struct DaemonInstanceGuard {
    /// Holds the open lock file; `flock` is released when `_file` is dropped.
    _lock_file: std::fs::File,
    /// Path of the lock file (kept for diagnostics).
    pub lock_path: PathBuf,
    /// Path of the metadata file (removed on drop when owned by us).
    pub metadata_path: PathBuf,
    /// Path of the socket the owning daemon should bind.
    pub socket_path: PathBuf,
    /// True if this process wrote the metadata file and should clean it up.
    owns_metadata: bool,
    /// Generation captured at lock acquisition time.
    pub generation: String,
}

impl std::fmt::Debug for DaemonInstanceGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonInstanceGuard")
            .field("lock_path", &self.lock_path)
            .field("metadata_path", &self.metadata_path)
            .field("socket_path", &self.socket_path)
            .field("generation", &self.generation)
            .finish()
    }
}

impl DaemonInstanceGuard {
    /// Try to acquire the singleton lock without blocking.
    ///
    /// Returns `Ok(None)` when the lock is held by another live process.
    /// Returns `Err` only on filesystem / OS errors.
    pub fn try_acquire(paths: &DaemonPaths) -> Result<Option<Self>, AppError> {
        paths.ensure_root()?;
        let lock_file = open_lock_file(&paths.lock_path)?;
        let acquired = try_flock_exclusive(&lock_file)?;
        if !acquired {
            return Ok(None);
        }
        Ok(Some(Self {
            _lock_file: lock_file,
            lock_path: paths.lock_path.clone(),
            metadata_path: paths.metadata_path.clone(),
            socket_path: paths.socket_path.clone(),
            owns_metadata: false,
            generation: uuid::Uuid::new_v4().to_string(),
        }))
    }

    /// Persist the metadata record for this daemon instance. Atomic
    /// (temp file + rename) and best-effort permission tightening.
    pub fn write_metadata(&mut self, metadata: &DaemonInstanceMetadata) -> Result<(), AppError> {
        let json = metadata.to_json()?;
        atomic_write(&self.metadata_path, json.as_bytes())?;
        set_user_only_permissions(&self.metadata_path);
        self.owns_metadata = true;
        Ok(())
    }

    /// Read metadata from disk without holding the lock. Returns `None`
    /// when no metadata file is present or when the file is unreadable.
    pub fn read_metadata(metadata_path: &Path) -> Option<DaemonInstanceMetadata> {
        let raw = std::fs::read_to_string(metadata_path).ok()?;
        DaemonInstanceMetadata::from_json(&raw).ok()
    }

    /// Remove the metadata file (if owned) and release the lock.
    pub fn release(mut self) {
        self.release_owned_paths();
    }

    fn release_owned_paths(&mut self) {
        if self.owns_metadata {
            let _ = std::fs::remove_file(&self.metadata_path);
            self.owns_metadata = false;
        }
    }
}

impl Drop for DaemonInstanceGuard {
    fn drop(&mut self) {
        self.release_owned_paths();
    }
}

// -----------------------------------------------------------------------------
// connect-or-start frontend helper
// -----------------------------------------------------------------------------

/// Outcome of a connect-or-start attempt. The `client` field is a live
/// `SocketCoreClient` suitable for immediate `request`/`subscribe` use;
/// callers should not re-connect.
pub struct ConnectOrStartOutcome {
    pub client: crate::core::transport::SocketCoreClient,
    pub daemon_id: String,
    pub endpoint: String,
    /// Set when this call started a new daemon; the PID of the spawned
    /// foreground child. None when an existing daemon was reused.
    pub started_pid: Option<u32>,
}

/// Errors produced by [`connect_or_start_daemon`].
#[derive(Debug)]
pub enum DaemonConnectError {
    /// The startup attempt exhausted its budget before the daemon became
    /// responsive.
    StartupTimeout { endpoint: String, timeout: Duration },
    /// The user-scoped lock is held but the socket is unreachable; do not
    /// unlink or steal the lock.
    InconsistentState { endpoint: String, detail: String },
    /// The launched child process exited before becoming ready.
    ChildExited { endpoint: String, detail: String },
    /// Filesystem / OS error during connect or launch.
    Io(AppError),
}

impl std::fmt::Display for DaemonConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StartupTimeout { endpoint, timeout } => write!(
                f,
                "daemon did not become ready within {} ms at {}",
                timeout.as_millis(),
                endpoint
            ),
            Self::InconsistentState { endpoint, detail } => {
                write!(f, "inconsistent daemon state at {}: {}", endpoint, detail)
            }
            Self::ChildExited { endpoint, detail } => {
                write!(f, "daemon child exited at {}: {}", endpoint, detail)
            }
            Self::Io(e) => write!(f, "daemon connect error: {}", e),
        }
    }
}

impl std::error::Error for DaemonConnectError {}

impl From<AppError> for DaemonConnectError {
    fn from(value: AppError) -> Self {
        Self::Io(value)
    }
}

/// Options for [`connect_or_start_daemon`].
#[derive(Clone, Debug)]
pub struct ConnectOrStartOptions {
    /// Resolved user-scoped paths.
    pub paths: DaemonPaths,
    /// If true, attempt to start a new daemon when none is running.
    pub autostart: bool,
    /// How long to wait for a freshly started daemon to become ready.
    pub startup_timeout: Duration,
    /// Poll interval when probing for readiness.
    pub poll_interval: Duration,
}

impl ConnectOrStartOptions {
    /// Default options built from [`DaemonPaths::resolve`] with a
    /// 10-second startup timeout and 100ms polling.
    pub fn for_default_paths() -> Self {
        Self {
            paths: DaemonPaths::resolve(),
            autostart: true,
            startup_timeout: Duration::from_secs(10),
            poll_interval: Duration::from_millis(100),
        }
    }
}

/// Connect to the user-scoped daemon; if none is running and `autostart`
/// is enabled, spawn a child daemon process and wait for readiness.
///
/// On success returns a [`ConnectOrStartOutcome`] containing a live
/// `SocketCoreClient` and metadata describing whether an existing daemon
/// was reused or a new one was started.
pub async fn connect_or_start_daemon(
    options: ConnectOrStartOptions,
) -> Result<ConnectOrStartOutcome, DaemonConnectError> {
    let endpoint = options.paths.endpoint_uri();
    options
        .paths
        .ensure_root()
        .map_err(DaemonConnectError::Io)?;

    // 1. Try connecting directly.
    if let Ok(client) = crate::core::transport::SocketCoreClient::connect(&endpoint).await {
        let daemon_id = match client.request(snapshot_request()).await {
            Ok(crate::protocol::core::CoreResponse::SnapshotDaemon { daemon_id, .. }) => daemon_id,
            _ => "unknown".to_string(),
        };
        return Ok(ConnectOrStartOutcome {
            client,
            daemon_id,
            endpoint,
            started_pid: None,
        });
    }

    // Connection refused or socket missing — try startup if allowed.
    if !options.autostart {
        let lock_held = is_lock_held(&options.paths.lock_path);
        if lock_held {
            let md = DaemonInstanceGuard::read_metadata(&options.paths.metadata_path);
            return Err(DaemonConnectError::InconsistentState {
                endpoint,
                detail: match md {
                    Some(m) => format!(
                        "lock held by daemon {} (pid {}) but socket unreachable",
                        m.daemon_id, m.pid
                    ),
                    None => "lock held by another process and socket unreachable".to_string(),
                },
            });
        }
        return Err(DaemonConnectError::Io(AppError::Other(anyhow::anyhow!(
            "no daemon running at {} (autostart disabled)",
            endpoint
        ))));
    }

    // 2. Spawn a detached child process that runs the singleton daemon.
    let exe = std::env::current_exe().map_err(|e| {
        DaemonConnectError::Io(AppError::Other(anyhow::anyhow!(
            "cannot resolve current exe: {}",
            e
        )))
    })?;
    let mut child = tokio::process::Command::new(exe)
        .args(["daemon", "start"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            DaemonConnectError::Io(AppError::Other(anyhow::anyhow!(
                "failed to spawn daemon: {}",
                e
            )))
        })?;
    let child_pid = child.id().unwrap_or(0);

    // 3. Poll for readiness.
    let deadline = tokio::time::Instant::now() + options.startup_timeout;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(DaemonConnectError::ChildExited {
                endpoint,
                detail: format!("daemon child exited with {:?}", status),
            });
        }
        match crate::core::transport::SocketCoreClient::connect(&endpoint).await {
            Ok(client) => {
                let daemon_id = match client.request(snapshot_request()).await {
                    Ok(crate::protocol::core::CoreResponse::SnapshotDaemon {
                        daemon_id, ..
                    }) => daemon_id,
                    _ => "unknown".to_string(),
                };
                return Ok(ConnectOrStartOutcome {
                    client,
                    daemon_id,
                    endpoint,
                    started_pid: Some(child_pid),
                });
            }
            Err(_) => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(DaemonConnectError::StartupTimeout {
                        endpoint,
                        timeout: options.startup_timeout,
                    });
                }
                tokio::time::sleep(options.poll_interval).await;
            }
        }
    }
}

// -----------------------------------------------------------------------------
// helpers
// -----------------------------------------------------------------------------

fn default_user_runtime_root() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("codegg");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(xrd) = std::env::var_os("XDG_RUNTIME_DIR") {
            return PathBuf::from(xrd).join("codegg");
        }
        if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
            return PathBuf::from(xdg).join("codegg");
        }
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("codegg");
        }
    }
    PathBuf::from("/tmp").join("codegg")
}

#[cfg(unix)]
fn open_lock_file(path: &Path) -> Result<std::fs::File, AppError> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| {
            AppError::Other(anyhow::anyhow!(
                "failed to open lock file {}: {}",
                path.display(),
                e
            ))
        })?;
    set_user_only_permissions(path);
    Ok(file)
}

#[cfg(not(unix))]
fn open_lock_file(path: &Path) -> Result<std::fs::File, AppError> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|e| {
            AppError::Other(anyhow::anyhow!(
                "failed to open lock file {}: {}",
                path.display(),
                e
            ))
        })
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn try_flock_exclusive(file: &std::fs::File) -> Result<bool, AppError> {
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    // LOCK_NB so we fail fast instead of blocking; the caller treats a
    // non-zero return (with EWOULDBLOCK) as "lock held by another process".
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        return Ok(true);
    }
    let err = std::io::Error::last_os_error();
    if err.raw_os_error() == Some(libc::EWOULDBLOCK) || err.raw_os_error() == Some(libc::EAGAIN) {
        return Ok(false);
    }
    Err(AppError::Other(anyhow::anyhow!("flock failed: {}", err)))
}

#[cfg(not(unix))]
fn try_flock_exclusive(_file: &std::fs::File) -> Result<bool, AppError> {
    Ok(true)
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn is_lock_held(lock_path: &Path) -> bool {
    use std::os::unix::fs::OpenOptionsExt;
    let open = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(lock_path);
    let Ok(file) = open else { return false };
    use std::os::fd::AsRawFd;
    let fd = file.as_raw_fd();
    // Try to acquire LOCK_EX | LOCK_NB. If it succeeds the lock is free; if
    // it fails with EWOULDBLOCK the lock is held.
    let ret = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
    if ret == 0 {
        // Release immediately so we don't hold it.
        unsafe { libc::flock(fd, libc::LOCK_UN) };
        false
    } else {
        true
    }
}

#[cfg(not(unix))]
fn is_lock_held(_lock_path: &Path) -> bool {
    false
}

fn atomic_write(path: &Path, contents: &[u8]) -> Result<(), AppError> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, contents).map_err(|e| {
        AppError::Other(anyhow::anyhow!(
            "failed to write temp file {}: {}",
            tmp.display(),
            e
        ))
    })?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(AppError::Other(anyhow::anyhow!(
            "failed to rename {} -> {}: {}",
            tmp.display(),
            path.display(),
            e
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn set_user_only_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        // Directories need execute permission to traverse; files don't.
        let mode = if meta.is_dir() { 0o700 } else { 0o600 };
        perms.set_mode(mode);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
fn set_user_only_permissions(_path: &Path) {}

fn snapshot_request() -> crate::protocol::core::RequestEnvelope<crate::protocol::core::CoreRequest>
{
    crate::core::new_request(
        "connect-or-start-status".to_string(),
        crate::protocol::core::CoreRequest::SnapshotDaemon,
    )
}

/// Construct a `DaemonInstanceMetadata` for the current process.
pub fn current_process_metadata(
    daemon_id: String,
    generation: String,
    socket_path: PathBuf,
) -> DaemonInstanceMetadata {
    DaemonInstanceMetadata {
        daemon_id,
        generation,
        pid: std::process::id(),
        socket_path,
        protocol_version: PROTOCOL_VERSION,
        started_at: Utc::now(),
        binary_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

/// Convenience: peek at the metadata file (no lock required). Returns
/// `None` when no metadata file is present.
pub fn read_metadata_for_paths(paths: &DaemonPaths) -> Option<DaemonInstanceMetadata> {
    DaemonInstanceGuard::read_metadata(&paths.metadata_path)
}

/// Internal: serialize-shared `Arc<Mutex<()>>` for tests that need to
/// gate overlapping tests on the user-scoped lock.
#[doc(hidden)]
pub fn global_lock_for_tests() -> Arc<Mutex<()>> {
    Arc::new(Mutex::new(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "codegg-instance-{}-{}",
            label,
            uuid::Uuid::new_v4().simple()
        ));
        p
    }

    #[test]
    fn paths_resolution_uses_overrides_and_defaults() {
        // Override via CODEGG_DAEMON_HOME
        let dir = temp_root("override");
        std::env::set_var("CODEGG_DAEMON_HOME", &dir);
        let p = DaemonPaths::resolve();
        assert_eq!(p.root, dir);
        assert!(p.lock_path.ends_with("daemon.lock"));
        assert!(p.socket_path.ends_with("core.sock"));
        assert!(p.metadata_path.ends_with("daemon.json"));
        std::env::remove_var("CODEGG_DAEMON_HOME");
    }

    #[test]
    fn paths_with_root_uses_specified_root() {
        let root = temp_root("with-root");
        let p = DaemonPaths::with_root(root.clone());
        assert_eq!(p.root, root);
        assert_eq!(p.lock_path, root.join("daemon.lock"));
        assert_eq!(p.metadata_path, root.join("daemon.json"));
        assert_eq!(p.socket_path, root.join("core.sock"));
    }

    #[test]
    fn metadata_roundtrip() {
        let m = DaemonInstanceMetadata {
            daemon_id: "codegg-deadbeef".into(),
            generation: "11111111-2222-3333-4444-555555555555".into(),
            pid: 4242,
            socket_path: PathBuf::from("/tmp/codegg/core.sock"),
            protocol_version: PROTOCOL_VERSION,
            started_at: Utc::now(),
            binary_version: "0.1.0".into(),
        };
        let json = m.to_json().unwrap();
        let back = DaemonInstanceMetadata::from_json(&json).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn metadata_roundtrip_through_atomic_write() {
        let root = temp_root("atomic");
        let paths = DaemonPaths::with_root(root.clone());
        paths.ensure_root().unwrap();
        let m = current_process_metadata(
            "codegg-aabbccdd".into(),
            "gen-aabbccdd".into(),
            paths.socket_path.clone(),
        );
        atomic_write(&paths.metadata_path, m.to_json().unwrap().as_bytes()).unwrap();
        let on_disk = DaemonInstanceGuard::read_metadata(&paths.metadata_path).unwrap();
        assert_eq!(on_disk.daemon_id, m.daemon_id);
        assert_eq!(on_disk.generation, m.generation);
        assert_eq!(on_disk.socket_path, m.socket_path);
        assert_eq!(on_disk.pid, m.pid);
        // Atomic write leaves no .tmp file behind.
        assert!(!paths.metadata_path.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn guard_acquire_blocks_second_acquisition() {
        let root = temp_root("guard");
        let paths = DaemonPaths::with_root(root.clone());
        paths.ensure_root().unwrap();

        let first = DaemonInstanceGuard::try_acquire(&paths).unwrap();
        assert!(first.is_some(), "first acquisition should succeed");
        let guard1 = first.unwrap();

        let second = DaemonInstanceGuard::try_acquire(&paths).unwrap();
        assert!(
            second.is_none(),
            "second acquisition should fail while first holds lock"
        );

        drop(guard1);

        let third = DaemonInstanceGuard::try_acquire(&paths).unwrap();
        assert!(
            third.is_some(),
            "third acquisition should succeed after release"
        );
        drop(third);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn guard_release_removes_owned_metadata() {
        let root = temp_root("release");
        let paths = DaemonPaths::with_root(root.clone());
        paths.ensure_root().unwrap();
        let mut guard = DaemonInstanceGuard::try_acquire(&paths).unwrap().unwrap();
        let m = current_process_metadata(
            "codegg-zzz".into(),
            "gen-zzz".into(),
            paths.socket_path.clone(),
        );
        guard.write_metadata(&m).unwrap();
        assert!(paths.metadata_path.exists());
        guard.release();
        assert!(!paths.metadata_path.exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn guard_release_does_not_remove_unowned_metadata() {
        let root = temp_root("unowned");
        let paths = DaemonPaths::with_root(root.clone());
        paths.ensure_root().unwrap();
        // Pre-existing metadata without our guard.
        let m = current_process_metadata(
            "codegg-yyy".into(),
            "gen-yyy".into(),
            paths.socket_path.clone(),
        );
        atomic_write(&paths.metadata_path, m.to_json().unwrap().as_bytes()).unwrap();
        let guard = DaemonInstanceGuard::try_acquire(&paths).unwrap().unwrap();
        guard.release();
        // Metadata should still exist because we never wrote it via this guard.
        assert!(paths.metadata_path.exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn endpoint_uri_formats_socket_path() {
        let root = temp_root("uri");
        let p = DaemonPaths::with_root(root.clone());
        let uri = p.endpoint_uri();
        assert!(uri.starts_with("unix://"));
        assert!(uri.ends_with("core.sock"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn core_runtime_mode_default_is_daemon_client() {
        assert_eq!(CoreRuntimeMode::default(), CoreRuntimeMode::DaemonClient);
        assert!(CoreRuntimeMode::DaemonClient.is_daemon_client());
        assert!(!CoreRuntimeMode::StandaloneInproc.is_daemon_client());
        assert!(CoreRuntimeMode::StandaloneInproc.is_inproc());
        assert!(CoreRuntimeMode::StandaloneStdio.is_stdio());
    }
}
