//! Multi-Project TUI persistent restoration service (Milestone 004).
//!
//! Owns atomic, debounced persistence of the TUI tab manifest. The
//! service is intentionally simple: it loads on demand, validates
//! on load, debounces writes, and flushes on shutdown.
//!
//! ## Invariants
//!
//! - File writes use a temp file + rename sequence with
//!   `fsync`-style flushing. The persisted file is never half-written.
//! - Permissions are restricted (`0o600` on Unix).
//! - Symlinks at the manifest path are rejected: the service refuses
//!   to overwrite an arbitrary symlink target.
//! - The service never persists secrets, prompts, tool outputs,
//!   file bodies, diffs, logs, terminal frames, subscriptions, or
//!   leases. Those are out of scope for the manifest (see
//!   `manifest.rs`).
//!
//! See `plans/implementation/tui-project-sessions/004-persistent-restoration-resource-closure.md`
//! for the full specification.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::tui::app::state::manifest::{
    default_manifest_path, default_temp_path, validate_manifest, ManifestDiagnostic,
    ManifestLoadOutcome, SerializedManifest, TuiWorkspaceManifest, MAX_MANIFEST_BYTES,
};

/// Default debounce interval for coalescing rapid save requests.
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(500);

/// Maximum size of the in-memory pending queue. The service refuses
/// to queue more than this many pending snapshots; oldest is dropped.
pub const MAX_PENDING_SNAPSHOTS: usize = 4;

/// Snapshot of the TUI state that is safe to persist. Constructed
/// by the TUI at the moment of a state mutation and handed to the
/// persistence service.
#[derive(Debug, Clone)]
pub struct PersistedSnapshot {
    /// The manifest to write. Already validated by the caller so
    /// the persistence layer does not need to re-run validation.
    pub manifest: TuiWorkspaceManifest,
}

/// Coarse-grained diagnostic snapshot for operator visibility. The
/// service keeps a rolling count of recent outcomes so the TUI can
/// surface "manifest saved N times since startup" without exposing
/// the file path or contents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PersistenceMetrics {
    /// Number of successful atomic writes since startup.
    pub saves_completed: u64,
    /// Number of saves that were coalesced (debounced, never
    /// reached disk).
    pub saves_coalesced: u64,
    /// Number of times a scheduled save was cancelled because the
    /// manifest did not change between scheduled writes.
    pub saves_deduped: u64,
    /// Number of save failures since startup.
    pub saves_failed: u64,
    /// Number of manifest loads since startup.
    pub loads_attempted: u64,
    /// Last load outcome (absent, loaded, rejected).
    pub last_load_outcome: Option<ManifestLoadOutcomeKind>,
    /// Whether the persistence service is currently disabled
    /// (operator `disable`).
    pub disabled: bool,
}

/// Coarse load outcome for the metrics surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManifestLoadOutcomeKind {
    Absent,
    Loaded,
    Rejected,
}

/// Persistence service. Holds the state root path, a mutex-guarded
/// pending snapshot, and the metrics. Cheap to clone only when
/// wrapped in `Arc`; the service itself is not Clone because it
/// carries mutable in-memory state.
#[derive(Debug)]
pub struct ManifestPersistence {
    state_root: PathBuf,
    debounce: Duration,
    metrics: PersistenceMetrics,
    /// Pending snapshot awaiting flush. Coalescing replaces this on
    /// each `schedule_save` call.
    pending: Option<PendingSnapshot>,
    /// Disabled flag (operator control).
    disabled: bool,
}

#[derive(Debug)]
struct PendingSnapshot {
    snapshot: PersistedSnapshot,
    /// When the snapshot was scheduled.
    scheduled_at: Instant,
    /// Whether `flush` has been called (forces immediate write).
    force: bool,
}

impl ManifestPersistence {
    /// Create a new persistence service rooted at the given state
    /// directory. The directory is not created until the first save
    /// or explicit `ensure_root`.
    pub fn new(state_root: impl Into<PathBuf>) -> Self {
        Self::with_debounce(state_root, DEFAULT_DEBOUNCE)
    }

