//! C004: single-flight admission for file-tree blocking scans.
//!
//! The pre-C004 `rebuild` aborted the outer Tokio waiter with
//! `cancel_kind(FileTree)` and immediately spawned a new waiter plus a new
//! `spawn_blocking` scan. Aborting the waiter does **not** stop the blocking
//! closure: the old scan keeps running until its next cancellation checkpoint
//! (or until a slow `read_dir` syscall returns). Repeated rebuilds could
//! therefore pile up concurrent blocking scans while
//! `a_hundred_rapid_rebuilds_keep_one_live_task` still reported one registry
//! task. The registry counts waiters, not workers.
//!
//! This coordinator makes the stronger claim true: at most **one** blocking
//! scan executes at a time (global single-flight, preferred per the plan),
//! with at most one coalesced latest-only pending intent. The admission
//! permit is held from the event-loop `try_admit` until the blocking work's
//! completion arrives; aborting a waiter never frees capacity early. A
//! superseded scan continues only as one accounted, non-authoritative,
//! finite-budget prior operation; there is no unbounded pileup.
//!
//! Why single-flight (cap 1) rather than a small semaphore: tree scans are
//! infrequent, user-initiated, and latency-tolerant. Overlapping walks would
//! contend on the same disk, complicate generation fencing, and buy no
//! responsiveness: the newest request is already coalesced as pending and
//! runs immediately after the in-flight scan exits. One worker keeps the
//! reasoning (and the tests) exact.
//!
//! A slow or hung kernel filesystem syscall cannot be preempted by aborting
//! a Tokio waiter. Cooperative cancellation still exits within one
//! *completed* entry, but that is not a wall-clock bound. The UI therefore
//! reports an explicit `Waiting` state while a prior scan is stuck, stays
//! responsive, and never claims a join deadline the OS does not enforce.
//!
//! Threading: admission (`active`/`pending`/`superseded`) is driven only on
//! the TUI event loop. `running`/`peak_running` are touched from blocking
//! threads purely as instrumentation (actual worker entry/exit), via
//! [`RunningGuard`]. No private paths or file contents are logged; diagnostics
//! expose counts only.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// One scan intent, captured immutably at request time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRequest {
    /// `FileTreeState::generation` at request time. Monotonic per rebuild.
    pub generation: u64,
    /// `AsyncUiRequestState` id at request time.
    pub request_id: u64,
    /// Workspace root captured at request time. Compared on completion and
    /// before launching a queued scan; never logged. Fencing state only —
    /// C005 never walks it.
    pub root: PathBuf,
    /// Frontend tab that issued the request, for scope fencing.
    pub tab_id: Option<String>,
    /// Canonical project binding at request time. Sent on the daemon
    /// request; never logged.
    pub project_id: String,
    /// Canonical workspace binding at request time. Sent on the daemon
    /// request and matched against the echoed daemon identity before any
    /// entry renders; never logged.
    pub workspace_id: String,
}

/// Admission decision for a new rebuild.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admit {
    /// No scan is active; the caller must spawn the blocking work now.
    Run,
    /// A scan is active; the request was coalesced as the single pending
    /// intent. The caller must **not** spawn new blocking work and must
    /// **not** abort the active waiter (it will complete stale and drive the
    /// pending scan). UI should show `Waiting`.
    Queued,
}

#[derive(Debug, Default)]
struct Inner {
    active: Option<ScanRequest>,
    pending: Option<ScanRequest>,
    superseded_count: u64,
    completed_scans: u64,
    /// Actual blocking workers currently inside the scan closure.
    running: usize,
    /// Highest observed `running`. Reset only by explicit test control.
    peak_running: usize,
}

/// Single-flight coordinator for file-tree blocking scans.
///
/// Cheaply cloneable; clones share the same inner state. Stored on `App`
/// (not in `FileTreeState`, which is cloned for `/files` snapshots).
#[derive(Debug, Clone, Default)]
pub struct FileTreeScanCoordinator {
    inner: Arc<Mutex<Inner>>,
}

impl FileTreeScanCoordinator {
    /// New idle coordinator: no active scan, no pending intent.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
        }
    }

    /// Attempt to admit `request`.
    ///
    /// * `Run` — no active scan; the caller owns the permit and must spawn
    ///   exactly one blocking scan for this request.
    /// * `Queued` — a scan is active; `request` becomes the single pending
    ///   intent (overwriting any older pending, which counts as superseded).
    ///   The caller must not spawn.
    pub fn try_admit(&self, request: ScanRequest) -> Admit {
        let mut inner = self.inner.lock().expect("file-tree coordinator lock");
        match inner.active {
            None => {
                inner.active = Some(request);
                Admit::Run
            }
            Some(_) => {
                // Every queued admission supersedes the previous intent: either the
                // still-running active scan (first queue) or the previous pending
                // (overwrite). Count each as one supersession.
                inner.superseded_count = inner.superseded_count.saturating_add(1);
                inner.pending = Some(request);
                Admit::Queued
            }
        }
    }

    /// Mark completion of the active scan and take the pending intent, if any.
    ///
    /// Called on the event loop when a `FileTreeListed` completion arrives
    /// (including the truncated-on-panic path). Clears `active` only when the
    /// completion matches the active `generation` + `request_id`; a stale
    /// completion (e.g. after a shutdown force-clear) returns `None` and leaves
    /// state untouched. The returned pending (if any) still needs validation
    /// against current `FileTreeState` before launching (see
    /// [`Self::take_pending_validated`]).
    pub fn finish_active(&self, generation: u64, request_id: u64) -> Option<ScanRequest> {
        let mut inner = self.inner.lock().expect("file-tree coordinator lock");
        let matches = match &inner.active {
            Some(active) => active.generation == generation && active.request_id == request_id,
            None => false,
        };
        if !matches {
            return None;
        }
        inner.active = None;
        inner.completed_scans = inner.completed_scans.saturating_add(1);
        inner.pending.take()
    }

    /// Take the pending intent only if it still matches the current view
    /// state. Use this before launching a queued scan after the active scan
    /// exits: a project switch, reset, or validation failure may have made the
    /// queued root stale even though it was the latest at queue time.
    pub fn take_pending_validated(
        &self,
        current_generation: u64,
        current_root: &std::path::Path,
    ) -> Option<ScanRequest> {
        let mut inner = self.inner.lock().expect("file-tree coordinator lock");
        match &inner.pending {
            Some(pending)
                if pending.generation == current_generation && pending.root == current_root =>
            {
                inner.pending.take()
            }
            Some(_) => {
                // Stale pending (e.g. reset cleared the view but not the queue, or a
                // validation failure bumped generation without queueing). Drop it
                // rather than scanning the wrong workspace.
                inner.pending.take();
                None
            }
            None => None,
        }
    }

    /// Peek at the pending intent without removing it.
    pub fn pending(&self) -> Option<ScanRequest> {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .pending
            .clone()
    }

    /// Drop any queued intent without touching the active scan. Used on
    /// validation failure (non-local root, missing channel), project reset,
    /// and shutdown: the active scan (if any) still drains as a stale,
    /// bounded prior operation, but nothing new will launch after it.
    pub fn clear_pending(&self) {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .pending = None;
    }

    /// Force-clear both active and pending. Shutdown only: the event loop is
    /// going away, so no completion will ever drive the pending scan. A
    /// blocking worker may still be inside its syscall; its [`RunningGuard`]
    /// still decrements `running` on exit, but the late completion (if the
    /// waiter was not aborted) will mismatch and be discarded. Never claims
    /// the worker was joined.
    pub fn force_clear_for_shutdown(&self) {
        let mut inner = self.inner.lock().expect("file-tree coordinator lock");
        inner.active = None;
        inner.pending = None;
    }

    /// Number of blocking workers currently inside the scan closure (0 or 1
    /// by construction; higher proves a bug).
    pub fn running_scan_count(&self) -> usize {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .running
    }

    /// Highest observed concurrent blocking workers since creation (or since
    /// the last [`Self::reset_peak`] in tests).
    pub fn peak_running(&self) -> usize {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .peak_running
    }

    /// Reset the peak counter. Tests only.
    pub fn reset_peak(&self) {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .peak_running = 0;
    }

    /// Whether a coalesced latest intent is waiting for the active scan.
    pub fn has_queued(&self) -> bool {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .pending
            .is_some()
    }

    /// How many admissions were queued (superseded the active or a previous
    /// pending) since creation.
    pub fn superseded_count(&self) -> u64 {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .superseded_count
    }

    /// How many active scans completed (each drove at most one pending).
    pub fn completed_scans(&self) -> u64 {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .completed_scans
    }

    /// Whether any scan is admitted (active present).
    pub fn has_active(&self) -> bool {
        self.inner
            .lock()
            .expect("file-tree coordinator lock")
            .active
            .is_some()
    }

    /// Count-only diagnostics for logs and `/tui-stats`. Never includes paths,
    /// names, or file contents.
    pub fn diagnostics(&self) -> String {
        let inner = self.inner.lock().expect("file-tree coordinator lock");
        format!(
            "file_tree_scan active={} queued={} running={} peak={} superseded={} completed={}",
            if inner.active.is_some() { 1 } else { 0 },
            if inner.pending.is_some() { 1 } else { 0 },
            inner.running,
            inner.peak_running,
            inner.superseded_count,
            inner.completed_scans,
        )
    }

    /// Track actual blocking-worker entry. Call at the top of the
    /// `spawn_blocking` closure; the returned guard decrements on exit
    /// (including panic unwind). This is instrumentation, not admission:
    /// gating already happened via [`Self::try_admit`].
    pub fn track_enter(&self) -> RunningGuard {
        let mut inner = self.inner.lock().expect("file-tree coordinator lock");
        inner.running = inner.running.saturating_add(1);
        if inner.running > inner.peak_running {
            inner.peak_running = inner.running;
        }
        RunningGuard {
            coordinator: self.clone(),
        }
    }
}