    /// Create a new persistence service with a custom debounce
    /// window. Tests use this to set a sub-millisecond debounce.
    pub fn with_debounce(state_root: impl Into<PathBuf>, debounce: Duration) -> Self {
        Self {
            state_root: state_root.into(),
            debounce,
            metrics: PersistenceMetrics::default(),
            pending: None,
            disabled: false,
        }
    }

    /// Disable persistence. Subsequent `schedule_save` calls are
    /// dropped. Used by the operator `disable` control and by
    /// remote-core startup where persistence is not desired.
    pub fn disable(&mut self) {
        self.disabled = true;
        self.metrics.disabled = true;
        self.pending = None;
    }

    /// Re-enable persistence after `disable`. Does not flush
    /// previously pending snapshots.
    pub fn enable(&mut self) {
        self.disabled = false;
        self.metrics.disabled = false;
    }

    /// Whether persistence is currently enabled.
    pub fn is_enabled(&self) -> bool {
        !self.disabled
    }

    /// The path the service would write to. Diagnostic only.
    pub fn manifest_path(&self) -> PathBuf {
        default_manifest_path(&self.state_root)
    }

    /// Take a metrics snapshot.
    pub fn metrics(&self) -> PersistenceMetrics {
        self.metrics.clone()
    }

    /// Ensure the state root directory exists. Idempotent. Called
    /// lazily by `schedule_save` so loading the service does not
    /// touch disk.
    pub fn ensure_root(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.state_root)?;
        Ok(())
    }

    /// Schedule a save. The snapshot is held in memory until either
    /// the debounce window elapses or `flush` is called. Coalescing
    /// replaces any pending snapshot with the latest one.
    pub fn schedule_save(&mut self, snapshot: PersistedSnapshot) {
        if self.disabled {
            return;
        }
        // Dedup: if the new snapshot is byte-equal to the pending
        // one, drop the write entirely.
        if let Some(pending) = &self.pending {
            if pending.snapshot.manifest == snapshot.manifest {
                self.metrics.saves_deduped = self.metrics.saves_deduped.saturating_add(1);
                return;
            }
        }
        self.pending = Some(PendingSnapshot {
            snapshot,
            scheduled_at: Instant::now(),
            force: false,
        });
    }

    /// Schedule a forced save that ignores the debounce window but
    /// still respects the disabled flag and the dedup rule.
    pub fn schedule_force_save(&mut self, snapshot: PersistedSnapshot) {
        if self.disabled {
            return;
        }
        if let Some(pending) = &self.pending {
            if pending.snapshot.manifest == snapshot.manifest {
                self.metrics.saves_deduped = self.metrics.saves_deduped.saturating_add(1);
                return;
            }
        }
        self.pending = Some(PendingSnapshot {
            snapshot,
            scheduled_at: Instant::now(),
            force: true,
        });
    }

    /// Returns whether a pending snapshot exists. Used by the event
    /// loop tick to decide whether to flush.
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Returns whether the pending snapshot is due (debounce elapsed
    /// or force flag set). Used by the event loop.
    pub fn pending_is_due(&self) -> bool {
        match &self.pending {
            None => false,
            Some(p) => p.force || p.scheduled_at.elapsed() >= self.debounce,
        }
    }

    /// Flush the pending snapshot to disk if one is present. Returns
    /// `Ok(true)` when a write was performed, `Ok(false)` when
    /// nothing was pending, and `Err` on I/O failure (the manifest
    /// is left pending for the next call).
    pub fn flush(&mut self) -> Result<bool, PersistenceError> {
        let pending = match self.pending.take() {
            Some(p) => p,
            None => {
                return Ok(false);
            }
        };
        if self.disabled {
            // Drop silently — operator disabled persistence mid-flight.
            self.metrics.saves_coalesced = self.metrics.saves_coalesced.saturating_add(1);
            return Ok(false);
        }
        match self.write_atomic(&pending.snapshot.manifest) {
            Ok(()) => {
                self.metrics.saves_completed = self.metrics.saves_completed.saturating_add(1);
                Ok(true)
            }
            Err(e) => {
                // Re-queue so the next flush retries.
                self.pending = Some(pending);
                self.metrics.saves_failed = self.metrics.saves_failed.saturating_add(1);
                Err(e)
            }
        }
    }

    /// Reset persistence: delete the manifest file (best-effort)
    /// and clear all pending state. Used by the operator `reset`
    /// control.
    pub fn reset(&mut self) -> std::io::Result<()> {
        self.pending = None;
        let path = self.manifest_path();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        let temp = default_temp_path(&self.state_root);
        if temp.exists() {
            let _ = std::fs::remove_file(&temp);
        }
        Ok(())
    }

    /// Load the manifest from disk. The result is always classified
    /// (absent, loaded, rejected); callers never receive a raw
    /// error. Updates `metrics`.
    pub fn load_manifest(&mut self) -> ManifestLoadOutcome {
        self.metrics.loads_attempted = self.metrics.loads_attempted.saturating_add(1);
        let outcome = load_manifest_from(&self.manifest_path());
        self.metrics.last_load_outcome = Some(match &outcome {
            ManifestLoadOutcome::Absent => ManifestLoadOutcomeKind::Absent,
            ManifestLoadOutcome::Loaded(_) => ManifestLoadOutcomeKind::Loaded,
            ManifestLoadOutcome::Rejected(_) => ManifestLoadOutcomeKind::Rejected,
        });
        outcome
    }

    /// Atomically write the manifest to disk. Public for tests.
    pub fn write_atomic(&self, manifest: &TuiWorkspaceManifest) -> Result<(), PersistenceError> {
        self.ensure_root()?;
        let path = self.manifest_path();
        let temp = default_temp_path(&self.state_root);

        // Refuse to overwrite a symlink at the target path.
        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Path does not exist; safe to proceed.
                std::fs::File::create(&temp).map_err(PersistenceError::Io)?;
                write_serialized(&temp, manifest)?;
                std::fs::rename(&temp, &path).map_err(PersistenceError::Io)?;
                return Ok(());
            }
            Err(e) => return Err(PersistenceError::Io(e)),
        };
        if meta.file_type().is_symlink() {
            return Err(PersistenceError::SymlinkRefused { path });
        }

        // Same check for the temp file (rare, but possible).
        if let Ok(temp_meta) = std::fs::symlink_metadata(&temp) {
            if temp_meta.file_type().is_symlink() {
                return Err(PersistenceError::SymlinkRefused { path: temp });
            }
        }

        write_serialized(&temp, manifest)?;
        std::fs::rename(&temp, &path).map_err(PersistenceError::Io)?;
        Ok(())
    }
}