/// RAII guard for one blocking worker's lifetime. Decrements `running` on
/// drop, including panic unwind. Never touches `active`/`pending`: those are
/// owned by the event loop's completion path.
#[derive(Debug)]
pub struct RunningGuard {
    coordinator: FileTreeScanCoordinator,
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.coordinator.inner.lock() {
            inner.running = inner.running.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};

    fn request(generation: u64, request_id: u64, root: &str) -> ScanRequest {
        ScanRequest {
            generation,
            request_id,
            root: PathBuf::from(root),
            tab_id: Some("tab-a".to_string()),
            project_id: "project-test".to_string(),
            workspace_id: "workspace-test".to_string(),
        }
    }

    #[test]
    fn first_admission_runs_second_queues_and_overwrites() {
        let coord = FileTreeScanCoordinator::new();
        assert_eq!(coord.try_admit(request(1, 1, "/a")), Admit::Run);
        assert!(coord.has_active());
        assert!(!coord.has_queued());

        assert_eq!(coord.try_admit(request(2, 2, "/a")), Admit::Queued);
        assert!(coord.has_queued());
        assert_eq!(coord.superseded_count(), 1);

        // A third admission overwrites the pending latest-only, not queues.
        assert_eq!(coord.try_admit(request(3, 3, "/a")), Admit::Queued);
        assert!(coord.has_queued());
        assert_eq!(coord.superseded_count(), 2);
        assert_eq!(coord.pending().expect("pending").generation, 3);

        // Finishing the active scan hands back the latest pending only.
        let pending = coord.finish_active(1, 1).expect("pending");
        assert_eq!(pending.generation, 3);
        assert!(!coord.has_active());
        assert!(!coord.has_queued());
    }

    #[test]
    fn stale_completion_does_not_clear_or_launch() {
        let coord = FileTreeScanCoordinator::new();
        assert_eq!(coord.try_admit(request(5, 5, "/a")), Admit::Run);
        // Wrong generation: no clear, no pending.
        assert!(coord.finish_active(4, 5).is_none());
        assert!(coord.has_active());
        assert_eq!(coord.completed_scans(), 0);
        // Wrong request id: same.
        assert!(coord.finish_active(5, 4).is_none());
        assert!(coord.has_active());
    }

    #[test]
    fn pending_validated_against_current_view() {
        let coord = FileTreeScanCoordinator::new();
        assert_eq!(coord.try_admit(request(1, 1, "/a")), Admit::Run);
        assert_eq!(coord.try_admit(request(2, 2, "/b")), Admit::Queued);
        // Current view moved on to /c without queueing (e.g. reset): the queued
        // /b intent is stale and must be dropped, not scanned.
        assert!(coord
            .take_pending_validated(3, PathBuf::from("/c").as_path())
            .is_none());
        assert!(!coord.has_queued());
    }

    #[test]
    fn clear_pending_leaves_active_to_drain() {
        let coord = FileTreeScanCoordinator::new();
        assert_eq!(coord.try_admit(request(1, 1, "/a")), Admit::Run);
        assert_eq!(coord.try_admit(request(2, 2, "/a")), Admit::Queued);
        coord.clear_pending();
        assert!(!coord.has_queued());
        assert!(coord.has_active());
        // Active still finishes normally; no pending to launch.
        assert!(coord.finish_active(1, 1).is_none());
        assert!(!coord.has_active());
    }

    #[test]
    fn force_clear_for_shutdown_discards_late_completion() {
        let coord = FileTreeScanCoordinator::new();
        assert_eq!(coord.try_admit(request(1, 1, "/a")), Admit::Run);
        assert_eq!(coord.try_admit(request(2, 2, "/a")), Admit::Queued);
        coord.force_clear_for_shutdown();
        assert!(!coord.has_active());
        assert!(!coord.has_queued());
        // A late completion from the still-running worker mismatches and is
        // discarded; it must not resurrect a pending scan.
        assert!(coord.finish_active(1, 1).is_none());
    }

    #[test]
    fn running_guard_tracks_entry_exit_and_panic() {
        let coord = FileTreeScanCoordinator::new();
        assert_eq!(coord.running_scan_count(), 0);
        assert_eq!(coord.peak_running(), 0);
        {
            let _guard = coord.track_enter();
            assert_eq!(coord.running_scan_count(), 1);
            assert_eq!(coord.peak_running(), 1);
        }
        assert_eq!(coord.running_scan_count(), 0);
        assert_eq!(coord.peak_running(), 1);

        // Panic unwind still drops the guard.
        let result = std::panic::catch_unwind({
            let coord = coord.clone();
            move || {
                let _guard = coord.track_enter();
                panic!("intentional test panic");
            }
        });
        assert!(result.is_err());
        assert_eq!(coord.running_scan_count(), 0);
        assert_eq!(coord.peak_running(), 1);
    }

    /// Deterministic barrier proof that at most one blocking worker runs even
    /// when 100 rebuilds arrive while the first is held. No wall-clock sleeps:
    /// the held worker waits on a barrier the test controls.
    #[test]
    fn barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending() {
        let coord = FileTreeScanCoordinator::new();
        let active = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        // Barrier for 2: held worker + test thread releases it at the end.
        let barrier = Arc::new(Barrier::new(2));

        // First admission runs.
        assert_eq!(coord.try_admit(request(1, 1, "/a")), Admit::Run);

        // Held blocking worker: enters, records peak, waits.
        let worker_coord = coord.clone();
        let worker_active = Arc::clone(&active);
        let worker_peak = Arc::clone(&peak);
        let worker_barrier = Arc::clone(&barrier);
        let held = std::thread::spawn(move || {
            let _guard = worker_coord.track_enter();
            let now = worker_active.fetch_add(1, Ordering::SeqCst) + 1;
            worker_peak.fetch_max(now, Ordering::SeqCst);
            // Hold until the test has queued 100 superseding rebuilds.
            worker_barrier.wait();
            worker_active.fetch_sub(1, Ordering::SeqCst);
            // Guard drops here: running back to 0.
        });

        // Wait until the worker is provably inside (spin on atomic, bounded).
        let mut spins = 0;
        while active.load(Ordering::SeqCst) == 0 && spins < 10_000 {
            std::thread::yield_now();
            spins += 1;
        }
        assert_eq!(
            active.load(Ordering::SeqCst),
            1,
            "held worker must be inside"
        );

        // 100 superseding rebuilds while the first is held. Each must queue,
        // never run, and coalesce to one pending.
        for i in 2..102 {
            assert_eq!(
                coord.try_admit(request(i, i, "/a")),
                Admit::Queued,
                "rebuild {i} must queue while the first worker is held"
            );
        }
        assert_eq!(coord.running_scan_count(), 1);
        assert_eq!(coord.peak_running(), 1);
        assert_eq!(peak.load(Ordering::SeqCst), 1);
        assert!(coord.has_queued());
        assert_eq!(
            coord.pending().expect("one pending").generation,
            101,
            "only the latest intent survives"
        );
        assert_eq!(coord.superseded_count(), 100);

        // Release the held worker and join it.
        barrier.wait();
        held.join().expect("held worker joins");
        assert_eq!(coord.running_scan_count(), 0);
        assert_eq!(coord.peak_running(), 1);
        assert_eq!(peak.load(Ordering::SeqCst), 1);

        // Completion drives exactly the latest pending.
        let pending = coord.finish_active(1, 1).expect("latest pending");
        assert_eq!(pending.generation, 101);
        assert!(!coord.has_queued());
    }

    #[test]
    fn diagnostics_expose_counts_without_paths() {
        let coord = FileTreeScanCoordinator::new();
        assert_eq!(coord.try_admit(request(1, 1, "/secret-root")), Admit::Run);
        let diagnostics = coord.diagnostics();
        assert!(diagnostics.contains("active=1"));
        assert!(
            !diagnostics.contains("secret-root"),
            "no paths in diagnostics"
        );
    }
}