fn write_serialized(temp: &Path, manifest: &TuiWorkspaceManifest) -> Result<(), PersistenceError> {
    let SerializedManifest { bytes } = serialize_manifest(manifest)?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err(PersistenceError::Oversized { bytes: bytes.len() });
    }
    std::fs::write(temp, &bytes).map_err(PersistenceError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(temp, perms).map_err(PersistenceError::Io)?;
    }
    // fsync best-effort: ignored on platforms where it is unsupported.
    let _ = sync_file(temp);
    Ok(())
}

/// Serialize a manifest, bounded by `MAX_MANIFEST_BYTES`.
pub fn serialize_manifest(
    manifest: &TuiWorkspaceManifest,
) -> Result<SerializedManifest, PersistenceError> {
    let bytes = serde_json::to_vec(manifest).map_err(PersistenceError::Serialize)?;
    Ok(SerializedManifest { bytes })
}

/// Best-effort fsync of a single file. Returns the underlying io
/// error if the platform call fails; callers may ignore it.
#[cfg(unix)]
fn sync_file(path: &Path) -> std::io::Result<()> {
    use std::fs::File;
    use std::os::unix::fs::FileExt;
    let f = File::open(path)?;
    f.sync_all()
}

#[cfg(not(unix))]
fn sync_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Load a manifest from disk and classify the outcome.
pub fn load_manifest_from(path: &Path) -> ManifestLoadOutcome {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ManifestLoadOutcome::Absent,
        Err(e) => {
            return ManifestLoadOutcome::Rejected(ManifestDiagnostic::Unreadable {
                reason: bounded_reason(&e),
            });
        }
    };
    if meta.file_type().is_symlink() {
        return ManifestLoadOutcome::Rejected(ManifestDiagnostic::ForbiddenIdentity {
            reason: "manifest is a symlink".into(),
        });
    }
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return ManifestLoadOutcome::Rejected(ManifestDiagnostic::Unreadable {
                reason: bounded_reason(&e),
            });
        }
    };
    if bytes.len() > MAX_MANIFEST_BYTES {
        return ManifestLoadOutcome::Rejected(ManifestDiagnostic::Oversized { bytes: bytes.len() });
    }
    let mut manifest: TuiWorkspaceManifest = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(e) => {
            return ManifestLoadOutcome::Rejected(ManifestDiagnostic::InvalidJson {
                reason: bounded_reason(&e),
            });
        }
    };
    if let Err(diag) = validate_manifest(&mut manifest) {
        return ManifestLoadOutcome::Rejected(diag);
    }
    ManifestLoadOutcome::Loaded(manifest)
}

/// Bound an io::Error or serde_json::Error reason string so we
/// never surface unbounded text in the diagnostic. Reasons are
/// truncated to 128 characters.
fn bounded_reason(err: &(impl std::fmt::Display)) -> String {
    let s = err.to_string();
    if s.len() <= 128 {
        s
    } else {
        let mut end = 128;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_string()
    }
}

/// Persistence-layer error. I/O failures keep the manifest pending
/// for retry; logic failures (oversized, symlink) are terminal and
/// should be surfaced to the operator.
#[derive(Debug)]
pub enum PersistenceError {
    Io(std::io::Error),
    Serialize(serde_json::Error),
    SymlinkRefused { path: PathBuf },
    Oversized { bytes: usize },
}

impl std::fmt::Display for PersistenceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "persistence I/O error: {e}"),
            Self::Serialize(e) => write!(f, "manifest serialize error: {e}"),
            Self::SymlinkRefused { path } => write!(f, "refused to overwrite symlink at {path:?}"),
            Self::Oversized { bytes } => write!(f, "manifest oversized: {bytes} bytes"),
        }
    }
}

impl std::error::Error for PersistenceError {}

impl From<std::io::Error> for PersistenceError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<serde_json::Error> for PersistenceError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialize(e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::state::manifest::{PersistedProjectTab, MANIFEST_SCHEMA_VERSION};

    fn tmpdir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn empty_snapshot() -> PersistedSnapshot {
        PersistedSnapshot {
            manifest: TuiWorkspaceManifest::default(),
        }
    }

    #[test]
    fn schedule_then_flush_writes_to_disk() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        let snap = empty_snapshot();
        svc.schedule_force_save(snap);
        assert!(svc.has_pending());
        let wrote = svc.flush().unwrap();
        assert!(wrote);
        assert!(!svc.has_pending());
        assert!(svc.manifest_path().exists());
    }

    #[test]
    fn schedule_coalesces_multiple_saves() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.schedule_force_save(empty_snapshot());
        svc.schedule_force_save(empty_snapshot());
        svc.schedule_force_save(empty_snapshot());
        let wrote = svc.flush().unwrap();
        assert!(wrote);
        assert_eq!(svc.metrics().saves_completed, 1);
    }

    #[test]
    fn dedup_skips_identical_snapshot() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.schedule_force_save(empty_snapshot());
        svc.schedule_force_save(empty_snapshot());
        // The second call sees the first pending and dedups.
        assert!(!svc.has_pending() || svc.metrics().saves_deduped >= 1);
    }

    #[test]
    fn dedup_skips_identical_snapshot_when_replaced() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.schedule_force_save(empty_snapshot());
        // Different snapshot replaces.
        let mut m = TuiWorkspaceManifest::default();
        m.ordered_tabs.push(PersistedProjectTab {
            project_id: Some("p1".into()),
            workspace_id: None,
            session_id: None,
            label_hint: None,
            selected_model_id: None,
            selected_agent: None,
            order_key: Some("k1".into()),
        });
        svc.schedule_force_save(PersistedSnapshot {
            manifest: m.clone(),
        });
        // Now reschedule the same manifest — should dedup.
        svc.schedule_force_save(PersistedSnapshot { manifest: m });
        assert!(svc.metrics().saves_deduped >= 1);
    }

    #[test]
    fn disable_drops_pending() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.schedule_force_save(empty_snapshot());
        svc.disable();
        assert!(!svc.has_pending());
        svc.schedule_save(empty_snapshot());
        let wrote = svc.flush().unwrap();
        assert!(!wrote);
    }

    #[test]
    fn enable_resumes_persistence() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.disable();
        svc.enable();
        svc.schedule_force_save(empty_snapshot());
        let wrote = svc.flush().unwrap();
        assert!(wrote);
    }

    #[test]
    fn reset_clears_file_and_pending() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.schedule_force_save(empty_snapshot());
        svc.flush().unwrap();
        assert!(svc.manifest_path().exists());
        svc.reset().unwrap();
        assert!(!svc.manifest_path().exists());
        assert!(!svc.has_pending());
    }

    #[test]
    fn load_returns_absent_when_no_file() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        let outcome = svc.load_manifest();
        assert!(matches!(outcome, ManifestLoadOutcome::Absent));
    }

    #[test]
    fn load_returns_loaded_after_write() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.schedule_force_save(empty_snapshot());
        svc.flush().unwrap();
        let outcome = svc.load_manifest();
        match outcome {
            ManifestLoadOutcome::Loaded(m) => {
                assert_eq!(m.schema_version, MANIFEST_SCHEMA_VERSION);
            }
            other => panic!("unexpected outcome {other:?}"),
        }
    }

    #[test]
    fn load_rejects_oversized_manifest() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.ensure_root().unwrap();
        // Write a file larger than the cap directly.
        let path = svc.manifest_path();
        let huge = vec![b'x'; MAX_MANIFEST_BYTES + 1];
        std::fs::write(&path, &huge).unwrap();
        let outcome = svc.load_manifest();
        assert!(matches!(outcome, ManifestLoadOutcome::Rejected(_)));
    }

    #[test]
    fn load_rejects_symlink_at_manifest_path() {
        let dir = tmpdir();
        let svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.ensure_root().unwrap();
        let path = svc.manifest_path();
        let target = dir.path().join("elsewhere.json");
        std::fs::write(&target, b"{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path).unwrap();
        #[cfg(not(unix))]
        {
            // On non-unix this test is a no-op.
            let _ = (path, target);
            return;
        }
        let outcome = load_manifest_from(&path);
        assert!(matches!(
            outcome,
            ManifestLoadOutcome::Rejected(ManifestDiagnostic::ForbiddenIdentity { .. })
        ));
    }

    #[test]
    fn write_atomic_refuses_symlink_target() {
        let dir = tmpdir();
        let svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        let path = svc.manifest_path();
        svc.ensure_root().unwrap();
        let target = dir.path().join("external.json");
        std::fs::write(&target, b"{}").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &path).unwrap();
        #[cfg(not(unix))]
        {
            let _ = (path, target);
            return;
        }
        let err = svc
            .write_atomic(&TuiWorkspaceManifest::default())
            .unwrap_err();
        assert!(matches!(err, PersistenceError::SymlinkRefused { .. }));
    }

    #[test]
    fn write_atomic_sets_restrictive_permissions() {
        let dir = tmpdir();
        let svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.write_atomic(&TuiWorkspaceManifest::default()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(svc.manifest_path()).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "expected 0o600 got {mode:o}");
        }
    }

    #[test]
    fn flush_returns_false_when_nothing_pending() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        let wrote = svc.flush().unwrap();
        assert!(!wrote);
    }

    #[test]
    fn pending_is_due_respects_force() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(10_000));
        svc.schedule_save(empty_snapshot());
        assert!(!svc.pending_is_due(), "not yet due with long debounce");
        let mut snap = empty_snapshot();
        snap.manifest.active_project_id = Some("different".into());
        svc.schedule_force_save(snap);
        assert!(svc.pending_is_due());
    }

    #[test]
    fn metrics_increment_after_write() {
        let dir = tmpdir();
        let mut svc = ManifestPersistence::with_debounce(dir.path(), Duration::from_millis(0));
        svc.schedule_force_save(empty_snapshot());
        svc.flush().unwrap();
        let m = svc.metrics();
        assert_eq!(m.saves_completed, 1);
        assert_eq!(m.saves_failed, 0);
    }
}
